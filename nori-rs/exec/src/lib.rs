//! Scriptable, terminal-independent execution over Nori's ACP harness.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context as TaskContext;
use std::task::Poll;

use agent_client_protocol::Agent;
use agent_client_protocol::ByteStreams;
use agent_client_protocol::Client;
use agent_client_protocol::ConnectionTo;
use agent_client_protocol::Responder;
use anyhow::Context;
use futures::io::AsyncRead;
use nori_config::NoriConfig;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::HarnessHandle;
use nori_harness::runtime::SessionStart;
use nori_harness::runtime::prepare_and_launch_session;
use nori_installed::AnalyticsReporter;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;

/// Completed output from a finite plaintext execution.
pub struct PlaintextOutcome {
    /// Complete assistant text emitted during the prompt turn.
    pub output: String,
    /// Whether Nori automatically rejected a delegated permission request.
    pub permission_denied: bool,
}

/// Execute one prompt and collect its final assistant text.
pub async fn run_plaintext(
    config: Arc<NoriConfig>,
    cli_version: String,
    prompt: String,
    analytics: Option<AnalyticsReporter>,
) -> anyhow::Result<PlaintextOutcome> {
    let mut launched = prepare_and_launch_session(
        AgentPrepareSpec {
            config,
            cli_version,
            session_context: None,
            initial_context: None,
        },
        SessionStart::New,
    );
    if let Some(reporter) = analytics.as_ref() {
        launched.handle = reporter.attach(launched.handle);
    }
    let request_id = launched
        .handle
        .prompt(vec![acp::ContentBlock::Text(acp::TextContent::new(prompt))])
        .await?;
    let result = collect_prompt(
        &launched.handle,
        &mut launched.events,
        request_id,
        PermissionMode::Reject,
        None,
    )
    .await;
    let shutdown_result = launched.handle.shutdown().await;
    let outcome = match result {
        Ok(result) => {
            shutdown_result?;
            Ok(PlaintextOutcome {
                output: result.output,
                permission_denied: result.permission_denied,
            })
        }
        Err(error) => Err(error),
    };
    if let Some(reporter) = analytics {
        reporter.flush();
    }
    outcome
}

#[derive(Clone, Copy)]
enum PermissionMode {
    Reject,
    Forward,
}

struct PromptResult {
    output: String,
    response: acp::PromptResponse,
    permission_denied: bool,
}

async fn collect_prompt(
    handle: &HarnessHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    prompt_request_id: acp::RequestId,
    permission_mode: PermissionMode,
    client: Option<&ConnectionTo<Client>>,
) -> anyhow::Result<PromptResult> {
    let mut output = String::new();
    let mut permission_denied = false;
    while let Some(event) = events.recv().await {
        match event {
            SessionEvent::Acp(AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(notification),
            )) => {
                if let acp::SessionUpdate::AgentMessageChunk(chunk) = notification.update
                    && let acp::ContentBlock::Text(text) = chunk.content
                {
                    output.push_str(&text.text);
                }
            }
            SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request: acp::AgentRequest::RequestPermissionRequest(request),
            }) => {
                let response = match permission_mode {
                    PermissionMode::Reject => {
                        permission_denied = true;
                        reject_permission(&request)
                    }
                    PermissionMode::Forward => {
                        let client = client.context("ACP client connection is unavailable")?;
                        match client.send_request(request).block_task().await {
                            Ok(response) => response,
                            Err(error) => {
                                handle
                                    .respond_to_agent(
                                        request_id,
                                        Ok(acp::ClientResponse::RequestPermissionResponse(
                                            acp::RequestPermissionResponse::new(
                                                acp::RequestPermissionOutcome::Cancelled,
                                            ),
                                        )),
                                    )
                                    .await?;
                                return Err(error)
                                    .context("ACP caller did not answer the permission request");
                            }
                        }
                    }
                };
                handle
                    .respond_to_agent(
                        request_id,
                        Ok(acp::ClientResponse::RequestPermissionResponse(response)),
                    )
                    .await?;
            }
            SessionEvent::Acp(AcpEvent::Request { request_id, .. }) => {
                handle
                    .respond_to_agent(request_id, Err(acp::Error::method_not_found()))
                    .await?;
            }
            SessionEvent::Acp(AcpEvent::Response {
                request_id,
                response,
            }) if request_id == prompt_request_id => {
                return match response {
                    Ok(acp::AgentResponse::PromptResponse(response)) => Ok(PromptResult {
                        output,
                        response,
                        permission_denied,
                    }),
                    Ok(_) => {
                        anyhow::bail!("ACP agent returned the wrong response to session/prompt")
                    }
                    Err(error) => Err(anyhow::anyhow!(error.to_string())),
                };
            }
            SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                let detail = ended
                    .message
                    .unwrap_or_else(|| format!("session ended: {:?}", ended.reason));
                anyhow::bail!(detail);
            }
            SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                anyhow::bail!(failure.message);
            }
            _ => {}
        }
    }
    anyhow::bail!("ACP session ended before session/prompt completed")
}

