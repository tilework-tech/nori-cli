//! Thin adapter between the harness session runtime and the TUI event loop.
//!
//! All session orchestration (backend config assembly, connect/shutdown/
//! timeout race and session control) lives in `nori_harness`. Every session,
//! including the hidden `/agent` switch candidate, starts through the app's
//! [`SessionManager`]; this module only builds a launch spec from the resolved
//! Nori config and maps the session's envelopes onto [`AppEvent`]s.

use nori_config::NoriConfig as Config;
use nori_harness::SessionContext;
use nori_harness::get_agent_display_name;
use nori_harness::list_available_agents;
use nori_harness::runtime::AgentPrepareSpec;
pub(crate) use nori_harness::runtime::HarnessHandle;
use nori_harness::runtime::PreparedAgent;
use nori_harness::runtime::SessionResume;
use nori_harness::runtime::SessionStart;
use nori_harness::session_manager::SessionAgent;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionSource;
use nori_harness::session_manager::StartSession;
use nori_installed::AnalyticsReporter;
use nori_protocol::NoriEvent;
use nori_protocol::RequestFailure;
use nori_protocol::RequestFailureKind;
use nori_protocol::SessionEndReason;
use nori_protocol::SessionEnded;
use nori_protocol::SessionEvent;
use std::sync::Arc;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

static NEXT_SESSION_GENERATION: AtomicI64 = AtomicI64::new(1);

pub(crate) fn next_session_generation() -> crate::app_event::SessionGeneration {
    NEXT_SESSION_GENERATION.fetch_add(1, Ordering::Relaxed)
}

/// Result of spawning a harness session.
pub(crate) struct SpawnAgentResult {
    pub handle: Option<HarnessHandle>,
    /// The session's key in the session manager.
    pub conversation_id: Option<nori_harness::ConversationId>,
}

/// Spawn the agent bootstrapper and return its typed handle.
///
/// Looks up the agent in the ACP registry. If found, spawns an ACP agent.
/// Otherwise, emits an error and opens the agent picker.
pub(crate) fn spawn_agent(
    manager: &SessionManager,
    config: Config,
    app_event_tx: AppEventSender,
    generation: crate::app_event::SessionGeneration,
    fork_context: Option<String>,
    analytics: Option<AnalyticsReporter>,
) -> SpawnAgentResult {
    let slug = nori_harness::resolve_agent_slug(&config.active_agent);
    match nori_harness::registry::get_agent_launch_config(
        &slug,
        config.agent_token_providers.get(&slug),
    ) {
        Ok(_) => prepare_and_launch_acp_agent(
            manager,
            config,
            app_event_tx,
            generation,
            fork_context,
            SessionStart::New,
            analytics,
        ),
        Err(error) => {
            let agent_name = config.active_agent;
            let known: Vec<String> = list_available_agents()
                .iter()
                .map(|a| a.agent_name.clone())
                .collect();
            let error_msg = if config.agent_token_providers.contains_key(&slug) {
                error.to_string()
            } else {
                format!(
                    "Agent '{agent_name}' is not registered as an ACP agent. \
                 Known ACP agents: {}",
                    known.join(", ")
                )
            };
            spawn_error_agent(agent_name, error_msg, app_event_tx);
            SpawnAgentResult {
                handle: None,
                conversation_id: None,
            }
        }
    }
}

/// Spawn an ACP agent backend that resumes a previous session.
///
/// If the agent supports `session/load`, server-side resume is used.
/// Otherwise, falls back to client-side replay using the provided transcript.
pub(crate) fn spawn_acp_agent_resume(
    manager: &SessionManager,
    config: Config,
    acp_session_id: Option<String>,
    transcript: Option<nori_harness::transcript::Transcript>,
    app_event_tx: AppEventSender,
    generation: crate::app_event::SessionGeneration,
    analytics: Option<AnalyticsReporter>,
) -> SpawnAgentResult {
    prepare_and_launch_acp_agent(
        manager,
        config,
        app_event_tx,
        generation,
        None,
        SessionStart::Resume(SessionResume {
            acp_session_id,
            transcript,
        }),
        analytics,
    )
}

