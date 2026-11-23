//! Scriptable, terminal-independent execution over Nori's ACP harness.

use std::sync::Arc;

use nori_config::NoriConfig;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::HarnessHandle;
use nori_harness::runtime::SessionLaunchSpec;
use nori_harness::runtime::SessionStart;
use nori_harness::runtime::launch_session;
use nori_harness::runtime::prepare_agent;
use nori_installed::AnalyticsReporter;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEndReason;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use tokio::sync::mpsc::UnboundedReceiver;

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
    let agent = prepare_agent(AgentPrepareSpec {
        config,
        cli_version,
        session_context: None,
        initial_context: None,
    })
    .await?;
    let mut launched = launch_session(SessionLaunchSpec {
        agent,
        start: SessionStart::New,
    });
    if let Some(reporter) = analytics.as_ref() {
        launched.handle = reporter.attach(launched.handle);
    }
    let request_id = match launched
        .handle
        .prompt(vec![acp::ContentBlock::Text(acp::TextContent::new(prompt))])
        .await
    {
        Ok(id) => id,
        Err(error) => {
            // A failed launch can publish its diagnostic as the first pending prompt closes.
            let startup_failure = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                while let Some(event) = launched.events.recv().await {
                    match event {
                        SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                            if ended.reason == SessionEndReason::SpawnFailed {
                                return ended.message;
                            }
                            break;
                        }
                        SessionEvent::Nori(NoriEvent::SessionStarted(_)) => break,
                        _ => {}
                    }
                }
                None
            })
            .await;
            return Err(match startup_failure {
                Ok(Some(message)) => anyhow::anyhow!(message),
                Ok(None) | Err(_) => error,
            });
        }
    };
    let result = collect_prompt(&launched.handle, &mut launched.events, request_id).await;
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

struct PromptResult {
    output: String,
    permission_denied: bool,
}

/// Collect the turn's agent text, rejecting every permission request.
async fn collect_prompt(
    handle: &HarnessHandle,
    events: &mut UnboundedReceiver<SessionEvent>,
    prompt_request_id: acp::RequestId,
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
                permission_denied = true;
                let response = reject_permission(&request);
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
                    Ok(acp::AgentResponse::PromptResponse(_)) => Ok(PromptResult {
                        output,
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
