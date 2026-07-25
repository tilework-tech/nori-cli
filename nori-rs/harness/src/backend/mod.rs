//! Backend adapter for ACP agents in the TUI
//!
//! This module provides the private runtime behind the typed harness API.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use crate::ConversationId;
use anyhow::Result;
use codex_rmcp_client::OAuthCredentialsStoreMode;
use nori_config::AskForApproval;
use nori_config::McpServerConfig;
use nori_config::SandboxPolicy;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tracing::debug;
use tracing::warn;

use crate::connection::DelegatedRequest;
use crate::connection::acp_connection::AcpConnection;
use crate::registry::get_agent_config;
use crate::transcript::ContentBlock;
use crate::transcript::TranscriptRecorder;
use crate::undo::GhostSnapshotStack;

// =============================================================================
// Error Categorization
// =============================================================================

pub use nori_acp_host::AcpErrorCategory;
pub use nori_acp_host::AcpErrorDetails;
pub use nori_acp_host::categorize_acp_error;
pub use nori_acp_host::categorize_acp_error_chain;

/// Generate an enhanced error message with actionable instructions.
///
/// Based on the error category, this function produces a user-friendly message
/// that explains the problem and provides steps to resolve it.
pub fn enhanced_error_message(
    category: AcpErrorCategory,
    original_error: &str,
    provider_name: &str,
    auth_hint: &str,
    display_name: &str,
    install_hint: &str,
) -> String {
    match category {
        AcpErrorCategory::Authentication => {
            format!("Authentication required for {provider_name}. {auth_hint}")
        }
        AcpErrorCategory::QuotaExceeded => {
            format!("Rate limit or quota exceeded for {provider_name}: {original_error}")
        }
        AcpErrorCategory::ExecutableNotFound => {
            format!("Could not find the {display_name} CLI. Please install it with: {install_hint}")
        }
        AcpErrorCategory::Initialization => {
            format!(
                "Failed to initialize {provider_name}. The agent may be incompatible or experiencing issues. Original error: {original_error}"
            )
        }
        AcpErrorCategory::PromptTooLong => {
            "Prompt is too long. Try using /compact to reduce context size, or start a new session."
                .to_string()
        }
        AcpErrorCategory::ApiServerError => {
            "The API returned a server error. This is usually temporary — please try again."
                .to_string()
        }
        AcpErrorCategory::SessionNotFound => format!(
            "That session no longer exists on {provider_name} — it may have expired or been \
             closed elsewhere. Pick another session from /resume or start a new one."
        ),
        AcpErrorCategory::SessionNotResumable => format!(
            "That session can't be reattached on {provider_name}. Start a new session instead."
        ),
        AcpErrorCategory::SessionAlreadyActive => format!(
            "A session is already active on the {provider_name} connection. Close it with /close \
             before resuming or creating another."
        ),
        AcpErrorCategory::NoActiveSession => format!(
            "No session is active on the {provider_name} connection. Start a new chat with /new."
        ),
        AcpErrorCategory::AgentUnreachable => format!(
            "Could not reach the {provider_name} backing service. Check your network and try \
             again. Original error: {original_error}"
        ),
        AcpErrorCategory::Unknown => original_error.to_string(),
    }
}

/// Wrap a local spawn/session error with category-based hints derived from the
/// agent's config. Shared by every local connection and session-creation
/// failure path in `spawn()` and `resume_session()`.
pub fn enhance_agent_error(
    error: anyhow::Error,
    config: &crate::registry::AcpAgentConfig,
) -> anyhow::Error {
    let details = categorize_acp_error_chain(&error);
    // An agent-supplied `error.data.detail` (e.g. the exact login command for
    // this deployment) is more precise than the registry's static auth hint.
    let auth_hint = details.detail.as_deref().unwrap_or(&config.auth_hint);
    let mut message = enhanced_error_message(
        details.category.clone(),
        &format!("{error}"),
        &config.provider_info.name,
        auth_hint,
        &config.display_name,
        &config.install_hint,
    );
    // For non-auth categories the detail is agent-supplied context (e.g.
    // "the session is no longer claimed") — keep it verbatim.
    if details.category != AcpErrorCategory::Authentication
        && let Some(detail) = details.detail
    {
        message = format!("{message} ({detail})");
    }
    anyhow::anyhow!(message)
}

