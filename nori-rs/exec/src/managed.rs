//! Feature-gated managed sessions behind the existing ACP stdio facade.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use agent_client_protocol::Agent;
use agent_client_protocol::ByteStreams;
use agent_client_protocol::Client;
use agent_client_protocol::ConnectionTo;
use agent_client_protocol::Responder;
use agent_client_protocol::UntypedMessage;
use nori_config::NoriConfig;
use nori_harness::remote_agent::ManagedAgent;
use nori_harness::remote_agent::ManagedRemoteHost;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::HarnessHandle;
use nori_harness::runtime::SessionStart;
use nori_harness::session_manager::SessionAgent;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionManagerConfig;
use nori_harness::session_manager::SessionSource;
use nori_harness::session_manager::StartSession;
use nori_installed::AnalyticsReporter;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use serde_json::Value;
use serde_json::json;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;

use super::ErrorOnEof;
use super::FacadeSession;
use super::PermissionMode;
use super::collect_prompt;

fn internal(error: impl std::fmt::Display) -> acp::Error {
    acp::Error::new(-32000, error.to_string())
}

/// Launch through the manager while retaining the standard facade projection.
/// The manager keeps the original envelopes for extension subscribers.
pub(super) fn launch(
    manager: &SessionManager,
    spec: AgentPrepareSpec,
    analytics: Option<&AnalyticsReporter>,
) -> anyhow::Result<(
    acp::SessionId,
    HarnessHandle,
    mpsc::UnboundedReceiver<SessionEvent>,
)> {
    let session = manager.start(StartSession {
        agent: SessionAgent::Prepare(spec),
        start: SessionStart::New,
        parent: None,
        source: SessionSource::User,
    })?;
    let id = acp::SessionId::new(session.conversation_id.to_string());
    let mut handle = session.handle;
    if let Some(reporter) = analytics {
        handle = reporter.attach(handle);
    }
    let (tx, rx) = mpsc::unbounded_channel();
    let mut envelopes = session.events;
    let outward = id.clone();
    tokio::spawn(async move {
        while let Some(envelope) = envelopes.recv().await {
            let mut event = envelope.event;
            match &mut event {
                SessionEvent::Acp(AcpEvent::Notification(
                    acp::AgentNotification::SessionNotification(notification),
                )) => notification.session_id = outward.clone(),
                SessionEvent::Acp(AcpEvent::Request {
                    request: acp::AgentRequest::RequestPermissionRequest(request),
                    ..
                }) => request.session_id = outward.clone(),
                _ => {}
            }
            if tx.send(event).is_err() {
                break;
            }
        }
    });
    Ok((id, handle, rx))
}

async fn forward_subscription(
    mut subscription: nori_harness::remote_agent::ManagedSubscription,
    mut send: impl FnMut(UntypedMessage) -> bool,
) {
    for envelope in subscription.replay {
        let Ok(message) = UntypedMessage::new("_nori/session/event", &envelope) else {
            return;
        };
        if !send(message) {
            return;
        }
    }
    while let Some(envelope) = subscription.events.recv().await {
        let Ok(message) = UntypedMessage::new("_nori/session/event", &envelope) else {
            return;
        };
        if !send(message) {
            return;
        }
    }
    if let Ok(message) = UntypedMessage::new(
        "_nori/session/subscription_ended",
        json!({"reason":"replay_required"}),
    ) {
        send(message);
    }
}

