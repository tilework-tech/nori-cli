//! Backend adapter for ACP agents in the TUI
//!
//! This module provides `AcpBackend`, which adapts the ACP connection interface
//! to be compatible with the TUI's event-driven architecture. It translates
//! between Codex `Op` submissions and ACP protocol calls, emits ACP session
//! semantics on `nori_protocol::ClientEvent`, and keeps `codex_protocol::Event`
//! only for shared control-plane notifications.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use codex_core::config::types::McpServerConfig;
use codex_protocol::ConversationId;
#[cfg(test)]
use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::HookOutputEvent;
use codex_protocol::protocol::HookOutputLevel;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::PromptSummaryEvent;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::SessionConfiguredEvent;
use codex_protocol::protocol::WarningEvent;
use codex_protocol::user_input::UserInput;
use codex_rmcp_client::OAuthCredentialsStoreMode;
use nori_protocol::ClientEvent;
use sacp::schema as acp;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tracing::debug;
use tracing::warn;

use crate::connection::AcpModelState;
use crate::connection::ApprovalEventType;
use crate::connection::ApprovalRequest;
use crate::connection::sacp_connection::SacpConnection;
use crate::registry::get_agent_config;
use crate::transcript::ContentBlock;
use crate::transcript::TranscriptRecorder;
use crate::translator;
use crate::undo::GhostSnapshotStack;

// =============================================================================
// Error Categorization
// =============================================================================

/// Categories of ACP spawn errors for providing actionable user messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpErrorCategory {
    /// Authentication required or failed
    Authentication,
    /// Rate limit or quota exceeded
    QuotaExceeded,
    /// Command/executable not found
    ExecutableNotFound,
    /// General initialization failure
    Initialization,
    /// Prompt exceeds the agent's context window
    PromptTooLong,
    /// API returned a server error (5xx)
    ApiServerError,
    /// Unknown error (fallback)
    Unknown,
}

/// Categorize an ACP error based on error string patterns.
///
/// This function analyzes error messages and categorizes them to enable
/// providing actionable instructions to users.
pub fn categorize_acp_error(error: &str) -> AcpErrorCategory {
    let error_lower = error.to_lowercase();

    if error_lower.contains("auth")
        || error_lower.contains("-32000") // JSON-RPC auth error code
        || error_lower.contains("api key")
        || error_lower.contains("unauthorized")
        || error_lower.contains("not logged in")
    {
        AcpErrorCategory::Authentication
    } else if error_lower.contains("quota")
        || error_lower.contains("rate limit")
        || error_lower.contains("too many requests")
        || error_lower.contains("429")
        || error_lower.contains("out of extra usage")
        || error_lower.contains("usage limit")
        || error_lower.contains("exceeded your usage")
    {
        AcpErrorCategory::QuotaExceeded
    } else if error_lower.contains("command not found")
        || (error_lower.contains("no such file") && error_lower.contains("directory"))
        || error_lower.contains("os error 2") // ENOENT on Unix
        || error_lower.contains("cannot find the path")
    // Windows
    {
        AcpErrorCategory::ExecutableNotFound
    } else if error_lower.contains("initialization")
        || error_lower.contains("handshake")
        || error_lower.contains("protocol")
    {
        AcpErrorCategory::Initialization
    } else if error_lower.contains("prompt is too long") {
        AcpErrorCategory::PromptTooLong
    } else if error_lower.contains("500")
        || error_lower.contains("502")
        || error_lower.contains("503")
        || error_lower.contains("504")
        || error_lower.contains("server error")
        || error_lower.contains("api_error")
        || error_lower.contains("overloaded")
    {
        AcpErrorCategory::ApiServerError
    } else {
        AcpErrorCategory::Unknown
    }
}

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
        AcpErrorCategory::Unknown => original_error.to_string(),
    }
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
    /// CLI version for transcript metadata
    pub cli_version: String,
    /// Auto-worktree mode (whether a worktree was created at startup)
    pub auto_worktree: crate::config::AutoWorktree,
    /// The git repo root (before worktree creation), used for renaming the worktree
    pub auto_worktree_repo_root: Option<PathBuf>,
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

