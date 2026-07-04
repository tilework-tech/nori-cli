//! Thin adapter between the harness session runtime and the TUI event loop.
//!
//! All session orchestration (backend config assembly, connect/shutdown/
//! timeout race, op forwarding, session-control commands) lives in
//! `nori_acp::runtime`; this module only builds a launch spec from the codex
//! `Config` and maps [`SessionEvent`]s onto [`AppEvent`]s.

use codex_core::config::Config;
use codex_protocol::protocol::Op;
use nori_acp::get_agent_display_name;
use nori_acp::list_available_agents;
use nori_acp::runtime::SessionEvent;
use nori_acp::runtime::SessionLaunchSpec;
use nori_acp::runtime::SessionResume;
use nori_acp::runtime::launch_session;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;

#[cfg(test)]
pub(crate) use nori_acp::runtime::AcpAgentCommand;
pub(crate) use nori_acp::runtime::AcpAgentHandle;
#[cfg(test)]
pub(crate) use nori_acp::runtime::drain_until_shutdown;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;

/// Result of spawning an agent, which may include an ACP handle for model control.
pub(crate) struct SpawnAgentResult {
    /// The Op sender for submitting operations to the agent.
    pub op_tx: UnboundedSender<Op>,
    /// Optional ACP handle for session control (only present in ACP mode).
    pub acp_handle: Option<AcpAgentHandle>,
}

/// Spawn the agent bootstrapper and op forwarding loop, returning a result
/// that includes the Op sender and optionally an ACP handle for model control.
///
/// Looks up the agent in the ACP registry. If found, spawns an ACP agent.
/// Otherwise, emits an error and opens the agent picker.
pub(crate) fn spawn_agent(
    config: Config,
    app_event_tx: AppEventSender,
    fork_context: Option<String>,
) -> SpawnAgentResult {
    match nori_acp::get_agent_config(&config.model) {
        Ok(_) => launch_acp_agent(config, app_event_tx, fork_context, None),
        Err(_) => {
            let agent_name = config.model;
            let known: Vec<String> = list_available_agents()
                .iter()
                .map(|a| a.agent_name.clone())
                .collect();
            let error_msg = format!(
                "Agent '{agent_name}' is not registered as an ACP agent. \
                 Known ACP agents: {}",
                known.join(", ")
            );
            let op_tx = spawn_error_agent(agent_name, error_msg, app_event_tx);
            SpawnAgentResult {
                op_tx,
                acp_handle: None,
            }
        }
    }
}

/// Spawn an ACP agent backend that resumes a previous session.
///
/// If the agent supports `session/load`, server-side resume is used.
/// Otherwise, falls back to client-side replay using the provided transcript.
pub(crate) fn spawn_acp_agent_resume(
    config: Config,
    acp_session_id: Option<String>,
    transcript: Option<nori_acp::transcript::Transcript>,
    app_event_tx: AppEventSender,
) -> SpawnAgentResult {
    launch_acp_agent(
        config,
        app_event_tx,
        None,
        Some(SessionResume {
            acp_session_id,
            transcript,
        }),
    )
}

/// Spawn an agent that emits an error and opens the agent picker.
///
/// This is used when the requested agent is not a valid ACP agent.
fn spawn_error_agent(
    agent_name: String,
    error_msg: String,
    app_event_tx: AppEventSender,
) -> UnboundedSender<Op> {
    let (codex_op_tx, _codex_op_rx) = unbounded_channel::<Op>();

    tokio::spawn(async move {
        tracing::error!("{}", error_msg);
        // Send AgentSpawnFailed so the user can select a different agent
        app_event_tx.send(AppEvent::AgentSpawnFailed {
            agent_name,
            error: error_msg,
        });
    });

    codex_op_tx
}

/// Launch a session via the harness runtime and forward its events into the
/// TUI event loop.
fn launch_acp_agent(
    config: Config,
    app_event_tx: AppEventSender,
    fork_context: Option<String>,
    resume: Option<SessionResume>,
) -> SpawnAgentResult {
    // Emit "Connecting" status before spawning the backend
    let display_name = get_agent_display_name(&config.model);
    app_event_tx.send(AppEvent::AgentConnecting { display_name });

    let spec = SessionLaunchSpec {
        agent: config.model.clone(),
        cwd: config.cwd.clone(),
        approval_policy: config.approval_policy,
        sandbox_policy: config.sandbox_policy.clone(),
        notify: config.notify.clone(),
        mcp_servers: config.mcp_servers.clone(),
        mcp_oauth_credentials_store_mode: config.mcp_oauth_credentials_store_mode,
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        session_context: Some(include_str!("../../session_context.md").to_string()),
        initial_context: fork_context,
        resume,
    };
    let agent_name = config.model;

    let mut session = launch_session(spec);
    let acp_handle = Some(session.handle.clone());
    let op_tx = session.op_tx.clone();

    tokio::spawn(async move {
        while let Some(event) = session.events.recv().await {
            match event {
                SessionEvent::Backend(backend_event) => match *backend_event {
                    nori_acp::BackendEvent::Control(event) => {
                        app_event_tx.send(AppEvent::CodexEvent(event));
                    }
                    nori_acp::BackendEvent::Client(client_event) => {
                        app_event_tx.send(AppEvent::ClientEvent(client_event));
                    }
                },
                SessionEvent::SpawnFailed { error } => {
                    app_event_tx.send(AppEvent::AgentSpawnFailed {
                        agent_name: agent_name.clone(),
                        error,
                    });
                }
                SessionEvent::ShutdownRequested => {
                    app_event_tx.send(AppEvent::ExitRequest);
                }
            }
        }
    });

    SpawnAgentResult { op_tx, acp_handle }
}