/// Configuration for spawning an ACP backend.
///
/// This contains the subset of Codex configuration needed for ACP mode,
/// avoiding a direct dependency on codex_core.
#[derive(Debug, Clone)]
pub struct AcpBackendConfig {
    /// Agent name used to look up agent in registry
    pub agent: String,
    /// Working directory for the session
    pub cwd: PathBuf,
    /// Approval policy for command execution
    pub approval_policy: AskForApproval,
    /// Sandbox policy for command execution
    pub sandbox_policy: SandboxPolicy,
    /// Optional external notifier command for OS-level notifications
    pub notify: Option<Vec<String>>,
    /// Whether OS-level desktop notifications are enabled
    pub os_notifications: crate::config::OsNotifications,
    /// How long after idle before sending a notification
    pub notify_after_idle: crate::config::NotifyAfterIdle,
    /// Nori home directory for history storage
    pub nori_home: PathBuf,
    /// History persistence policy
    pub history_persistence: crate::config::HistoryPersistence,
    /// ACP wire proxy logging settings
    pub acp_proxy: crate::config::AcpProxyConfig,
    /// CLI version for transcript metadata
    pub cli_version: String,
    /// Auto-worktree mode (whether a worktree was created at startup)
    pub auto_worktree: crate::config::AutoWorktree,
    /// The git repo root (before worktree creation), used for renaming the worktree
    pub auto_worktree_repo_root: Option<PathBuf>,
    /// Whether the first prompt should be summarized for the footer.
    pub prompt_summary_enabled: bool,
    /// Scripts to run when a session starts
    pub session_start_hooks: Vec<PathBuf>,
    /// Scripts to run when a session ends
    pub session_end_hooks: Vec<PathBuf>,
    /// Scripts to run before a user prompt is sent to the agent
    pub pre_user_prompt_hooks: Vec<PathBuf>,
    /// Scripts to run after a user prompt is sent to the agent
    pub post_user_prompt_hooks: Vec<PathBuf>,
    /// Scripts to run before a tool call is executed
    pub pre_tool_call_hooks: Vec<PathBuf>,
    /// Scripts to run after a tool call completes
    pub post_tool_call_hooks: Vec<PathBuf>,
    /// Scripts to run before the agent produces a response
    pub pre_agent_response_hooks: Vec<PathBuf>,
    /// Scripts to run after the agent finishes its response
    pub post_agent_response_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run when a session starts
    pub async_session_start_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run when a session ends
    pub async_session_end_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run before a user prompt is sent
    pub async_pre_user_prompt_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run after a user prompt is sent
    pub async_post_user_prompt_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run before a tool call is executed
    pub async_pre_tool_call_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run after a tool call completes
    pub async_post_tool_call_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run before the agent produces a response
    pub async_pre_agent_response_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run after the agent finishes its response
    pub async_post_agent_response_hooks: Vec<PathBuf>,
    /// Timeout for hook script execution
    pub script_timeout: std::time::Duration,
    /// Default model to apply on session start (from config.toml [default_models])
    pub default_model: Option<String>,
    /// Optional initial context to inject into the first prompt.
    /// Used by fork to provide conversation history as context to the new session.
    pub initial_context: Option<String>,
    /// Optional session context injected into the first prompt without
    /// `SUMMARY_PREFIX` framing. Used to provide product-level context
    /// (e.g. "you are running inside the nori CLI").
    pub session_context: Option<String>,
    /// MCP server configuration for listing via /mcp command
    pub mcp_servers: HashMap<String, McpServerConfig>,
    /// OAuth credentials store mode for MCP auth status computation
    pub mcp_oauth_credentials_store_mode: OAuthCredentialsStoreMode,
}

#[derive(Debug, Clone)]
pub enum BackendEvent {
    Public(SessionEvent),
}