fn reject_permission(request: &acp::RequestPermissionRequest) -> acp::RequestPermissionResponse {
    let rejected = request.options.iter().find(|option| {
        matches!(
            option.kind,
            acp::PermissionOptionKind::RejectOnce | acp::PermissionOptionKind::RejectAlways
        )
    });
    let outcome = rejected.map_or(acp::RequestPermissionOutcome::Cancelled, |option| {
        acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
            option.option_id.clone(),
        ))
    });
    acp::RequestPermissionResponse::new(outcome)
}

struct FacadeSession {
    session_id: acp::SessionId,
    handle: HarnessHandle,
    events: Option<UnboundedReceiver<SessionEvent>>,
    prompt_started: bool,
}

struct FacadeState {
    base_config: Arc<NoriConfig>,
    cli_version: String,
    analytics: Option<AnalyticsReporter>,
    session: Option<FacadeSession>,
}

struct ErrorOnEof<R> {
    inner: R,
    eof: Arc<AtomicBool>,
    shutdown_delay: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl<R> ErrorOnEof<R> {
    fn new(inner: R, eof: Arc<AtomicBool>) -> Self {
        Self {
            inner,
            eof,
            shutdown_delay: None,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for ErrorOnEof<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buffer: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        if let Some(delay) = self.shutdown_delay.as_mut() {
            return match delay.as_mut().poll(cx) {
                Poll::Ready(()) => Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "ACP caller closed stdin",
                ))),
                Poll::Pending => Poll::Pending,
            };
        }
        match Pin::new(&mut self.inner).poll_read(cx, buffer) {
            Poll::Ready(Ok(0)) if !buffer.is_empty() => {
                self.eof.store(true, Ordering::SeqCst);
                self.shutdown_delay = Some(Box::pin(tokio::time::sleep(
                    std::time::Duration::from_millis(100),
                )));
                self.poll_read(cx, buffer)
            }
            result => result,
        }
    }
}