/// Backend adapter that provides a TUI-compatible interface for ACP agents.
///
/// This struct wraps a `SacpConnection` and translates between:
/// - Codex `Op` submissions → ACP protocol calls
/// - ACP control-plane output → `codex_protocol::Event`
/// - ACP session-domain output → `nori_protocol::ClientEvent`
#[derive(Debug, Clone)]
pub enum BackendEvent {
    Control(Event),
    Client(ClientEvent),
}

pub(crate) struct PendingApprovalRequest {
    request_id: String,
    request: ApprovalRequest,
}

#[derive(Clone)]
pub struct AcpBackend {
    connection: Arc<SacpConnection>,
    /// Session ID is wrapped in RwLock to allow replacing it during /compact
    session_id: Arc<RwLock<acp::SessionId>>,
    event_tx: mpsc::Sender<Event>,
    backend_event_tx: mpsc::Sender<BackendEvent>,
    /// Working directory for the session
    cwd: PathBuf,
    /// Pending approval requests waiting for user decision
    pending_approvals: Arc<Mutex<Vec<PendingApprovalRequest>>>,
    /// Notifier for OS-level notifications (approval waiting, idle)
    user_notifier: Arc<codex_core::UserNotifier>,
    /// Abort handle for the idle detection timer (if running)
    idle_timer_abort: Arc<Mutex<Option<tokio::task::AbortHandle>>>,
    /// Nori home directory for history storage
    nori_home: PathBuf,
    /// History persistence policy
    history_persistence: crate::config::HistoryPersistence,
    /// Conversation ID for this session (used for history entries)
    conversation_id: ConversationId,
    /// Sender for broadcasting approval policy updates to the handler
    approval_policy_tx: watch::Sender<AskForApproval>,
    /// Stored summary from last /compact operation, to be prepended to next prompt
    pending_compact_summary: Arc<Mutex<Option<String>>>,
    /// Accumulated context from hook `::context::` lines, prepended to next prompt
    pending_hook_context: Arc<Mutex<Option<String>>>,
    /// Transcript recorder for session persistence
    transcript_recorder: Option<Arc<TranscriptRecorder>>,
    /// Internal queue for prompt result events that need reducer processing.
    session_event_tx: mpsc::Sender<session_runtime_driver::SessionRuntimeInput>,
    /// Prompt result channel bridged with ACP notifications to preserve ordering.
    prompt_result_tx: mpsc::Sender<session_reducer::InboundEvent>,
    /// How long after idle before sending a notification
    notify_after_idle: crate::config::NotifyAfterIdle,
    /// Stack of ghost commit snapshots for /undo support
    ghost_snapshots: Arc<GhostSnapshotStack>,
    /// Whether the first user prompt has been sent (for prompt summary)
    is_first_prompt: Arc<Mutex<bool>>,
    /// Agent name stored for spawning summarization connection
    agent_name: String,
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
}

mod helpers;
mod session;
pub(crate) mod session_reducer;
mod session_runtime_driver;
mod spawn_and_relay;
mod submit_and_ops;
mod user_input;
use helpers::get_op_name;
mod tool_display;
#[cfg(test)]
pub(crate) use tool_display::classify_tool_to_parsed_command;
#[cfg(test)]
pub(crate) use tool_display::truncate_for_log;
mod transcript;
pub use transcript::client_events_to_replay_client_events;
pub use transcript::transcript_to_replay_client_events;
pub use transcript::transcript_to_summary;
mod hooks;

pub(crate) async fn emit_client_event(
    backend_event_tx: &mpsc::Sender<BackendEvent>,
    transcript_recorder: Option<&Arc<TranscriptRecorder>>,
    client_event: ClientEvent,
) {
    let _ = backend_event_tx
        .send(BackendEvent::Client(client_event.clone()))
        .await;
    if let Some(recorder) = transcript_recorder
        && let Err(e) = recorder.record_client_event(&client_event).await
    {
        warn!("Failed to record normalized client event: {e}");
    }
}

pub(crate) async fn forward_control_events(
    mut event_rx: mpsc::Receiver<Event>,
    backend_event_tx: mpsc::Sender<BackendEvent>,
) {
    while let Some(event) = event_rx.recv().await {
        let _ = backend_event_tx.send(BackendEvent::Control(event)).await;
    }
}

use hooks::commands_dir;
use hooks::generate_id;
use hooks::route_hook_results;
use hooks::run_prompt_summary;
use hooks::run_session_start_hooks;

#[cfg(test)]
mod tests;