/// Spawn an agent that emits an error and opens the agent picker.
///
/// This is used when the requested agent is not a valid ACP agent.
fn spawn_error_agent(agent_name: String, error_msg: String, app_event_tx: AppEventSender) {
    tokio::spawn(async move {
        tracing::error!("{}", error_msg);
        // Send AgentSpawnFailed so the user can select a different agent
        app_event_tx.send(AppEvent::AgentSpawnFailed {
            agent_name,
            error: error_msg,
        });
    });
}

/// Launch a session via the harness runtime and forward its events into the
/// TUI event loop.
fn prepare_and_launch_acp_agent(
    manager: &SessionManager,
    config: Config,
    app_event_tx: AppEventSender,
    generation: crate::app_event::SessionGeneration,
    fork_context: Option<String>,
    start: SessionStart,
    analytics: Option<AnalyticsReporter>,
) -> SpawnAgentResult {
    let display_name = get_agent_display_name(&config.active_agent);
    app_event_tx.send(AppEvent::AgentConnecting { display_name });

    let prepare_spec = agent_prepare_spec(config, fork_context);
    start_managed_session(
        manager,
        SessionAgent::Prepare(prepare_spec),
        start,
        app_event_tx,
        generation,
        analytics,
    )
}

pub(crate) fn agent_prepare_spec(
    config: Config,
    initial_context: Option<String>,
) -> AgentPrepareSpec {
    AgentPrepareSpec {
        config: Arc::new(config),
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        session_context: Some(terminal_session_context()),
        initial_context,
    }
}

pub(crate) fn terminal_session_context() -> SessionContext {
    SessionContext {
        with_http_mcp: include_str!("../../session_context_http_mcp.md").to_string(),
        without_http_mcp: include_str!("../../session_context.md").to_string(),
    }
}

/// Activate an already prepared connection and forward its session events.
pub(crate) fn launch_prepared_agent(
    manager: &SessionManager,
    agent: PreparedAgent,
    start: SessionStart,
    app_event_tx: AppEventSender,
    generation: crate::app_event::SessionGeneration,
    analytics: Option<AnalyticsReporter>,
) -> SpawnAgentResult {
    start_managed_session(
        manager,
        SessionAgent::Prepared(agent),
        start,
        app_event_tx,
        generation,
        analytics,
    )
}

/// Start a session in the session manager and forward its events, tagged
/// with `generation`, into the TUI event loop. An agent prepared for a fork
/// starts a fork of that parent; any other agent starts a user session.
fn start_managed_session(
    manager: &SessionManager,
    agent: SessionAgent,
    start: SessionStart,
    app_event_tx: AppEventSender,
    generation: crate::app_event::SessionGeneration,
    analytics: Option<AnalyticsReporter>,
) -> SpawnAgentResult {
    let parent = match &agent {
        SessionAgent::Prepared(agent) => agent.fork_parent(),
        SessionAgent::Prepare(_) => None,
    }
    .filter(|parent| manager.get(parent).is_some());
    let source = if parent.is_some() {
        SessionSource::Fork
    } else {
        SessionSource::User
    };
    let session = match manager.start(StartSession {
        agent,
        start,
        parent,
        source,
    }) {
        Ok(session) => session,
        Err(error) => {
            let message = format!("Failed to start ACP session: {error}");
            for event in [
                NoriEvent::RequestFailed(RequestFailure {
                    request_id: None,
                    message: message.clone(),
                    kind: RequestFailureKind::Fatal,
                }),
                NoriEvent::SessionEnded(SessionEnded {
                    reason: SessionEndReason::SpawnFailed,
                    message: Some(message),
                }),
            ] {
                app_event_tx.send(AppEvent::SessionEvent {
                    generation,
                    event: SessionEvent::Nori(event),
                });
            }
            return SpawnAgentResult {
                handle: None,
                conversation_id: None,
            };
        }
    };
    forward_managed_session(session, app_event_tx, generation, analytics)
}

