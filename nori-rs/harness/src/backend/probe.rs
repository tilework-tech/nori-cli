//! Pre-session agent probe.
//!
//! Spawns the agent, completes the ACP `initialize` handshake, fetches
//! `session/list`, and tears the child down — WITHOUT creating a session.
//! This powers the picker-first `nori cloud` entry: nothing is claimed until
//! the user explicitly picks "start new" or an existing session.

use super::AcpBackendConfig;
use super::enhance_agent_error;
use super::get_agent_config;
use super::nori_client_mcp;
use crate::connection::AcpSessionSummary;
use crate::connection::acp_connection::AcpConnection;

/// How long the probe waits for its child to exit after stdin EOF before
/// killing it. EOF is a non-terminal detach signal for cloud agents
/// (sessions PR #1276), and the probe never created a session, so there is
/// nothing worth waiting the full shutdown grace for — but a cooperative
/// child (which exits in milliseconds) gets to finish cleanly.
const PROBE_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

/// Why a probe produced no session list. `SessionListUnsupported` is the
/// expected case for agents without the lifecycle (older handroll, local
/// agents) — callers fall back silently; `Failed` is a real error worth
/// showing the user.
#[derive(Debug)]
pub enum ProbeError {
    /// The agent does not advertise `session/list`.
    SessionListUnsupported(String),
    /// Spawn, initialize, or the list call itself failed.
    Failed(String),
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeError::SessionListUnsupported(message) | ProbeError::Failed(message) => {
                write!(f, "{message}")
            }
        }
    }
}

impl std::error::Error for ProbeError {}

/// What the probe learned about the agent before any session exists.
#[derive(Debug, Clone, PartialEq)]
pub struct AgentSessionsProbe {
    /// The agent's advertised capability view, exactly as the TUI projects it.
    pub capabilities: nori_protocol::AgentCapabilitiesView,
    /// The agent's `session/list` rows (broker titles included, when present).
    pub sessions: Vec<AcpSessionSummary>,
}

/// Probe using the ambient Nori config — what the TUI entry flow calls.
/// Only the inputs the probe actually needs are resolved (agent, cwd, and
/// the wire-log proxy setting); no session-level configuration is involved.
pub async fn probe_agent_sessions_for(
    agent: &str,
    cwd: &std::path::Path,
) -> Result<AgentSessionsProbe, ProbeError> {
    let nori_config = crate::config::NoriConfig::load().unwrap_or_default();
    probe(agent, cwd, nori_config.acp_proxy).await
}

/// Spawn the agent, read capabilities, list sessions, and shut the child
/// down. Never calls `session/new`, `session/load`, or `session/resume`.
pub async fn probe_agent_sessions(
    config: &AcpBackendConfig,
) -> Result<AgentSessionsProbe, ProbeError> {
    probe(&config.agent, &config.cwd, config.acp_proxy.clone()).await
}

async fn probe(
    agent: &str,
    cwd: &std::path::Path,
    acp_proxy: crate::config::AcpProxyConfig,
) -> Result<AgentSessionsProbe, ProbeError> {
    let agent_config = get_agent_config(agent).map_err(|e| ProbeError::Failed(format!("{e:#}")))?;
    let connection = AcpConnection::spawn(&agent_config, cwd, acp_proxy)
        .await
        .map_err(|e| ProbeError::Failed(format!("{:#}", enhance_agent_error(e, &agent_config))))?;

    let capabilities = nori_client_mcp::agent_capabilities_view(&connection);
    if !capabilities.session_list {
        connection.shutdown_with_grace(PROBE_SHUTDOWN_GRACE).await;
        return Err(ProbeError::SessionListUnsupported(format!(
            "Agent '{agent}' does not advertise session listing (session/list), so there are \
             no sessions to pick from"
        )));
    }

    let sessions = match connection.list_sessions(cwd).await {
        Ok(sessions) => sessions,
        Err(e) => {
            connection.shutdown_with_grace(PROBE_SHUTDOWN_GRACE).await;
            return Err(ProbeError::Failed(format!(
                "{:#}",
                enhance_agent_error(e, &agent_config)
            )));
        }
    };

    connection.shutdown_with_grace(PROBE_SHUTDOWN_GRACE).await;
    Ok(AgentSessionsProbe {
        capabilities,
        sessions,
    })
}