pub(crate) struct PendingApprovalRequest {
    request: DelegatedRequest,
}

#[derive(Clone)]
pub struct AcpBackend {
    connection: Arc<AcpConnection>,
    /// Session ID is wrapped in RwLock to allow replacing it during /compact
    session_id: Arc<RwLock<acp::SessionId>>,
    backend_event_tx: mpsc::Sender<BackendEvent>,
    /// Working directory for the session
    cwd: PathBuf,
    /// Pending approval requests waiting for user decision
    pending_approvals: Arc<Mutex<Vec<PendingApprovalRequest>>>,
    /// Typed prompt callers waiting for the ACP transport request ID.
    pending_prompt_submissions:
        Arc<Mutex<HashMap<String, oneshot::Sender<Result<acp::RequestId>>>>>,
    /// Notifier for OS-level notifications (approval waiting, idle)
    user_notifier: Arc<crate::UserNotifier>,
    /// Abort handle for the idle detection timer (if running)
    idle_timer_abort: Arc<Mutex<Option<tokio::task::AbortHandle>>>,
    /// Nori home directory for history storage
    nori_home: PathBuf,
    /// History persistence policy
    history_persistence: crate::config::HistoryPersistence,
    /// ACP wire proxy logging settings
    acp_proxy: crate::config::AcpProxyConfig,
    /// Conversation ID for this session (used for history entries).
    ///
    /// Interior-mutable so branch-at-head `/fork` can swap the active
    /// conversation to the forked transcript.
    conversation_id: Arc<RwLock<ConversationId>>,
    /// Sender for broadcasting approval policy updates to the handler
    approval_policy_tx: watch::Sender<AskForApproval>,
    /// Stored summary from last /compact operation, to be prepended to next prompt
    pending_compact_summary: Arc<Mutex<Option<String>>>,
    /// Persistent goal for this ACP session.
    thread_goal_state: Arc<Mutex<thread_goal::ThreadGoalState>>,
    /// True after the active ACP agent has successfully opened the backend-owned
    /// `nori-client` MCP endpoint.
    goal_mcp_connected: Arc<AtomicBool>,
    /// Loopback HTTP server exposing the backend-owned `nori-client` MCP tools.
    goal_mcp_http_server: Arc<Mutex<Option<nori_client_mcp::NoriClientServer>>>,
    /// Transcript recorder cell used by local MCP tools created before the
    /// recorder's session ID is known.
    /// Accumulated context from hook `::context::` lines, prepended to next prompt
    pending_hook_context: Arc<Mutex<Option<String>>>,
    /// Transcript recorder for session persistence.
    ///
    /// Interior-mutable so branch-at-head `/fork` can swap the active recorder
    /// to a freshly forked transcript. The event-forwarding task reads this
    /// cell per event so post-fork entries never land in the frozen parent.
    transcript_recorder: Arc<RwLock<Option<Arc<TranscriptRecorder>>>>,
    /// Internal queue for prompt result events that need reducer processing.
    session_event_tx: mpsc::Sender<session_runtime_driver::SessionRuntimeInput>,
    /// Prompt result channel bridged with ACP notifications to preserve ordering.
    prompt_result_tx: mpsc::Sender<session_reducer::InboundEvent>,
    /// Blocks prompt-time transport and failure events until the public Prompting
    /// phase carrying the exact wire request ID has been emitted.
    prompt_phase_gate: Arc<Mutex<Option<tokio::sync::watch::Receiver<bool>>>>,
    /// How long after idle before sending a notification
    notify_after_idle: crate::config::NotifyAfterIdle,
    /// Stack of ghost commit snapshots for /undo support
    ghost_snapshots: Arc<GhostSnapshotStack>,
    /// Whether the first user prompt has been sent (for prompt summary)
    is_first_prompt: Arc<Mutex<bool>>,
    /// Whether the first prompt should be summarized for the footer.
    prompt_summary_enabled: bool,
    /// Agent name stored for spawning summarization connection
    agent_name: String,
    /// CLI version recorded in transcript metadata (including forked transcripts)
    cli_version: String,
    /// Auto-worktree mode (whether a worktree was created at startup)
    auto_worktree: crate::config::AutoWorktree,
    /// The git repo root (before worktree creation), used for renaming
    auto_worktree_repo_root: Option<PathBuf>,
    /// Scripts to run when a session ends
    session_end_hooks: Vec<PathBuf>,
    /// Scripts to run before a user prompt is sent to the agent
    pre_user_prompt_hooks: Vec<PathBuf>,
    /// Scripts to run after a user prompt is sent to the agent
    post_user_prompt_hooks: Vec<PathBuf>,
    /// Scripts to run after the agent finishes its response
    post_agent_response_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run when a session ends
    async_session_end_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run before a user prompt is sent
    async_pre_user_prompt_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run after a user prompt is sent
    async_post_user_prompt_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run after the agent finishes its response
    async_post_agent_response_hooks: Vec<PathBuf>,
    /// Timeout for hook script execution
    script_timeout: std::time::Duration,
    /// Serialized reducer-owned ACP session runtime.
    session_driver: Arc<Mutex<session_runtime_driver::SessionDriver>>,
    /// MCP server configuration forwarded to ACP agents at session creation.
    mcp_servers: HashMap<String, McpServerConfig>,
    /// OAuth credential store mode used when forwarding MCP auth to ACP agents.
    mcp_oauth_credentials_store_mode: OAuthCredentialsStoreMode,
    /// Set to true when Op::Shutdown is initiated, to avoid spurious disconnect errors
    is_shutting_down: Arc<AtomicBool>,
    /// Abort handle for the in-flight prompt task (if any)
    prompt_task_abort: Arc<Mutex<Option<tokio::task::AbortHandle>>>,
    /// Abort handle for the cancel timeout watchdog (if any)
    cancel_timeout_abort: Arc<Mutex<Option<tokio::task::AbortHandle>>>,
    /// Abort handle for the serialized reducer task.
    runtime_task_abort: Arc<Mutex<Option<tokio::task::AbortHandle>>>,
    /// Abort handle for the connection event relay task.
    relay_task_abort: Arc<Mutex<Option<tokio::task::AbortHandle>>>,
}