/// Connect a managed session to the widget's ordered event stream.
pub(super) fn forward_managed_session(
    mut session: nori_harness::session_manager::ManagedSession,
    app_event_tx: AppEventSender,
    generation: crate::app_event::SessionGeneration,
    analytics: Option<AnalyticsReporter>,
) -> SpawnAgentResult {
    if let Some(reporter) = analytics.as_ref() {
        session.handle = reporter.attach(session.handle);
    }
    let handle = Some(session.handle.clone());

    tokio::spawn(async move {
        while let Some(envelope) = session.events.recv().await {
            app_event_tx.send(AppEvent::SessionEvent {
                generation,
                event: envelope.event,
            });
        }
    });

    SpawnAgentResult {
        handle,
        conversation_id: Some(session.conversation_id),
    }
}

/// Connect a child history and live subscription to its widget.
pub(super) fn forward_managed_view(
    id: nori_harness::ConversationId,
    mut subscription: nori_harness::remote_agent::ManagedSubscription,
    tx: AppEventSender,
    generation: crate::app_event::SessionGeneration,
) {
    tokio::spawn(async move {
        let mut current = id.to_string();
        let mut forward = |envelope: nori_protocol::SessionEnvelope| {
            if let SessionEvent::Nori(NoriEvent::SessionForked(fork)) = &envelope.event
                && fork.previous_conversation_id == current
            {
                current.clone_from(&fork.new_conversation_id);
            }
            if envelope.conversation_id == current {
                tx.send(AppEvent::SessionEvent {
                    generation,
                    event: envelope.event,
                });
            }
        };
        for envelope in subscription.replay {
            forward(envelope);
        }
        while let Some(envelope) = subscription.events.recv().await {
            forward(envelope);
        }
    });
}

#[cfg(test)]
mod delegated_view_tests {
    use super::*;

    #[tokio::test]
    async fn delegated_view_follows_fork_and_excludes_other_conversations() {
        let old = nori_harness::ConversationId::from_string("00000000-0000-0000-0000-000000000001")
            .unwrap();
        let new = "00000000-0000-0000-0000-000000000002";
        let envelope = |id: &str, seq: i64, event| nori_protocol::SessionEnvelope {
            conversation_id: id.into(),
            agent: "mock-model".into(),
            seq,
            event,
        };
        let notice = |text: &str| {
            SessionEvent::Nori(NoriEvent::Notice(nori_protocol::Notice {
                message: text.into(),
            }))
        };
        let (events, receiver) = tokio::sync::mpsc::channel(4);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        forward_managed_view(
            old,
            nori_harness::remote_agent::ManagedSubscription {
                replay: vec![
                    envelope(&old.to_string(), 1, notice("before fork")),
                    envelope(
                        new,
                        1,
                        SessionEvent::Nori(NoriEvent::SessionForked(
                            nori_protocol::SessionForked {
                                previous_conversation_id: old.to_string(),
                                new_conversation_id: new.into(),
                                new_acp_session_id: "fork-agent".into(),
                            },
                        )),
                    ),
                ],
                events: receiver,
            },
            AppEventSender::new(tx),
            7,
        );
        events
            .send(envelope("unrelated", 1, notice("unrelated activity")))
            .await
            .unwrap();
        events
            .send(envelope(new, 2, notice("child needs permission")))
            .await
            .unwrap();
        drop(events);
        let mut received = Vec::new();
        while let Some(event) = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
        {
            if let AppEvent::SessionEvent { event, .. } = event {
                received.push(event);
            }
        }
        assert_eq!(
            received.len(),
            3,
            "only the child process's events reach its view"
        );
        assert!(
            matches!(received.last(), Some(SessionEvent::Nori(NoriEvent::Notice(notice))) if notice.message == "child needs permission")
        );
    }
}