/// Serve a bounded ACP agent facade over stdin/stdout.
pub async fn run_acp(
    config: Arc<NoriConfig>,
    cli_version: String,
    analytics: Option<AnalyticsReporter>,
) -> anyhow::Result<()> {
    let state = Arc::new(Mutex::new(FacadeState {
        base_config: config,
        cli_version,
        analytics: analytics.clone(),
        session: None,
    }));

    let eof = Arc::new(AtomicBool::new(false));
    let connection_result = Agent
        .builder()
        .name("nori-exec")
        .on_receive_request(
            async move |request: acp::InitializeRequest,
                        responder: Responder<acp::InitializeResponse>,
                        _cx: ConnectionTo<Client>| {
                responder.respond(
                    acp::InitializeResponse::new(request.protocol_version)
                        .agent_info(acp::Implementation::new("nori", env!("CARGO_PKG_VERSION"))),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: acp::NewSessionRequest,
                            responder: Responder<acp::NewSessionResponse>,
                            _cx: ConnectionTo<Client>| {
                    if !request.mcp_servers.is_empty() || !request.additional_directories.is_empty()
                    {
                        return responder.respond_with_error(acp::Error::invalid_params());
                    }
                    let (mut config, cli_version, analytics) = {
                        let state = state.lock().await;
                        if state.session.is_some() {
                            return responder.respond_with_error(acp::Error::invalid_params());
                        }
                        (
                            (*state.base_config).clone(),
                            state.cli_version.clone(),
                            state.analytics.clone(),
                        )
                    };
                    config.cwd = request.cwd;
                    let mut launched = prepare_and_launch_session(
                        AgentPrepareSpec {
                            config: Arc::new(config),
                            cli_version,
                            session_context: None,
                            initial_context: None,
                        },
                        SessionStart::New,
                    );
                    if let Some(reporter) = analytics.as_ref() {
                        launched.handle = reporter.attach(launched.handle);
                    }
                    let session_id = loop {
                        match launched.events.recv().await {
                            Some(SessionEvent::Nori(NoriEvent::SessionStarted(started))) => {
                                break started.acp_session_id;
                            }
                            Some(SessionEvent::Nori(NoriEvent::SessionEnded(ended))) => {
                                return responder.respond_with_error(acp::Error::new(
                                    -32000,
                                    ended
                                        .message
                                        .unwrap_or_else(|| "failed to start session".to_string()),
                                ));
                            }
                            Some(_) => {}
                            None => {
                                return responder.respond_with_error(acp::Error::new(
                                    -32000,
                                    "ACP session ended during startup",
                                ));
                            }
                        }
                    };
                    let config_options = launched.handle.get_session_config().await;
                    state.lock().await.session = Some(FacadeSession {
                        session_id: session_id.clone(),
                        handle: launched.handle,
                        events: Some(launched.events),
                        prompt_started: false,
                    });
                    responder.respond(
                        acp::NewSessionResponse::new(session_id).config_options(config_options),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: acp::SetSessionConfigOptionRequest,
                            responder: Responder<acp::SetSessionConfigOptionResponse>,
                            _cx: ConnectionTo<Client>| {
                    let handle = {
                        let state = state.lock().await;
                        let Some(session) = state.session.as_ref() else {
                            return responder.respond_with_error(acp::Error::invalid_params());
                        };
                        if session.session_id != request.session_id || session.prompt_started {
                            return responder.respond_with_error(acp::Error::invalid_params());
                        }
                        session.handle.clone()
                    };
                    let Some(value) = request.value.as_value_id().map(ToString::to_string) else {
                        return responder.respond_with_error(acp::Error::invalid_params());
                    };
                    match handle
                        .set_session_config_option(request.config_id.to_string(), value)
                        .await
                    {
                        Ok(options) => {
                            responder.respond(acp::SetSessionConfigOptionResponse::new(options))
                        }
                        Err(error) => {
                            responder.respond_with_error(acp::Error::new(-32000, error.to_string()))
                        }
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |request: acp::PromptRequest,
                            responder: Responder<acp::PromptResponse>,
                            cx: ConnectionTo<Client>| {
                    let (handle, mut events) = {
                        let mut state = state.lock().await;
                        let Some(session) = state.session.as_mut() else {
                            return responder.respond_with_error(acp::Error::invalid_params());
                        };
                        if session.session_id != request.session_id || session.prompt_started {
                            return responder.respond_with_error(acp::Error::invalid_params());
                        }
                        session.prompt_started = true;
                        let Some(events) = session.events.take() else {
                            return responder.respond_with_error(acp::Error::invalid_params());
                        };
                        (session.handle.clone(), events)
                    };
                    let client = cx.clone();
                    cx.spawn(async move {
                        let request_id = match handle.prompt(request.prompt).await {
                            Ok(request_id) => request_id,
                            Err(error) => {
                                return responder.respond_with_error(acp::Error::new(
                                    -32000,
                                    error.to_string(),
                                ));
                            }
                        };
                        let result = collect_prompt(
                            &handle,
                            &mut events,
                            request_id,
                            PermissionMode::Forward,
                            Some(&client),
                        )
                        .await;
                        if let Err(error) = handle.shutdown().await {
                            return responder
                                .respond_with_error(acp::Error::new(-32000, error.to_string()));
                        }
                        match result {
                            Ok(result) => {
                                if !result.output.is_empty() {
                                    client.send_notification(acp::SessionNotification::new(
                                        request.session_id,
                                        acp::SessionUpdate::AgentMessageChunk(
                                            acp::ContentChunk::new(acp::ContentBlock::Text(
                                                acp::TextContent::new(result.output),
                                            )),
                                        ),
                                    ))?;
                                }
                                responder.respond(result.response)
                            }
                            Err(error) => responder
                                .respond_with_error(acp::Error::new(-32000, error.to_string())),
                        }
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let state = state.clone();
                async move |notification: acp::CancelNotification, _cx: ConnectionTo<Client>| {
                    let handle = {
                        let state = state.lock().await;
                        state.session.as_ref().and_then(|session| {
                            (session.session_id == notification.session_id)
                                .then(|| session.handle.clone())
                        })
                    };
                    if let Some(handle) = handle {
                        handle
                            .cancel()
                            .await
                            .map_err(|error| acp::Error::new(-32000, error.to_string()))?;
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(ByteStreams::new(
            tokio::io::stdout().compat_write(),
            ErrorOnEof::new(tokio::io::stdin().compat(), eof.clone()),
        ))
        .await;

    let handle = state
        .lock()
        .await
        .session
        .as_ref()
        .map(|session| session.handle.clone());
    if let Some(handle) = handle {
        let _ = handle.cancel().await;
        let _ = handle.shutdown().await;
    }
    if let Some(reporter) = analytics {
        reporter.flush();
    }
    if eof.load(Ordering::SeqCst) {
        Ok(())
    } else {
        connection_result.map_err(Into::into)
    }
}