impl AcpBackend {
    /// The shared cell holding the active transcript recorder.
    ///
    /// The event-forwarding task holds this cell and re-reads the recorder per
    /// event so a branch-at-head fork swap immediately redirects recording to
    /// the new transcript instead of the frozen parent.
    pub(crate) fn transcript_recorder_cell(&self) -> Arc<RwLock<Option<Arc<TranscriptRecorder>>>> {
        Arc::clone(&self.transcript_recorder)
    }
}

fn fallback_session_context_for_connection(
    config: &AcpBackendConfig,
    connection: &AcpConnection,
) -> Option<String> {
    if connection.capabilities().mcp_capabilities.http {
        None
    } else {
        config.session_context.clone()
    }
}

#[cfg(unix)]
pub mod browser_session;
mod nori_client_context;
mod nori_client_mcp;
pub mod probe;
mod session;
mod session_defaults;
pub(crate) mod session_reducer;
mod session_runtime_driver;
mod session_swap;
mod spawn_and_relay;
mod submit_and_ops;
mod thread_goal;
mod transcript;
mod user_input;
mod user_shell;
pub use transcript::client_events_to_replay_client_events;
pub use transcript::transcript_to_replay_client_events;
pub use transcript::transcript_to_replay_session_events;
pub use transcript::transcript_to_summary;
mod hooks;

fn public_nori_capabilities(
    view: crate::normalized::SessionCapabilitiesView,
) -> nori_protocol::NoriCapabilities {
    nori_protocol::NoriCapabilities {
        goal_management: view.nori_client.advertised,
        builtin_commands: view.builtin_commands,
    }
}

use hooks::commands_dir;
use hooks::generate_id;
use hooks::route_hook_results;
use hooks::run_prompt_summary;
use hooks::run_session_start_hooks;