pub(super) async fn run(
    config: Arc<NoriConfig>,
    cli_version: String,
    analytics: Option<AnalyticsReporter>,
) -> anyhow::Result<()> {
    let manager = SessionManager::new(SessionManagerConfig {
        max_sessions: config.max_sessions,
        ..Default::default()
    });
    let host = Arc::new(ManagedRemoteHost::new(manager.clone()));
    let sessions = Arc::new(Mutex::new(HashMap::<acp::SessionId, FacadeSession>::new()));
    let forwarding = Arc::new(Mutex::new(None::<tokio::task::JoinHandle<()>>));
    let eof = Arc::new(AtomicBool::new(false));
    let result = Agent.builder().name("nori-exec")
        .on_receive_request(
            async move |request: acp::InitializeRequest, responder: Responder<acp::InitializeResponse>, _cx: ConnectionTo<Client>| {
                let mut meta = acp::Meta::new();
                meta.insert("nori/multiSession".into(), json!({"version":1}));
                responder.respond(acp::InitializeResponse::new(request.protocol_version)
                    .agent_info(acp::Implementation::new("nori", env!("CARGO_PKG_VERSION")))
                    .agent_capabilities(acp::AgentCapabilities::new().meta(meta)))
            }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let manager = manager.clone(); let host = host.clone(); let sessions = sessions.clone(); let analytics = analytics.clone();
            async move |request: acp::NewSessionRequest, responder: Responder<acp::NewSessionResponse>, cx: ConnectionTo<Client>| {
                if !request.mcp_servers.is_empty() || !request.additional_directories.is_empty() {
                    return responder.respond_with_error(acp::Error::invalid_params());
                }
                let mut config = (*config).clone();
                config.cwd = request.cwd;
                let launched = launch(&manager, AgentPrepareSpec { config: Arc::new(config), cli_version: cli_version.clone(), session_context: None, initial_context: None }, analytics.as_ref());
                let (id, handle, mut events) = match launched {
                    Ok(launched) => launched,
                    Err(error) => return responder.respond_with_error(internal(error)),
                };
                let sessions = sessions.clone(); let host = host.clone();
                cx.spawn(async move {
                    loop {
                        match events.recv().await {
                            Some(SessionEvent::Nori(NoriEvent::SessionStarted(_))) => break,
                            Some(SessionEvent::Nori(NoriEvent::SessionEnded(ended))) => return responder.respond_with_error(internal(ended.message.unwrap_or_else(|| "session failed to start".into()))),
                            Some(_) => {},
                            None => return responder.respond_with_error(internal("session ended during startup")),
                        }
                    }
                    let options = handle.get_session_config().await;
                    let mut sessions = sessions.lock().await;
                    if sessions.is_empty() {
                        host.set_selected_session(nori_harness::ConversationId::from_string(&id.0).ok());
                    }
                    sessions.insert(id.clone(), FacadeSession { session_id: id.clone(), handle, events: None, prompt_started: false });
                    responder.respond(acp::NewSessionResponse::new(id).config_options(options))
                })?;
                Ok(())
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let sessions = sessions.clone();
            async move |request: acp::SetSessionConfigOptionRequest, responder: Responder<acp::SetSessionConfigOptionResponse>, cx: ConnectionTo<Client>| {
                let handle = sessions.lock().await.get(&request.session_id).filter(|session| !session.prompt_started).map(|session| session.handle.clone());
                let Some(handle) = handle else { return responder.respond_with_error(acp::Error::invalid_params()); };
                let Some(value) = request.value.as_value_id().map(ToString::to_string) else { return responder.respond_with_error(acp::Error::invalid_params()); };
                cx.spawn(async move {
                    match handle.set_session_config_option(request.config_id.to_string(), value).await {
                        Ok(options) => responder.respond(acp::SetSessionConfigOptionResponse::new(options)),
                        Err(error) => responder.respond_with_error(internal(error)),
                    }
                })?;
                Ok(())
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let sessions = sessions.clone();
            let manager = manager.clone();
            async move |request: acp::PromptRequest, responder: Responder<acp::PromptResponse>, cx: ConnectionTo<Client>| {
                let handle = {
                    let mut sessions = sessions.lock().await;
                    let Some(session) = sessions.get_mut(&request.session_id) else { return responder.respond_with_error(acp::Error::invalid_params()); };
                    if session.prompt_started { return responder.respond_with_error(acp::Error::new(-32015, "session already has an active turn")); }
                    session.prompt_started = true;
                    session.handle.clone()
                };
                let sessions = sessions.clone(); let client = cx.clone(); let manager = manager.clone();
                cx.spawn(async move {
                    let result = async {
                        // Only resolution signals come from the manager. Prompt
                        // content stays on the raw turn-local subscription.
                        let id = nori_harness::ConversationId::from_string(&request.session_id.0)?;
                        let cursor = manager.replay_info(&id).map_or(0, |info| info.latest_seq);
                        let resolutions = manager.subscribe_from(Some(id), &HashMap::from([(id.to_string(), cursor)]), 256)?.events;
                        // Subscribe at the harness command boundary before admission,
                        // excluding output from earlier extension-owned turns.
                        let mut events = handle.subscribe_events().await?;
                        let request_id = handle.prompt_with_meta_if_idle(request.prompt, request.meta).await?;
                        collect_prompt(&handle, &mut events, request_id, PermissionMode::Forward, Some(&client), Some(&request.session_id), Some(resolutions)).await
                    }.await;
                    if let Some(session) = sessions.lock().await.get_mut(&request.session_id) {
                        session.prompt_started = false;
                    }
                    match result {
                        Ok(result) => {
                            if !result.output.is_empty() {
                                client.send_notification(acp::SessionNotification::new(request.session_id, acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(result.output))))))?;
                            }
                            responder.respond(result.response)
                        },
                        Err(error) => responder.respond_with_error(internal(error)),
                    }
                })?;
                Ok(())
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_request({
            let host = host.clone(); let forwarding = forwarding.clone();
            async move |request: UntypedMessage, responder: Responder<Value>, cx: ConnectionTo<Client>| {
                let reply = host.request(json!({"jsonrpc":"2.0","id":0,"method":request.method(),"params":request.params()})).await;
                if let Some(error) = reply.response.get("error") {
                    let error = serde_json::from_value(error.clone()).unwrap_or_else(|_| internal(error));
                    return responder.respond_with_error(error);
                }
                responder.respond(reply.response["result"].clone())?;
                if let Some(subscription) = reply.subscription {
                    let mut task = forwarding.lock().await;
                    if let Some(previous) = task.take() { previous.abort(); }
                    *task = Some(tokio::spawn(async move {
                        forward_subscription(subscription, |message| cx.send_notification(message).is_ok()).await;
                    }));
                }
                Ok(())
            }
        }, agent_client_protocol::on_receive_request!())
        .on_receive_notification({
            let sessions = sessions.clone();
            async move |request: acp::CancelNotification, _cx: ConnectionTo<Client>| {
                let handle = sessions.lock().await.get(&request.session_id).map(|session| session.handle.clone());
                if let Some(handle) = handle { handle.cancel().await.map_err(internal)?; }
                Ok(())
            }
        }, agent_client_protocol::on_receive_notification!())
        .connect_to(ByteStreams::new(tokio::io::stdout().compat_write(), ErrorOnEof::new(tokio::io::stdin().compat(), eof.clone()))).await;
    if let Some(task) = forwarding.lock().await.take() {
        task.abort();
    }
    manager.shutdown(Duration::ZERO).await;
    if let Some(reporter) = analytics {
        reporter.flush();
    }
    if eof.load(Ordering::SeqCst) {
        Ok(())
    } else {
        result.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nori_harness::remote_agent::ManagedSubscription;
    use nori_protocol::Notice;
    use nori_protocol::SessionEnvelope;

    #[tokio::test]
    async fn ended_subscription_delivers_buffered_events_then_requests_replay() {
        let envelope = |seq| SessionEnvelope {
            conversation_id: "example-session".into(),
            agent: "example-agent".into(),
            seq,
            event: SessionEvent::Nori(NoriEvent::Notice(Notice {
                message: "visible event".into(),
            })),
        };
        let (tx, rx) = mpsc::channel(1);
        tx.send(envelope(2)).await.unwrap();
        drop(tx);
        let subscription = ManagedSubscription {
            replay: vec![envelope(1)],
            events: rx,
        };
        let mut messages = Vec::new();
        forward_subscription(subscription, |message| {
            messages.push(message);
            true
        })
        .await;
        assert_eq!(
            messages
                .iter()
                .map(agent_client_protocol::UntypedMessage::method)
                .collect::<Vec<_>>(),
            [
                "_nori/session/event",
                "_nori/session/event",
                "_nori/session/subscription_ended"
            ]
        );
        assert_eq!(messages[0].params()["seq"], 1);
        assert_eq!(messages[1].params()["seq"], 2);
        assert_eq!(messages[2].params()["reason"], "replay_required");
    }
}
