//! Thin adapter between the harness session runtime and the TUI event loop.
//!
//! All session orchestration (backend config assembly, connect/shutdown/
//! timeout race and session control) lives in
//! `nori_harness::runtime`; this module only builds a launch spec from the codex
//! resolved Nori config and maps session events onto [`AppEvent`]s.

use nori_config::NoriConfig as Config;
use nori_harness::SessionContext;
use nori_harness::get_agent_display_name;
use nori_harness::list_available_agents;
use nori_harness::runtime::AgentPrepareSpec;
pub(crate) use nori_harness::runtime::HarnessHandle;
use nori_harness::runtime::PreparedAgent;
use nori_harness::runtime::SessionLaunchSpec;
use nori_harness::runtime::SessionResume;
use nori_harness::runtime::SessionStart;
use nori_harness::runtime::launch_session;
use nori_harness::runtime::prepare_and_launch_session;
use nori_installed::AnalyticsReporter;
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
}

/// Spawn the agent bootstrapper and return its typed handle.
///
/// Looks up the agent in the ACP registry. If found, spawns an ACP agent.
/// Otherwise, emits an error and opens the agent picker.
pub(crate) fn spawn_agent(
    config: Config,
    app_event_tx: AppEventSender,
    generation: crate::app_event::SessionGeneration,
    fork_context: Option<String>,
    analytics: Option<AnalyticsReporter>,
) -> SpawnAgentResult {
    match nori_harness::get_agent_config(&config.active_agent) {
        Ok(_) => prepare_and_launch_acp_agent(
            config,
            app_event_tx,
            generation,
            fork_context,
            SessionStart::New,
            analytics,
        ),
        Err(_) => {
            let agent_name = config.active_agent;
            let known: Vec<String> = list_available_agents()
                .iter()
                .map(|a| a.agent_name.clone())
                .collect();
            let error_msg = format!(
                "Agent '{agent_name}' is not registered as an ACP agent. \
                 Known ACP agents: {}",
                known.join(", ")
            );
            spawn_error_agent(agent_name, error_msg, app_event_tx);
            SpawnAgentResult { handle: None }
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
    transcript: Option<nori_harness::transcript::Transcript>,
    app_event_tx: AppEventSender,
    generation: crate::app_event::SessionGeneration,
    analytics: Option<AnalyticsReporter>,
) -> SpawnAgentResult {
    prepare_and_launch_acp_agent(
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
    let session = prepare_and_launch_session(prepare_spec, start);
    forward_launched_session(session, app_event_tx, generation, analytics)
}

pub(crate) fn agent_prepare_spec(
    config: Config,
    initial_context: Option<String>,
) -> AgentPrepareSpec {
    AgentPrepareSpec {
        config: Arc::new(config),
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        session_context: Some(SessionContext {
            with_http_mcp: include_str!("../../session_context_http_mcp.md").to_string(),
            without_http_mcp: include_str!("../../session_context.md").to_string(),
        }),
        initial_context,
    }
}

/// Activate an already prepared connection and forward its session events.
pub(crate) fn launch_prepared_agent(
    agent: PreparedAgent,
    start: SessionStart,
    app_event_tx: AppEventSender,
    generation: crate::app_event::SessionGeneration,
    analytics: Option<AnalyticsReporter>,
) -> SpawnAgentResult {
    let session = launch_session(SessionLaunchSpec { agent, start });
    forward_launched_session(session, app_event_tx, generation, analytics)
}

fn forward_launched_session(
    mut session: nori_harness::runtime::LaunchedSession,
    app_event_tx: AppEventSender,
    generation: crate::app_event::SessionGeneration,
    analytics: Option<AnalyticsReporter>,
) -> SpawnAgentResult {
    if let Some(reporter) = analytics.as_ref() {
        session.handle = reporter.attach(session.handle);
    }
    let handle = Some(session.handle.clone());

    tokio::spawn(async move {
        while let Some(event) = session.events.recv().await {
            app_event_tx.send(AppEvent::SessionEvent { generation, event });
        }
    });

    SpawnAgentResult { handle }
}
