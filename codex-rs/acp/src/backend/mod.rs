//! Backend adapter for ACP agents in the TUI
//!
//! This module provides `AcpBackend`, which adapts the ACP connection interface
//! to be compatible with the TUI's event-driven architecture. It translates
//! between Codex `Op` submissions and ACP protocol calls, and converts ACP
//! session updates into `codex_protocol::Event` for the TUI.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use agent_client_protocol as acp;
use anyhow::Result;
use codex_protocol::ConversationId;
use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::ContextCompactedEvent;
use codex_protocol::protocol::ErrorEvent;
use codex_protocol::protocol::Event;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::HookOutputEvent;
use codex_protocol::protocol::HookOutputLevel;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::PatchApplyBeginEvent;
use codex_protocol::protocol::PromptSummaryEvent;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::SandboxPolicy;
use codex_protocol::protocol::SessionConfiguredEvent;
use codex_protocol::protocol::TurnAbortReason;
use codex_protocol::protocol::TurnAbortedEvent;
use codex_protocol::protocol::WarningEvent;
use codex_protocol::user_input::UserInput;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tracing::debug;
use tracing::warn;

use crate::connection::AcpConnection;
use crate::connection::AcpModelState;
use crate::connection::ApprovalEventType;
use crate::connection::ApprovalRequest;
use crate::registry::get_agent_config;
use crate::transcript::ContentBlock;
use crate::transcript::TranscriptRecorder;
use crate::translator;
use crate::translator::is_patch_operation;
use crate::translator::tool_call_to_file_change;
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
    npm_package: &str,
) -> String {
    match category {
        AcpErrorCategory::Authentication => {
            format!("Authentication required for {provider_name}. {auth_hint}")
        }
        AcpErrorCategory::QuotaExceeded => {
            format!("Rate limit or quota exceeded for {provider_name}: {original_error}")
        }
        AcpErrorCategory::ExecutableNotFound => {
            format!(
                "Could not find the {display_name} CLI. Please install it with: npm install -g {npm_package}"
            )
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
    /// Whether auto-worktree is enabled (worktree was created at startup)
    pub auto_worktree: bool,
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
}

/// Backend adapter that provides a TUI-compatible interface for ACP agents.
///
/// This struct wraps an `AcpConnection` and translates between:
/// - Codex `Op` submissions → ACP protocol calls
/// - ACP `SessionUpdate` events → `codex_protocol::Event`
pub struct AcpBackend {
    connection: Arc<AcpConnection>,
    /// Session ID is wrapped in RwLock to allow replacing it during /compact
    session_id: Arc<RwLock<acp::SessionId>>,
    event_tx: mpsc::Sender<Event>,
    /// Working directory for the session
    cwd: PathBuf,
    /// Pending approval requests waiting for user decision
    pending_approvals: Arc<Mutex<Vec<ApprovalRequest>>>,
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
    /// How long after idle before sending a notification
    notify_after_idle: crate::config::NotifyAfterIdle,
    /// Stack of ghost commit snapshots for /undo support
    ghost_snapshots: Arc<GhostSnapshotStack>,
    /// Whether the first user prompt has been sent (for prompt summary)
    is_first_prompt: Arc<Mutex<bool>>,
    /// Agent name stored for spawning summarization connection
    agent_name: String,
    /// Whether auto-worktree is enabled (worktree was created at startup)
    auto_worktree: bool,
    /// The git repo root (before worktree creation), used for renaming
    auto_worktree_repo_root: Option<PathBuf>,
    /// Scripts to run when a session ends
    session_end_hooks: Vec<PathBuf>,
    /// Scripts to run before a user prompt is sent to the agent
    pre_user_prompt_hooks: Vec<PathBuf>,
    /// Scripts to run after a user prompt is sent to the agent
    post_user_prompt_hooks: Vec<PathBuf>,
    /// Scripts to run before a tool call is executed
    pre_tool_call_hooks: Vec<PathBuf>,
    /// Scripts to run after a tool call completes
    post_tool_call_hooks: Vec<PathBuf>,
    /// Scripts to run before the agent produces a response
    pre_agent_response_hooks: Vec<PathBuf>,
    /// Scripts to run after the agent finishes its response
    post_agent_response_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run when a session ends
    async_session_end_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run before a user prompt is sent
    async_pre_user_prompt_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run after a user prompt is sent
    async_post_user_prompt_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run before a tool call is executed
    async_pre_tool_call_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run after a tool call completes
    async_post_tool_call_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run before the agent produces a response
    async_pre_agent_response_hooks: Vec<PathBuf>,
    /// Async (fire-and-forget) scripts to run after the agent finishes its response
    async_post_agent_response_hooks: Vec<PathBuf>,
    /// Timeout for hook script execution
    script_timeout: std::time::Duration,
}

impl AcpBackend {
    /// Spawn an ACP backend for the given configuration.
    ///
    /// This will:
    /// 1. Look up the agent config from the registry
    /// 2. Spawn the ACP connection
    /// 3. Create a session
    /// 4. Send a synthetic `SessionConfigured` event
    /// 5. Start background tasks for event translation and approval handling
    ///
    /// # Arguments
    /// * `config` - The ACP backend configuration
    /// * `event_tx` - Channel to send translated events to the TUI
    ///
    /// # Returns
    /// A connected `AcpBackend` ready to receive operations.
    pub async fn spawn(config: &AcpBackendConfig, event_tx: mpsc::Sender<Event>) -> Result<Self> {
        let agent_config = get_agent_config(&config.agent)?;
        let cwd = config.cwd.clone();

        debug!("Spawning ACP backend for agent: {}", config.agent);

        // Spawn the ACP connection with enhanced error handling
        let connection_result = AcpConnection::spawn(&agent_config, &cwd).await;

        let mut connection = match connection_result {
            Ok(conn) => conn,
            Err(e) => {
                // Get the full error chain to check for nested auth errors
                let error_string = format!("{e:?}");
                let category = categorize_acp_error(&error_string);

                // Use the display format for the user-facing message
                let display_error = format!("{e}");
                let enhanced_message = enhanced_error_message(
                    category,
                    &display_error,
                    &agent_config.provider_info.name,
                    &agent_config.auth_hint,
                    agent_config.agent.display_name(),
                    agent_config.agent.npm_package(),
                );

                return Err(anyhow::anyhow!(enhanced_message));
            }
        };

        // Create a session with enhanced error handling
        let session_result = connection.create_session(&cwd).await;
        let session_id = match session_result {
            Ok(id) => id,
            Err(e) => {
                // Get the full error chain to check for nested auth errors
                let error_string = format!("{e:?}");
                let category = categorize_acp_error(&error_string);

                // Use the display format for the user-facing message
                let display_error = format!("{e}");
                let enhanced_message = enhanced_error_message(
                    category,
                    &display_error,
                    &agent_config.provider_info.name,
                    &agent_config.auth_hint,
                    agent_config.agent.display_name(),
                    agent_config.agent.npm_package(),
                );

                return Err(anyhow::anyhow!(enhanced_message));
            }
        };

        debug!("ACP session created: {:?}", session_id);

        // Apply default model from config if one is set for this agent
        #[cfg(feature = "unstable")]
        if let Some(ref default_model) = config.default_model {
            let model_state = connection.model_state();
            let model_available = model_state
                .available_models
                .iter()
                .any(|m| m.model_id.to_string() == *default_model);
            if model_available {
                let model_id = acp::ModelId::from(default_model.clone());
                match connection.set_model(&session_id, &model_id).await {
                    Ok(()) => {
                        debug!("Applied default model from config: {default_model}");
                    }
                    Err(e) => {
                        warn!("Failed to apply default model '{default_model}': {e}");
                    }
                }
            } else {
                debug!("Default model '{default_model}' not in available models, skipping");
            }
        }

        // Take the approval receiver for handling permission requests
        let approval_rx = connection.take_approval_receiver();
        let persistent_rx = connection.take_persistent_receiver();

        let connection = Arc::new(connection);
        let pending_approvals = Arc::new(Mutex::new(Vec::new()));
        let use_native_notifications =
            config.os_notifications == crate::config::OsNotifications::Enabled;
        let user_notifier = Arc::new(codex_core::UserNotifier::new(
            config.notify.clone(),
            use_native_notifications,
        ));

        let idle_timer_abort = Arc::new(Mutex::new(None));

        // Create watch channel for dynamic approval policy updates
        let (approval_policy_tx, approval_policy_rx) = watch::channel(config.approval_policy);

        // Create conversation ID for this session
        let conversation_id = ConversationId::new();

        // Get history metadata
        let (history_log_id, history_entry_count) =
            crate::message_history::history_metadata(&config.nori_home).await;

        // Initialize transcript recorder (non-fatal if it fails)
        let transcript_recorder = match TranscriptRecorder::new(
            &config.nori_home,
            &cwd,
            Some(config.agent.clone()),
            &config.cli_version,
            Some(session_id.to_string()),
        )
        .await
        {
            Ok(recorder) => Some(Arc::new(recorder)),
            Err(e) => {
                warn!("Failed to initialize transcript recorder: {e}");
                None
            }
        };

        let backend = Self {
            connection,
            session_id: Arc::new(RwLock::new(session_id)),
            event_tx: event_tx.clone(),
            cwd: cwd.clone(),
            pending_approvals: Arc::clone(&pending_approvals),
            user_notifier: Arc::clone(&user_notifier),
            idle_timer_abort: Arc::clone(&idle_timer_abort),
            nori_home: config.nori_home.clone(),
            history_persistence: config.history_persistence,
            conversation_id,
            approval_policy_tx,
            pending_compact_summary: Arc::new(Mutex::new(None)),
            pending_hook_context: Arc::new(Mutex::new(None)),
            transcript_recorder,
            notify_after_idle: config.notify_after_idle,
            ghost_snapshots: Arc::new(GhostSnapshotStack::new()),
            is_first_prompt: Arc::new(Mutex::new(true)),
            agent_name: config.agent.clone(),
            auto_worktree: config.auto_worktree,
            auto_worktree_repo_root: config.auto_worktree_repo_root.clone(),
            session_end_hooks: config.session_end_hooks.clone(),
            pre_user_prompt_hooks: config.pre_user_prompt_hooks.clone(),
            post_user_prompt_hooks: config.post_user_prompt_hooks.clone(),
            pre_tool_call_hooks: config.pre_tool_call_hooks.clone(),
            post_tool_call_hooks: config.post_tool_call_hooks.clone(),
            pre_agent_response_hooks: config.pre_agent_response_hooks.clone(),
            post_agent_response_hooks: config.post_agent_response_hooks.clone(),
            async_session_end_hooks: config.async_session_end_hooks.clone(),
            async_pre_user_prompt_hooks: config.async_pre_user_prompt_hooks.clone(),
            async_post_user_prompt_hooks: config.async_post_user_prompt_hooks.clone(),
            async_pre_tool_call_hooks: config.async_pre_tool_call_hooks.clone(),
            async_post_tool_call_hooks: config.async_post_tool_call_hooks.clone(),
            async_pre_agent_response_hooks: config.async_pre_agent_response_hooks.clone(),
            async_post_agent_response_hooks: config.async_post_agent_response_hooks.clone(),
            script_timeout: config.script_timeout,
        };

        // Execute session_start hooks
        run_session_start_hooks(
            &config.session_start_hooks,
            config.script_timeout,
            &event_tx,
            Some(&backend.pending_hook_context),
        )
        .await;

        // Fire-and-forget async session start hooks
        let _ = crate::hooks::execute_hooks_fire_and_forget(
            config.async_session_start_hooks.clone(),
            config.script_timeout,
            HashMap::new(),
        );

        // Send synthetic SessionConfigured event
        let session_configured = SessionConfiguredEvent {
            session_id: conversation_id,
            model: config.agent.clone(),
            model_provider_id: "acp".to_string(),
            approval_policy: config.approval_policy,
            sandbox_policy: config.sandbox_policy.clone(),
            cwd: cwd.clone(),
            reasoning_effort: None,
            history_log_id,
            history_entry_count,
            initial_messages: None,
            rollout_path: cwd.join(".codex-rollout.jsonl"),
        };

        event_tx
            .send(Event {
                id: String::new(),
                msg: EventMsg::SessionConfigured(session_configured),
            })
            .await
            .ok();

        // Spawn approval handler task
        tokio::spawn(Self::run_approval_handler(
            approval_rx,
            event_tx.clone(),
            Arc::clone(&pending_approvals),
            Arc::clone(&user_notifier),
            cwd.clone(),
            approval_policy_rx,
        ));

        // Spawn persistent listener relay for inter-turn notifications
        tokio::spawn(Self::run_persistent_relay(persistent_rx, event_tx.clone()));

        Ok(backend)
    }

    /// Resume a previous ACP session.
    ///
    /// If the agent supports `session/load` (via capabilities) and an
    /// `acp_session_id` is provided, the existing server-side resume path is
    /// used. Otherwise a client-side replay fallback is used: a fresh session
    /// is created via `session/new`, the transcript is converted into
    /// `initial_messages` for TUI display, and a summary is stored in
    /// `pending_compact_summary` so it gets prepended to the first prompt.
    pub async fn resume_session(
        config: &AcpBackendConfig,
        acp_session_id: Option<&str>,
        transcript: Option<&crate::transcript::Transcript>,
        event_tx: mpsc::Sender<Event>,
    ) -> Result<Self> {
        let agent_config = get_agent_config(&config.agent)?;
        let cwd = config.cwd.clone();

        debug!(
            "Resuming ACP session (acp_session_id={:?}) for agent: {}",
            acp_session_id, config.agent
        );

        let mut connection = AcpConnection::spawn(&agent_config, &cwd)
            .await
            .map_err(|e| {
                let error_string = format!("{e:?}");
                let category = categorize_acp_error(&error_string);
                let display_error = format!("{e}");
                anyhow::anyhow!(enhanced_error_message(
                    category,
                    &display_error,
                    &agent_config.provider_info.name,
                    &agent_config.auth_hint,
                    agent_config.agent.display_name(),
                    agent_config.agent.npm_package(),
                ))
            })?;

        let supports_load_session = connection.capabilities().load_session;

        // Either load the session server-side or create a fresh session for
        // client-side replay.
        //
        // If server-side load_session fails at runtime, we fall back to
        // client-side replay rather than propagating the error. This ensures
        // /resume works even when the agent's load_session is broken.
        // The sixth tuple element carries buffered replay events from
        // server-side session/load.  We must NOT spawn a relay task for
        // these events until *after* resume_session has finished sending
        // its own events (SessionConfigured, Warning, etc.) to event_tx,
        // because the relay can fill the bounded channel and block
        // resume_session from sending.
        let (
            session_id,
            initial_messages,
            pending_summary,
            is_first_prompt_val,
            used_fallback,
            deferred_replay_events,
        ) = if let Some(sid) = acp_session_id.filter(|_| supports_load_session) {
            debug!("Agent supports session/load — using server-side resume");

            let (update_tx, mut update_rx) = mpsc::channel::<acp::SessionUpdate>(256);

            // Collect replay events into a buffer instead of sending them
            // directly to event_tx. The event_tx consumer only starts after
            // resume_session returns, so sending directly would deadlock
            // when the number of events exceeds the channel capacity.
            let collect_handle = tokio::spawn(async move {
                let mut pending_patch_changes = std::collections::HashMap::new();
                let mut buffered_events = Vec::new();
                while let Some(update) = update_rx.recv().await {
                    let event_msgs =
                        translate_session_update_to_events(&update, &mut pending_patch_changes);
                    for msg in event_msgs {
                        buffered_events.push(Event {
                            id: String::new(),
                            msg,
                        });
                    }
                }
                buffered_events
            });

            match connection.load_session(sid, &cwd, update_tx).await {
                Ok(session_id) => {
                    // Wait for all updates to be collected. This is safe
                    // because the collect task buffers into a Vec (no
                    // backpressure) and update_rx closes when load_session
                    // completes (the worker thread drops update_tx).
                    let buffered_events = collect_handle.await.unwrap_or_default();
                    if !buffered_events.is_empty() {
                        debug!(
                            "ACP session/load produced {} replay events (deferred until after setup)",
                            buffered_events.len()
                        );
                    }
                    debug!("ACP session resumed via session/load: {sid}");
                    (session_id, None, None, false, None, buffered_events)
                }
                Err(e) => {
                    warn!(
                        "Server-side session/load failed, falling back to client-side replay: {e}"
                    );
                    collect_handle.abort();

                    let session_id = connection.create_session(&cwd).await.map_err(|e| {
                        let error_string = format!("{e:?}");
                        let category = categorize_acp_error(&error_string);
                        let display_error = format!("{e}");
                        anyhow::anyhow!(enhanced_error_message(
                            category,
                            &display_error,
                            &agent_config.provider_info.name,
                            &agent_config.auth_hint,
                            agent_config.agent.display_name(),
                            agent_config.agent.npm_package(),
                        ))
                    })?;

                    let (replay_events, summary) = if let Some(t) = transcript {
                        let events = transcript_to_replay_events(t);
                        let summary_text = transcript_to_summary(t);
                        let summary_opt = if summary_text.is_empty() {
                            None
                        } else {
                            Some(summary_text)
                        };
                        (Some(events), summary_opt)
                    } else {
                        (None, None)
                    };

                    (
                        session_id,
                        replay_events,
                        summary,
                        true,
                        Some(e.to_string()),
                        Vec::new(),
                    )
                }
            }
        } else {
            debug!("Agent does not support session/load — using client-side replay");

            let session_id = connection.create_session(&cwd).await.map_err(|e| {
                let error_string = format!("{e:?}");
                let category = categorize_acp_error(&error_string);
                let display_error = format!("{e}");
                anyhow::anyhow!(enhanced_error_message(
                    category,
                    &display_error,
                    &agent_config.provider_info.name,
                    &agent_config.auth_hint,
                    agent_config.agent.display_name(),
                    agent_config.agent.npm_package(),
                ))
            })?;

            let (replay_events, summary) = if let Some(t) = transcript {
                let events = transcript_to_replay_events(t);
                let summary_text = transcript_to_summary(t);
                let summary_opt = if summary_text.is_empty() {
                    None
                } else {
                    Some(summary_text)
                };
                (Some(events), summary_opt)
            } else {
                (None, None)
            };

            (session_id, replay_events, summary, true, None, Vec::new())
        };

        let approval_rx = connection.take_approval_receiver();
        let persistent_rx = connection.take_persistent_receiver();
        let connection = Arc::new(connection);
        let pending_approvals = Arc::new(Mutex::new(Vec::new()));
        let use_native_notifications =
            config.os_notifications == crate::config::OsNotifications::Enabled;
        let user_notifier = Arc::new(codex_core::UserNotifier::new(
            config.notify.clone(),
            use_native_notifications,
        ));
        let idle_timer_abort = Arc::new(Mutex::new(None));
        let (approval_policy_tx, approval_policy_rx) = watch::channel(config.approval_policy);
        let conversation_id = ConversationId::new();
        let (history_log_id, history_entry_count) =
            crate::message_history::history_metadata(&config.nori_home).await;

        let transcript_recorder = match TranscriptRecorder::new(
            &config.nori_home,
            &cwd,
            Some(config.agent.clone()),
            &config.cli_version,
            Some(session_id.to_string()),
        )
        .await
        {
            Ok(recorder) => Some(Arc::new(recorder)),
            Err(e) => {
                warn!("Failed to initialize transcript recorder: {e}");
                None
            }
        };

        let backend = Self {
            connection,
            session_id: Arc::new(RwLock::new(session_id)),
            event_tx: event_tx.clone(),
            cwd: cwd.clone(),
            pending_approvals: Arc::clone(&pending_approvals),
            user_notifier: Arc::clone(&user_notifier),
            idle_timer_abort: Arc::clone(&idle_timer_abort),
            nori_home: config.nori_home.clone(),
            history_persistence: config.history_persistence,
            conversation_id,
            approval_policy_tx,
            pending_compact_summary: Arc::new(Mutex::new(pending_summary)),
            pending_hook_context: Arc::new(Mutex::new(None)),
            transcript_recorder,
            notify_after_idle: config.notify_after_idle,
            ghost_snapshots: Arc::new(GhostSnapshotStack::new()),
            is_first_prompt: Arc::new(Mutex::new(is_first_prompt_val)),
            agent_name: config.agent.clone(),
            auto_worktree: config.auto_worktree,
            auto_worktree_repo_root: config.auto_worktree_repo_root.clone(),
            session_end_hooks: config.session_end_hooks.clone(),
            pre_user_prompt_hooks: config.pre_user_prompt_hooks.clone(),
            post_user_prompt_hooks: config.post_user_prompt_hooks.clone(),
            pre_tool_call_hooks: config.pre_tool_call_hooks.clone(),
            post_tool_call_hooks: config.post_tool_call_hooks.clone(),
            pre_agent_response_hooks: config.pre_agent_response_hooks.clone(),
            post_agent_response_hooks: config.post_agent_response_hooks.clone(),
            async_session_end_hooks: config.async_session_end_hooks.clone(),
            async_pre_user_prompt_hooks: config.async_pre_user_prompt_hooks.clone(),
            async_post_user_prompt_hooks: config.async_post_user_prompt_hooks.clone(),
            async_pre_tool_call_hooks: config.async_pre_tool_call_hooks.clone(),
            async_post_tool_call_hooks: config.async_post_tool_call_hooks.clone(),
            async_pre_agent_response_hooks: config.async_pre_agent_response_hooks.clone(),
            async_post_agent_response_hooks: config.async_post_agent_response_hooks.clone(),
            script_timeout: config.script_timeout,
        };

        // Execute session_start hooks
        run_session_start_hooks(
            &config.session_start_hooks,
            config.script_timeout,
            &event_tx,
            Some(&backend.pending_hook_context),
        )
        .await;

        // Fire-and-forget async session start hooks
        let _ = crate::hooks::execute_hooks_fire_and_forget(
            config.async_session_start_hooks.clone(),
            config.script_timeout,
            HashMap::new(),
        );

        let session_configured = SessionConfiguredEvent {
            session_id: conversation_id,
            model: config.agent.clone(),
            model_provider_id: "acp".to_string(),
            approval_policy: config.approval_policy,
            sandbox_policy: config.sandbox_policy.clone(),
            cwd: cwd.clone(),
            reasoning_effort: None,
            history_log_id,
            history_entry_count,
            initial_messages,
            rollout_path: cwd.join(".codex-rollout.jsonl"),
        };

        event_tx
            .send(Event {
                id: String::new(),
                msg: EventMsg::SessionConfigured(session_configured),
            })
            .await
            .ok();

        if let Some(ref fallback_error) = used_fallback {
            event_tx
                .send(Event {
                    id: String::new(),
                    msg: EventMsg::Warning(WarningEvent {
                        message: format!(
                            "Server-side session restore failed ({fallback_error}). \
                             Falling back to transcript replay. The restored session \
                             will not have tool call information in the context."
                        ),
                    }),
                })
                .await
                .ok();
        }

        tokio::spawn(Self::run_approval_handler(
            approval_rx,
            event_tx.clone(),
            Arc::clone(&pending_approvals),
            Arc::clone(&user_notifier),
            cwd.clone(),
            approval_policy_rx,
        ));

        // Spawn persistent listener relay for inter-turn notifications
        tokio::spawn(Self::run_persistent_relay(persistent_rx, event_tx.clone()));

        // Spawn the replay relay *after* all setup events (SessionConfigured,
        // Warning, etc.) have been sent.  Spawning it earlier causes a
        // deadlock: the relay fills the bounded event_tx channel, blocking
        // resume_session from sending its own events while nobody is
        // consuming from event_rx yet.
        if !deferred_replay_events.is_empty() {
            tokio::spawn(async move {
                for event in deferred_replay_events {
                    let _ = event_tx.send(event).await;
                }
            });
        }

        Ok(backend)
    }

    /// Submit an operation to the ACP backend.
    ///
    /// Translates Codex `Op` variants to appropriate ACP actions:
    /// - `Op::UserInput` → ACP prompt
    /// - `Op::Interrupt` → ACP cancel
    /// - `Op::ExecApproval` → Resolve pending approval
    /// - Other ops → Send error event (not supported)
    pub async fn submit(&self, op: Op) -> Result<String> {
        let id = generate_id();

        // Cancel any running idle timer on new user activity
        if let Some(abort_handle) = self.idle_timer_abort.lock().await.take() {
            abort_handle.abort();
        }

        match op {
            Op::UserInput { items } => {
                self.handle_user_input(items, &id).await?;
            }
            Op::Interrupt => {
                self.connection
                    .cancel(&*self.session_id.read().await)
                    .await?;
                // Send TurnAborted event to notify the TUI that the turn was interrupted
                let _ = self
                    .event_tx
                    .send(Event {
                        id: id.clone(),
                        msg: EventMsg::TurnAborted(TurnAbortedEvent {
                            reason: TurnAbortReason::Interrupted,
                        }),
                    })
                    .await;
            }
            Op::ExecApproval {
                id: call_id,
                decision,
            } => {
                self.handle_exec_approval(&call_id, decision).await;
            }
            Op::PatchApproval {
                id: call_id,
                decision,
            } => {
                self.handle_exec_approval(&call_id, decision).await;
            }
            Op::Shutdown => {
                // Cancel any in-progress session and send ShutdownComplete
                // to allow the TUI to exit properly
                debug!("Processing Op::Shutdown in ACP mode");
                let _ = self.connection.cancel(&*self.session_id.read().await).await;

                // Execute session_end hooks and route output before teardown
                if !self.session_end_hooks.is_empty() {
                    let results =
                        crate::hooks::execute_hooks(&self.session_end_hooks, self.script_timeout)
                            .await;
                    // Context lines are irrelevant during shutdown, so pass None.
                    route_hook_results(&results, &self.event_tx, &id, None).await;
                }

                // Async session end hooks: await completion before shutdown
                // so the runtime doesn't kill them when the process exits.
                if let Some(handle) = crate::hooks::execute_hooks_fire_and_forget(
                    self.async_session_end_hooks.clone(),
                    self.script_timeout,
                    HashMap::new(),
                ) && let Err(e) = handle.await
                {
                    warn!("Async session_end hook task panicked: {e}");
                }

                // Shutdown transcript recorder
                if let Some(ref recorder) = self.transcript_recorder
                    && let Err(e) = recorder.shutdown().await
                {
                    warn!("Failed to shutdown transcript recorder: {e}");
                }

                let _ = self
                    .event_tx
                    .send(Event {
                        id: id.clone(),
                        msg: EventMsg::ShutdownComplete,
                    })
                    .await;
            }
            Op::AddToHistory { text } => {
                // Append to history file in the background
                let nori_home = self.nori_home.clone();
                let conversation_id = self.conversation_id;
                let persistence = self.history_persistence;
                tokio::spawn(async move {
                    if let Err(e) = crate::message_history::append_entry(
                        &text,
                        &conversation_id,
                        &nori_home,
                        persistence,
                    )
                    .await
                    {
                        warn!("failed to append to message history: {e}");
                    }
                });
            }
            Op::GetHistoryEntryRequest { offset, log_id } => {
                // Look up history entry in the background
                let nori_home = self.nori_home.clone();
                let event_tx = self.event_tx.clone();
                let id_clone = id.clone();
                tokio::spawn(async move {
                    // Run lookup in blocking thread because it does file IO + locking.
                    let entry_opt = tokio::task::spawn_blocking(move || {
                        crate::message_history::lookup(log_id, offset, &nori_home)
                    })
                    .await
                    .unwrap_or(None);

                    let event = Event {
                        id: id_clone,
                        msg: EventMsg::GetHistoryEntryResponse(
                            codex_protocol::protocol::GetHistoryEntryResponseEvent {
                                offset,
                                log_id,
                                entry: entry_opt.map(|e| {
                                    codex_protocol::message_history::HistoryEntry {
                                        conversation_id: e.session_id,
                                        ts: e.ts,
                                        text: e.text,
                                    }
                                }),
                            },
                        ),
                    };

                    let _ = event_tx.send(event).await;
                });
            }
            Op::SearchHistoryRequest { max_results } => {
                let nori_home = self.nori_home.clone();
                let event_tx = self.event_tx.clone();
                let id_clone = id.clone();
                tokio::spawn(async move {
                    let entries = tokio::task::spawn_blocking(move || {
                        crate::message_history::search_entries(&nori_home, max_results)
                    })
                    .await
                    .unwrap_or_default();

                    let event = Event {
                        id: id_clone,
                        msg: EventMsg::SearchHistoryResponse(
                            codex_protocol::protocol::SearchHistoryResponseEvent {
                                entries: entries
                                    .into_iter()
                                    .map(|e| codex_protocol::message_history::HistoryEntry {
                                        conversation_id: e.session_id,
                                        ts: e.ts,
                                        text: e.text,
                                    })
                                    .collect(),
                            },
                        ),
                    };

                    let _ = event_tx.send(event).await;
                });
            }
            Op::Compact => {
                self.handle_compact(&id).await?;
            }
            Op::ListCustomPrompts => {
                let dir = commands_dir(&self.nori_home);
                let event_tx = self.event_tx.clone();
                let id_clone = id.clone();
                tokio::spawn(async move {
                    let custom_prompts =
                        codex_core::custom_prompts::discover_prompts_in(&dir).await;
                    let _ = event_tx
                        .send(Event {
                            id: id_clone,
                            msg: EventMsg::ListCustomPromptsResponse(
                                codex_protocol::protocol::ListCustomPromptsResponseEvent {
                                    custom_prompts,
                                },
                            ),
                        })
                        .await;
                });
            }
            Op::Undo => {
                // Best-effort cancel any in-progress agent turn before restoring.
                self.connection
                    .cancel(&*self.session_id.read().await)
                    .await
                    .ok();
                crate::undo::handle_undo(&self.event_tx, &id, &self.cwd, &self.ghost_snapshots)
                    .await;
            }
            Op::UndoList => {
                crate::undo::handle_list_snapshots(&self.event_tx, &id, &self.ghost_snapshots)
                    .await;
            }
            Op::UndoTo { index } => {
                self.connection
                    .cancel(&*self.session_id.read().await)
                    .await
                    .ok();
                crate::undo::handle_undo_to(
                    &self.event_tx,
                    &id,
                    &self.cwd,
                    &self.ghost_snapshots,
                    index,
                )
                .await;
            }
            // Unsupported operations - only show error in debug builds
            Op::ListMcpTools | Op::RunUserShellCommand { .. } => {
                let op_name = get_op_name(&op);
                warn!("Unsupported Op in ACP mode: {op_name}");
                #[cfg(debug_assertions)]
                self.send_error(&format!(
                    "Operation '{op_name}' is not supported in ACP mode"
                ))
                .await;
            }
            Op::OverrideTurnContext {
                approval_policy, ..
            } => {
                // Update approval policy if provided
                if let Some(policy) = approval_policy {
                    debug!("Updating approval policy to {policy:?} in ACP mode");
                    // Send the new policy to the approval handler via watch channel
                    let _ = self.approval_policy_tx.send(policy);
                }
            }
            // These ops are internal/context-related, silently ignore
            Op::UserTurn { .. } | Op::ResolveElicitation { .. } => {
                debug!("Ignoring internal Op in ACP mode: {}", get_op_name(&op));
            }
            // Catch any new Op variants we haven't handled - only show error in debug builds
            _ => {
                let op_name = get_op_name(&op);
                warn!("Unknown Op in ACP mode: {op_name}");
                #[cfg(debug_assertions)]
                self.send_error(&format!(
                    "Operation '{op_name}' is not supported in ACP mode"
                ))
                .await;
            }
        }

        Ok(id)
    }

    /// Handle user input by sending a prompt to the ACP agent.
    async fn handle_user_input(&self, items: Vec<UserInput>, id: &str) -> Result<()> {
        // Separate text items (needed for hooks, summary, transcript) from
        // image items (converted to ACP ContentBlock::Image).
        let mut prompt_text = String::new();
        let mut image_items = Vec::new();
        for item in items {
            match item {
                UserInput::Text { text } => {
                    if !prompt_text.is_empty() {
                        prompt_text.push('\n');
                    }
                    prompt_text.push_str(&text);
                }
                UserInput::Image { .. } | UserInput::LocalImage { .. } => {
                    image_items.push(item);
                }
                _ => {
                    warn!("Unknown UserInput variant in ACP mode");
                }
            }
        }

        // Convert image items to ACP content blocks
        let image_blocks = translator::user_inputs_to_content_blocks(image_items)?;

        if prompt_text.is_empty() && image_blocks.is_empty() {
            return Ok(());
        }

        // For image-only prompts, use a placeholder for downstream consumers
        // (hooks, transcript, summary, snapshot labels) that expect non-empty text.
        let display_text = if prompt_text.is_empty() && !image_blocks.is_empty() {
            "[image]".to_string()
        } else {
            prompt_text.clone()
        };

        // Execute pre_user_prompt hooks before sending the prompt
        if !self.pre_user_prompt_hooks.is_empty() {
            let env_vars = HashMap::from([
                ("NORI_HOOK_EVENT".to_string(), "pre_user_prompt".to_string()),
                ("NORI_HOOK_PROMPT_TEXT".to_string(), display_text.clone()),
            ]);
            let results = crate::hooks::execute_hooks_with_env(
                &self.pre_user_prompt_hooks,
                self.script_timeout,
                &env_vars,
            )
            .await;
            route_hook_results(
                &results,
                &self.event_tx,
                id,
                Some(&self.pending_hook_context),
            )
            .await;
        }

        // Fire-and-forget async pre_user_prompt hooks
        if !self.async_pre_user_prompt_hooks.is_empty() {
            let env_vars = HashMap::from([
                ("NORI_HOOK_EVENT".to_string(), "pre_user_prompt".to_string()),
                ("NORI_HOOK_PROMPT_TEXT".to_string(), display_text.clone()),
            ]);
            let _ = crate::hooks::execute_hooks_fire_and_forget(
                self.async_pre_user_prompt_hooks.clone(),
                self.script_timeout,
                env_vars,
            );
        }

        // On first prompt, spawn a fire-and-forget summarization task.
        // Skip for mock models (debug-only test agents) since they don't
        // produce meaningful summaries.
        {
            let mut is_first = self.is_first_prompt.lock().await;
            if *is_first {
                *is_first = false;
                let skip_summary = cfg!(debug_assertions) && self.agent_name.starts_with("mock-");
                if !skip_summary {
                    let event_tx = self.event_tx.clone();
                    let agent_name = self.agent_name.clone();
                    let cwd = self.cwd.clone();
                    let prompt_for_summary = display_text.clone();
                    let auto_worktree = self.auto_worktree;
                    let auto_worktree_repo_root = self.auto_worktree_repo_root.clone();
                    tokio::spawn(async move {
                        if let Err(e) = run_prompt_summary(
                            &event_tx,
                            &agent_name,
                            &cwd,
                            &prompt_for_summary,
                            auto_worktree,
                            auto_worktree_repo_root.as_deref(),
                        )
                        .await
                        {
                            debug!("Prompt summary failed (non-fatal): {e}");
                        }
                    });
                }
            }
        }

        // Create ghost snapshot before sending prompt to agent.
        // This captures the working tree state so /undo can restore it.
        let snapshot_cwd = self.cwd.clone();
        let ghost_snapshots = Arc::clone(&self.ghost_snapshots);
        let label_for_snapshot = display_text.clone();
        match tokio::task::spawn_blocking(move || {
            let options = codex_git::CreateGhostCommitOptions::new(&snapshot_cwd);
            codex_git::create_ghost_commit(&options)
        })
        .await
        {
            Ok(Ok(snapshot)) => {
                ghost_snapshots.push(snapshot, label_for_snapshot).await;
            }
            Ok(Err(codex_git::GitToolingError::NotAGitRepository { .. })) => {
                debug!("Skipping ghost snapshot: not a git repository");
            }
            Ok(Err(err)) => {
                warn!("Failed to create ghost snapshot: {err}");
            }
            Err(err) => {
                warn!("Ghost snapshot task panicked: {err}");
            }
        }

        // Record user message to transcript
        if let Some(ref recorder) = self.transcript_recorder
            && let Err(e) = recorder
                .record_user_message(id, &display_text, vec![])
                .await
        {
            warn!("Failed to record user message to transcript: {e}");
        }

        // Save prompt text for post_user_prompt hooks (before it gets moved)
        let prompt_text_for_hooks = display_text;

        // Prepend any accumulated hook context (from ::context:: lines)
        // This must happen before the compact summary prefix so that the
        // SUMMARY_PREFIX framing instruction always comes first.
        let prompt_with_context = if let Some(ctx) = self.pending_hook_context.lock().await.take() {
            format!("{ctx}\n{prompt_text}")
        } else {
            prompt_text
        };

        // Check if we have a pending compact summary to prepend
        let pending_summary = self.pending_compact_summary.lock().await.take();
        let final_prompt_text = if let Some(summary) = pending_summary {
            use codex_core::compact::SUMMARY_PREFIX;
            format!("{SUMMARY_PREFIX}\n{summary}\n\n{prompt_with_context}")
        } else {
            prompt_with_context
        };

        let mut prompt = Vec::new();
        if !final_prompt_text.is_empty() {
            prompt.push(translator::text_to_content_block(&final_prompt_text));
        }
        prompt.extend(image_blocks);

        // Create channel for receiving session updates
        let (update_tx, mut update_rx) = mpsc::channel(32);

        // Clone what we need for the background task
        let event_tx = self.event_tx.clone();
        let session_id = self.session_id.read().await.clone();
        let connection = Arc::clone(&self.connection);
        let id_clone = id.to_string();
        let user_notifier = Arc::clone(&self.user_notifier);
        let idle_timer_abort = Arc::clone(&self.idle_timer_abort);
        let transcript_recorder = self.transcript_recorder.clone();
        let notify_after_idle = self.notify_after_idle;
        let post_user_prompt_hooks = self.post_user_prompt_hooks.clone();
        let pre_tool_call_hooks = self.pre_tool_call_hooks.clone();
        let post_tool_call_hooks = self.post_tool_call_hooks.clone();
        let pre_agent_response_hooks = self.pre_agent_response_hooks.clone();
        let post_agent_response_hooks = self.post_agent_response_hooks.clone();
        let async_post_user_prompt_hooks = self.async_post_user_prompt_hooks.clone();
        let async_pre_tool_call_hooks = self.async_pre_tool_call_hooks.clone();
        let async_post_tool_call_hooks = self.async_post_tool_call_hooks.clone();
        let async_pre_agent_response_hooks = self.async_pre_agent_response_hooks.clone();
        let async_post_agent_response_hooks = self.async_post_agent_response_hooks.clone();
        let hook_timeout = self.script_timeout;
        let pending_hook_context = Arc::clone(&self.pending_hook_context);

        // Spawn task to handle the prompt and translate events
        tokio::spawn(async move {
            // Cancel any existing idle timer when a new turn starts processing.
            // This handles the case where a new prompt arrives while a previous
            // task's idle timer is pending but before submit() could cancel it.
            if let Some(abort_handle) = idle_timer_abort.lock().await.take() {
                abort_handle.abort();
            }

            // Send TaskStarted event
            let _ = event_tx
                .send(Event {
                    id: id_clone.clone(),
                    msg: EventMsg::TaskStarted(codex_protocol::protocol::TaskStartedEvent {
                        model_context_window: None,
                    }),
                })
                .await;

            // Spawn update consumer task that returns accumulated text for transcript
            let event_tx_clone = event_tx.clone();
            let id_for_updates = id_clone.clone();
            let transcript_recorder_for_updates = transcript_recorder.clone();
            let pre_tool_call_hooks_for_updates = pre_tool_call_hooks.clone();
            let post_tool_call_hooks_for_updates = post_tool_call_hooks.clone();
            let pre_agent_response_hooks_for_updates = pre_agent_response_hooks.clone();
            let async_pre_tool_call_hooks_for_updates = async_pre_tool_call_hooks.clone();
            let async_post_tool_call_hooks_for_updates = async_post_tool_call_hooks.clone();
            let async_pre_agent_response_hooks_for_updates = async_pre_agent_response_hooks.clone();
            let update_handler = tokio::spawn(async move {
                let mut event_sequence: u64 = 0;
                // Accumulate assistant text for transcript recording
                let mut accumulated_text = String::new();
                // Track whether pre_agent_response hook has fired
                let mut has_fired_pre_agent_response = false;
                let mut has_agent_text = false;
                let mut needs_agent_separator = false;
                // Track call_ids that have already been recorded to the transcript.
                let mut recorded_tool_call_ids: std::collections::HashSet<String> =
                    std::collections::HashSet::new();
                // Track pending patch operations: store FileChange data from ToolCall events
                // so we can emit PatchApplyBegin on ToolCallUpdate (after approval).
                let mut pending_patch_changes: std::collections::HashMap<
                    String,
                    std::collections::HashMap<PathBuf, codex_protocol::protocol::FileChange>,
                > = std::collections::HashMap::new();
                while let Some(update) = update_rx.recv().await {
                    if has_agent_text
                        && matches!(
                            update,
                            acp::SessionUpdate::ToolCall(_)
                                | acp::SessionUpdate::ToolCallUpdate(_)
                                | acp::SessionUpdate::Plan(_)
                                | acp::SessionUpdate::UserMessageChunk(_)
                                | acp::SessionUpdate::CurrentModeUpdate(_)
                                | acp::SessionUpdate::AvailableCommandsUpdate(_)
                        )
                    {
                        needs_agent_separator = true;
                    }
                    // Record tool calls and results to transcript
                    if let Some(ref recorder) = transcript_recorder_for_updates {
                        record_tool_events_to_transcript(
                            &update,
                            recorder,
                            &mut recorded_tool_call_ids,
                        )
                        .await;
                    }

                    // Execute pre_agent_response hooks on first agent message chunk
                    if let acp::SessionUpdate::AgentMessageChunk(chunk) = &update
                        && !has_fired_pre_agent_response
                        && let acp::ContentBlock::Text(text) = &chunk.content
                        && !text.text.is_empty()
                    {
                        has_fired_pre_agent_response = true;
                        if !pre_agent_response_hooks_for_updates.is_empty() {
                            let env_vars = HashMap::from([(
                                "NORI_HOOK_EVENT".to_string(),
                                "pre_agent_response".to_string(),
                            )]);
                            let results = crate::hooks::execute_hooks_with_env(
                                &pre_agent_response_hooks_for_updates,
                                hook_timeout,
                                &env_vars,
                            )
                            .await;
                            route_hook_results(&results, &event_tx_clone, &id_for_updates, None)
                                .await;
                        }
                        if !async_pre_agent_response_hooks_for_updates.is_empty() {
                            let env = HashMap::from([(
                                "NORI_HOOK_EVENT".to_string(),
                                "pre_agent_response".to_string(),
                            )]);
                            let _ = crate::hooks::execute_hooks_fire_and_forget(
                                async_pre_agent_response_hooks_for_updates.clone(),
                                hook_timeout,
                                env,
                            );
                        }
                    }

                    // Execute pre_tool_call hooks when a tool call begins
                    if let acp::SessionUpdate::ToolCall(tool_call) = &update {
                        let env_vars = HashMap::from([
                            ("NORI_HOOK_EVENT".to_string(), "pre_tool_call".to_string()),
                            ("NORI_HOOK_TOOL_NAME".to_string(), tool_call.title.clone()),
                            (
                                "NORI_HOOK_TOOL_ARGS".to_string(),
                                tool_call
                                    .raw_input
                                    .as_ref()
                                    .map_or_else(String::new, std::string::ToString::to_string),
                            ),
                        ]);
                        if !pre_tool_call_hooks_for_updates.is_empty() {
                            let results = crate::hooks::execute_hooks_with_env(
                                &pre_tool_call_hooks_for_updates,
                                hook_timeout,
                                &env_vars,
                            )
                            .await;
                            route_hook_results(&results, &event_tx_clone, &id_for_updates, None)
                                .await;
                        }
                        if !async_pre_tool_call_hooks_for_updates.is_empty() {
                            let _ = crate::hooks::execute_hooks_fire_and_forget(
                                async_pre_tool_call_hooks_for_updates.clone(),
                                hook_timeout,
                                env_vars.clone(),
                            );
                        }
                    }

                    // Execute post_tool_call hooks when a tool call completes
                    if let acp::SessionUpdate::ToolCallUpdate(tcu) = &update
                        && tcu.fields.status == Some(acp::ToolCallStatus::Completed)
                    {
                        let tool_output = extract_tool_output(&tcu.fields);
                        let env_vars = HashMap::from([
                            ("NORI_HOOK_EVENT".to_string(), "post_tool_call".to_string()),
                            (
                                "NORI_HOOK_TOOL_NAME".to_string(),
                                tcu.fields.title.clone().unwrap_or_default(),
                            ),
                            ("NORI_HOOK_TOOL_OUTPUT".to_string(), tool_output),
                        ]);
                        if !post_tool_call_hooks_for_updates.is_empty() {
                            let results = crate::hooks::execute_hooks_with_env(
                                &post_tool_call_hooks_for_updates,
                                hook_timeout,
                                &env_vars,
                            )
                            .await;
                            route_hook_results(&results, &event_tx_clone, &id_for_updates, None)
                                .await;
                        }
                        if !async_post_tool_call_hooks_for_updates.is_empty() {
                            let _ = crate::hooks::execute_hooks_fire_and_forget(
                                async_post_tool_call_hooks_for_updates.clone(),
                                hook_timeout,
                                env_vars.clone(),
                            );
                        }
                    }

                    let events =
                        translate_session_update_to_events(&update, &mut pending_patch_changes);
                    for mut event_msg in events {
                        // Accumulate text for transcript
                        if let EventMsg::AgentMessageDelta(ref mut delta) = event_msg {
                            if needs_agent_separator && has_agent_text {
                                if !delta.delta.starts_with('\n') {
                                    delta.delta =
                                        format!("\n{delta_text}", delta_text = delta.delta);
                                }
                                needs_agent_separator = false;
                            }
                            if !delta.delta.is_empty() {
                                has_agent_text = true;
                            }
                            accumulated_text.push_str(&delta.delta);
                        }
                        event_sequence += 1;
                        debug!(
                            target: "acp_event_flow",
                            seq = event_sequence,
                            event_type = get_event_msg_type(&event_msg),
                            "ACP dispatch: sending event to TUI"
                        );
                        let _ = event_tx_clone
                            .send(Event {
                                id: id_for_updates.clone(),
                                msg: event_msg,
                            })
                            .await;
                    }
                }
                debug!(
                    target: "acp_event_flow",
                    total_events = event_sequence,
                    "ACP dispatch: update stream completed"
                );
                accumulated_text
            });

            // Send the prompt (clone session_id before moving it since we need it for idle timer)
            let session_id_for_timer = session_id.to_string();
            let result = connection.prompt(session_id, prompt, update_tx).await;

            // Wait for all updates to be processed and get accumulated text
            let accumulated_text = update_handler.await.unwrap_or_default();

            // Record assistant message to transcript if there's accumulated text
            if !accumulated_text.is_empty()
                && let Some(ref recorder) = transcript_recorder
            {
                let content = vec![ContentBlock::Text {
                    text: accumulated_text.clone(),
                }];
                if let Err(e) = recorder
                    .record_assistant_message(&id_clone, content, None)
                    .await
                {
                    warn!("Failed to record assistant message to transcript: {e}");
                }
            }

            // Execute post_agent_response hooks after the agent has finished responding
            if !accumulated_text.is_empty() && !post_agent_response_hooks.is_empty() {
                let env_vars = HashMap::from([
                    (
                        "NORI_HOOK_EVENT".to_string(),
                        "post_agent_response".to_string(),
                    ),
                    (
                        "NORI_HOOK_RESPONSE_TEXT".to_string(),
                        accumulated_text.clone(),
                    ),
                ]);
                let results = crate::hooks::execute_hooks_with_env(
                    &post_agent_response_hooks,
                    hook_timeout,
                    &env_vars,
                )
                .await;
                route_hook_results(&results, &event_tx, &id_clone, None).await;
            }

            if !accumulated_text.is_empty() && !async_post_agent_response_hooks.is_empty() {
                let env_vars = HashMap::from([
                    (
                        "NORI_HOOK_EVENT".to_string(),
                        "post_agent_response".to_string(),
                    ),
                    (
                        "NORI_HOOK_RESPONSE_TEXT".to_string(),
                        accumulated_text.clone(),
                    ),
                ]);
                let _ = crate::hooks::execute_hooks_fire_and_forget(
                    async_post_agent_response_hooks,
                    hook_timeout,
                    env_vars,
                );
            }

            // Execute post_user_prompt hooks after the turn completes
            if !post_user_prompt_hooks.is_empty() {
                let env_vars = HashMap::from([
                    (
                        "NORI_HOOK_EVENT".to_string(),
                        "post_user_prompt".to_string(),
                    ),
                    (
                        "NORI_HOOK_PROMPT_TEXT".to_string(),
                        prompt_text_for_hooks.clone(),
                    ),
                ]);
                let results = crate::hooks::execute_hooks_with_env(
                    &post_user_prompt_hooks,
                    hook_timeout,
                    &env_vars,
                )
                .await;
                route_hook_results(&results, &event_tx, &id_clone, Some(&pending_hook_context))
                    .await;
            }

            if !async_post_user_prompt_hooks.is_empty() {
                let env_vars = HashMap::from([
                    (
                        "NORI_HOOK_EVENT".to_string(),
                        "post_user_prompt".to_string(),
                    ),
                    (
                        "NORI_HOOK_PROMPT_TEXT".to_string(),
                        prompt_text_for_hooks.clone(),
                    ),
                ]);
                let _ = crate::hooks::execute_hooks_fire_and_forget(
                    async_post_user_prompt_hooks,
                    hook_timeout,
                    env_vars,
                );
            }

            // If prompt failed, send an error event to the TUI BEFORE TaskComplete
            // This ensures the user sees why their request failed instead of a silent failure
            if let Err(ref e) = result {
                let error_string = format!("{e:?}");
                let category = categorize_acp_error(&error_string);
                let display_error = format!("{e:#}");

                // Generate user-friendly message based on error category
                let user_message = match category {
                    AcpErrorCategory::Authentication => {
                        format!(
                            "Authentication error: {display_error}. Please check your credentials or re-authenticate."
                        )
                    }
                    AcpErrorCategory::QuotaExceeded => {
                        format!("Rate limit or quota exceeded: {display_error}")
                    }
                    AcpErrorCategory::ExecutableNotFound => {
                        format!("Agent executable not found: {display_error}")
                    }
                    AcpErrorCategory::Initialization => {
                        format!("Agent initialization failed: {display_error}")
                    }
                    AcpErrorCategory::PromptTooLong => {
                        "Prompt is too long. Try using /compact to reduce context size, or start a new session."
                            .to_string()
                    }
                    AcpErrorCategory::Unknown => {
                        format!("ACP prompt failed: {display_error}")
                    }
                };

                warn!("ACP prompt failed: {}", e);
                debug!(
                    target: "acp_event_flow",
                    user_message = %user_message,
                    "ACP prompt failure: sending ErrorEvent to TUI"
                );

                // Send error event to TUI so user sees the error
                let _ = event_tx
                    .send(Event {
                        id: id_clone.clone(),
                        msg: EventMsg::Error(ErrorEvent {
                            message: user_message.clone(),
                            codex_error_info: None,
                        }),
                    })
                    .await;

                debug!(
                    target: "acp_event_flow",
                    "ACP prompt failure: ErrorEvent sent to TUI"
                );
            }

            // Send TaskComplete event (always, to end the turn)
            let _ = event_tx
                .send(Event {
                    id: id_clone,
                    msg: EventMsg::TaskComplete(codex_protocol::protocol::TaskCompleteEvent {
                        last_agent_message: None,
                    }),
                })
                .await;

            // Start idle timer if configured
            if let Some(duration) = notify_after_idle.as_duration() {
                let idle_secs = duration.as_secs();
                let user_notifier_for_timer = Arc::clone(&user_notifier);
                let idle_task = tokio::spawn(async move {
                    tokio::time::sleep(duration).await;
                    user_notifier_for_timer.notify(&codex_core::UserNotification::Idle {
                        session_id: session_id_for_timer,
                        idle_duration_secs: idle_secs,
                    });
                });
                // Store the abort handle so the timer can be cancelled on new activity
                *idle_timer_abort.lock().await = Some(idle_task.abort_handle());
            }
        });

        Ok(())
    }

    /// Handle an exec approval decision by finding and resolving the pending approval.
    async fn handle_exec_approval(&self, call_id: &str, decision: ReviewDecision) {
        let mut pending = self.pending_approvals.lock().await;
        if let Some(pos) = pending.iter().position(|r| r.event.call_id() == call_id) {
            let request = pending.remove(pos);
            let _ = request.response_tx.send(decision);
        } else {
            warn!("No pending approval found for call_id: {}", call_id);
        }
    }

    /// Handle the /compact operation by sending a summarization prompt to the agent,
    /// capturing the summary, and storing it for the next user prompt.
    ///
    /// This implements Option 3 (Prompt-Based Approach) from the implementation plan:
    /// 1. Send the summarization prompt to the agent
    /// 2. Capture the agent's summary response
    /// 3. Store it in pending_compact_summary
    /// 4. Emit ContextCompacted and Warning events
    async fn handle_compact(&self, id: &str) -> Result<()> {
        use codex_core::compact::SUMMARIZATION_PROMPT;

        // Build the summarization prompt
        let prompt = vec![translator::text_to_content_block(SUMMARIZATION_PROMPT)];

        // Create channel for receiving session updates
        let (update_tx, mut update_rx) = mpsc::channel(32);

        // Clone what we need for capturing the response
        let event_tx = self.event_tx.clone();
        let session_id = self.session_id.read().await.clone();
        let session_id_lock = Arc::clone(&self.session_id);
        let connection = Arc::clone(&self.connection);
        let cwd = self.cwd.clone();
        let id_clone = id.to_string();
        let pending_compact_summary = Arc::clone(&self.pending_compact_summary);
        let user_notifier = Arc::clone(&self.user_notifier);
        let idle_timer_abort = Arc::clone(&self.idle_timer_abort);
        let notify_after_idle = self.notify_after_idle;

        // Spawn task to handle the prompt and capture the summary
        tokio::spawn(async move {
            // Cancel any existing idle timer when a new turn starts processing
            if let Some(abort_handle) = idle_timer_abort.lock().await.take() {
                abort_handle.abort();
            }

            // Send TaskStarted event (inside spawned task for consistency)
            let _ = event_tx
                .send(Event {
                    id: id_clone.clone(),
                    msg: EventMsg::TaskStarted(codex_protocol::protocol::TaskStartedEvent {
                        model_context_window: None,
                    }),
                })
                .await;

            // Spawn update consumer task to capture the agent's response
            let event_tx_clone = event_tx.clone();
            let id_for_updates = id_clone.clone();
            let pending_summary_for_capture = Arc::clone(&pending_compact_summary);

            let update_handler = tokio::spawn(async move {
                let mut summary_text = String::new();
                let mut pending_patch_changes: std::collections::HashMap<
                    String,
                    std::collections::HashMap<PathBuf, codex_protocol::protocol::FileChange>,
                > = std::collections::HashMap::new();

                while let Some(update) = update_rx.recv().await {
                    // Capture text from agent message chunks
                    if let acp::SessionUpdate::AgentMessageChunk(chunk) = &update
                        && let acp::ContentBlock::Text(text) = &chunk.content
                    {
                        summary_text.push_str(&text.text);
                    }

                    // Translate and forward events to TUI for display
                    let events =
                        translate_session_update_to_events(&update, &mut pending_patch_changes);
                    for event_msg in events {
                        let _ = event_tx_clone
                            .send(Event {
                                id: id_for_updates.clone(),
                                msg: event_msg,
                            })
                            .await;
                    }
                }

                // Store the captured summary for use in the next prompt
                if !summary_text.is_empty() {
                    *pending_summary_for_capture.lock().await = Some(summary_text);
                }
            });

            // Send the summarization prompt
            let session_id_for_timer = session_id.to_string();
            let result = connection.prompt(session_id, prompt, update_tx).await;

            // Wait for all updates to be processed
            let _ = update_handler.await;

            // If prompt failed, send error event and clear any partial summary
            if let Err(ref e) = result {
                warn!("Compact prompt failed: {e}");
                // Clear any partial summary that may have been stored
                *pending_compact_summary.lock().await = None;
                let _ = event_tx
                    .send(Event {
                        id: id_clone.clone(),
                        msg: EventMsg::Error(ErrorEvent {
                            message: format!("Compact failed: {e}"),
                            codex_error_info: None,
                        }),
                    })
                    .await;
            } else {
                // Create a new session to clear the agent's conversation history.
                // The summary we captured will be prepended to the next user prompt,
                // giving the agent context about the previous conversation.
                match connection.create_session(&cwd).await {
                    Ok(new_session_id) => {
                        debug!("Created new session after compact: {:?}", new_session_id);
                        *session_id_lock.write().await = new_session_id;
                    }
                    Err(e) => {
                        warn!("Failed to create new session after compact: {e}");
                        // Continue anyway - summary will still be prepended but agent
                        // will retain its full history, which is suboptimal but functional
                    }
                }

                // Send ContextCompacted event to notify TUI
                let _ = event_tx
                    .send(Event {
                        id: id_clone.clone(),
                        msg: EventMsg::ContextCompacted(ContextCompactedEvent {}),
                    })
                    .await;

                // Send warning about long conversations
                let _ = event_tx
                    .send(Event {
                        id: id_clone.clone(),
                        msg: EventMsg::Warning(WarningEvent {
                            message: "Heads up: Long conversations and multiple compactions can cause the model to be less accurate. Start a new conversation when possible to keep conversations small and targeted.".to_string(),
                        }),
                    })
                    .await;
            }

            // Send TaskComplete event
            let _ = event_tx
                .send(Event {
                    id: id_clone,
                    msg: EventMsg::TaskComplete(codex_protocol::protocol::TaskCompleteEvent {
                        last_agent_message: None,
                    }),
                })
                .await;

            // Start idle timer if configured
            if let Some(duration) = notify_after_idle.as_duration() {
                let idle_secs = duration.as_secs();
                let user_notifier_for_timer = Arc::clone(&user_notifier);
                let idle_task = tokio::spawn(async move {
                    tokio::time::sleep(duration).await;
                    user_notifier_for_timer.notify(&codex_core::UserNotification::Idle {
                        session_id: session_id_for_timer,
                        idle_duration_secs: idle_secs,
                    });
                });
                // Store the abort handle so the timer can be cancelled on new activity
                *idle_timer_abort.lock().await = Some(idle_task.abort_handle());
            }
        });

        Ok(())
    }

    /// Send an error event to the TUI (only used in debug builds).
    #[cfg(debug_assertions)]
    async fn send_error(&self, message: &str) {
        let _ = self
            .event_tx
            .send(Event {
                id: String::new(),
                msg: EventMsg::Error(ErrorEvent {
                    message: message.to_string(),
                    codex_error_info: None,
                }),
            })
            .await;
    }

    /// Get the current model state from the ACP connection.
    ///
    /// Returns information about the current model and available models.
    /// This state is updated when a session is created or when the model is switched.
    pub fn model_state(&self) -> AcpModelState {
        self.connection.model_state()
    }

    /// Get the current session ID.
    ///
    /// Note: This clones the session ID since it may be replaced during /compact.
    pub async fn session_id(&self) -> acp::SessionId {
        self.session_id.read().await.clone()
    }

    /// Get a reference to the underlying ACP connection.
    ///
    /// This provides access to low-level ACP operations like model switching.
    pub fn connection(&self) -> &Arc<AcpConnection> {
        &self.connection
    }

    /// Switch to a different model for the current session.
    ///
    /// This sends a `session/set_model` request to the ACP agent and updates
    /// the internal model state. The model_id must be one of the available
    /// models returned by `model_state().available_models`.
    ///
    /// # Arguments
    /// * `model_id` - The ID of the model to switch to
    ///
    /// # Errors
    /// Returns an error if the model switch fails (e.g., invalid model ID,
    /// agent doesn't support model switching, or connection error).
    #[cfg(feature = "unstable")]
    pub async fn set_model(&self, model_id: &acp::ModelId) -> Result<()> {
        let session_id = self.session_id.read().await;
        self.connection.set_model(&session_id, model_id).await
    }

    /// Background task to handle approval requests from the ACP connection.
    ///
    /// When `approval_policy` is `AskForApproval::Never` (yolo mode), requests
    /// are auto-approved without prompting the user.
    async fn run_approval_handler(
        mut approval_rx: mpsc::Receiver<ApprovalRequest>,
        event_tx: mpsc::Sender<Event>,
        pending_approvals: Arc<Mutex<Vec<ApprovalRequest>>>,
        user_notifier: Arc<codex_core::UserNotifier>,
        cwd: PathBuf,
        approval_policy_rx: watch::Receiver<AskForApproval>,
    ) {
        while let Some(request) = approval_rx.recv().await {
            // Check current approval policy (may have changed via OverrideTurnContext)
            let current_policy = *approval_policy_rx.borrow();

            // If approval_policy is Never (yolo mode), auto-approve immediately
            if current_policy == AskForApproval::Never {
                debug!(
                    target: "acp_event_flow",
                    call_id = %request.event.call_id(),
                    "Auto-approving request (approval_policy=Never)"
                );
                let _ = request.response_tx.send(ReviewDecision::Approved);
                continue;
            }

            // Send the appropriate approval request event to TUI based on operation type.
            // Use the call_id as the event wrapper ID so that the TUI can
            // correctly route the user's decision back to this pending request.
            let (id, msg, command_for_notification) = match &request.event {
                ApprovalEventType::Exec(exec_event) => (
                    exec_event.call_id.clone(),
                    EventMsg::ExecApprovalRequest(exec_event.clone()),
                    exec_event.command.join(" "),
                ),
                ApprovalEventType::Patch(patch_event) => (
                    patch_event.call_id.clone(),
                    EventMsg::ApplyPatchApprovalRequest(patch_event.clone()),
                    format!(
                        "patch: {}",
                        patch_event
                            .changes
                            .keys()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                ),
            };

            // Send the approval event to the TUI first, then notify.
            // Notification must come after event delivery because
            // notif.show() can block on some platforms (e.g. macOS),
            // which would prevent the TUI from ever receiving the event.
            let _ = event_tx
                .send(Event {
                    id: id.clone(),
                    msg,
                })
                .await;

            // Store the pending approval for later resolution
            pending_approvals.lock().await.push(request);

            // Send OS notification (non-blocking, but ordered after event delivery)
            user_notifier.notify(&codex_core::UserNotification::AwaitingApproval {
                call_id: id,
                command: command_for_notification,
                cwd: cwd.display().to_string(),
            });
        }
    }

    /// Background task that relays inter-turn notifications from the persistent
    /// listener channel to the TUI event stream.
    ///
    /// The persistent listener receives `SessionUpdate`s that arrive after
    /// `unregister_session` has been called (i.e. between prompt turns). Without
    /// this relay, those updates would be silently dropped.
    async fn run_persistent_relay(
        mut persistent_rx: mpsc::Receiver<acp::SessionUpdate>,
        event_tx: mpsc::Sender<Event>,
    ) {
        let mut pending_patch_changes = HashMap::new();
        while let Some(update) = persistent_rx.recv().await {
            let event_msgs =
                translate_session_update_to_events(&update, &mut pending_patch_changes);
            for msg in event_msgs {
                let _ = event_tx
                    .send(Event {
                        id: String::new(),
                        msg,
                    })
                    .await;
            }
        }
    }
}

/// Spawn a separate ACP connection, send a summarization prompt, and emit a
/// `PromptSummary` event with the result. Designed to be called as a
/// fire-and-forget task from `handle_user_input`.
async fn run_prompt_summary(
    event_tx: &mpsc::Sender<Event>,
    agent_name: &str,
    cwd: &std::path::Path,
    user_prompt: &str,
    auto_worktree: bool,
    auto_worktree_repo_root: Option<&std::path::Path>,
) -> Result<()> {
    use tokio::time::Duration;
    use tokio::time::timeout;

    let agent_config = get_agent_config(agent_name)?;
    let connection = AcpConnection::spawn(&agent_config, cwd).await?;
    let session_id = connection.create_session(cwd).await?;

    let summarization_prompt = format!(
        "Summarize the following user request in 5 words or fewer. \
         Reply with ONLY the summary, no extra text.\n\n{user_prompt}"
    );
    let prompt = vec![translator::text_to_content_block(&summarization_prompt)];

    let (update_tx, mut update_rx) = mpsc::channel::<acp::SessionUpdate>(32);

    // Consume updates in a task to accumulate the agent's text response
    let collector = tokio::spawn(async move {
        let mut text = String::new();
        while let Some(update) = update_rx.recv().await {
            if let acp::SessionUpdate::AgentMessageChunk(chunk) = &update
                && let acp::ContentBlock::Text(t) = &chunk.content
            {
                text.push_str(&t.text);
            }
        }
        text
    });

    // Send the prompt with a timeout to prevent indefinite hangs
    let prompt_result = timeout(
        Duration::from_secs(30),
        connection.prompt(session_id, prompt, update_tx),
    )
    .await;

    // Drop the connection on a blocking thread to avoid blocking the async
    // runtime (AcpConnection::drop does a synchronous recv_timeout).
    tokio::task::spawn_blocking(move || drop(connection));

    match prompt_result {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => return Err(e),
        Err(_) => {
            debug!("Prompt summary timed out");
            return Ok(());
        }
    }

    let mut summary = collector.await.unwrap_or_default().trim().to_string();
    // Truncate to prevent a runaway response from dominating the footer
    if summary.chars().count() > 40 {
        summary = summary.chars().take(37).collect::<String>();
        summary.push_str("...");
    }
    if !summary.is_empty() {
        // If auto_worktree is enabled, rename the branch based on the summary.
        // Only the branch is renamed; the directory stays unchanged so that
        // processes running inside the worktree are not disrupted.
        if auto_worktree && let Some(repo_root) = auto_worktree_repo_root {
            let cwd_owned = cwd.to_path_buf();
            let repo_root = repo_root.to_path_buf();
            let summary_for_rename = summary.clone();
            let rename_result = tokio::task::spawn_blocking(move || {
                let dir_name = cwd_owned.file_name().and_then(|n| n.to_str()).unwrap_or("");
                let old_branch = format!("auto/{dir_name}");
                crate::auto_worktree::rename_auto_worktree_branch(
                    &repo_root,
                    &old_branch,
                    &summary_for_rename,
                )
            })
            .await;

            match rename_result {
                Ok(Ok(())) => {
                    debug!("Auto-worktree branch renamed based on summary");
                }
                Ok(Err(e)) => {
                    warn!("Failed to rename auto-worktree branch (non-fatal): {e}");
                }
                Err(e) => {
                    warn!("Auto-worktree branch rename task panicked (non-fatal): {e}");
                }
            }
        }

        let _ = event_tx
            .send(Event {
                id: String::new(),
                msg: EventMsg::PromptSummary(PromptSummaryEvent { summary }),
            })
            .await;
    }

    Ok(())
}

/// Return the custom commands directory: `{nori_home}/commands`.
fn commands_dir(nori_home: &std::path::Path) -> PathBuf {
    nori_home.join("commands")
}

/// Execute session_start hooks and emit warnings for any failures.
/// Route parsed hook results to the appropriate event channels.
///
/// For each successful hook result with output:
/// - `Log` lines go to `tracing::info!`
/// - `Output`/`OutputWarn`/`OutputError` lines become `HookOutput` events
/// - `Context` lines accumulate into `pending_hook_context` (if provided)
///
/// Failed hooks emit `Warning` events.
async fn route_hook_results(
    results: &[crate::hooks::HookResult],
    event_tx: &mpsc::Sender<Event>,
    event_id: &str,
    pending_hook_context: Option<&Mutex<Option<String>>>,
) {
    for result in results {
        if !result.success {
            if let Some(ref err) = result.error {
                let _ = event_tx
                    .send(Event {
                        id: event_id.to_string(),
                        msg: EventMsg::Warning(WarningEvent {
                            message: err.clone(),
                        }),
                    })
                    .await;
            }
            continue;
        }
        if let Some(ref output) = result.output {
            let parsed = crate::hooks::parse_hook_output(output);
            for line in parsed {
                match line {
                    crate::hooks::HookOutputLine::Log(msg) => {
                        tracing::info!("hook [{}]: {msg}", result.path);
                    }
                    crate::hooks::HookOutputLine::Output(msg) => {
                        let _ = event_tx
                            .send(Event {
                                id: event_id.to_string(),
                                msg: EventMsg::HookOutput(HookOutputEvent {
                                    message: msg,
                                    level: HookOutputLevel::Info,
                                }),
                            })
                            .await;
                    }
                    crate::hooks::HookOutputLine::OutputWarn(msg) => {
                        let _ = event_tx
                            .send(Event {
                                id: event_id.to_string(),
                                msg: EventMsg::HookOutput(HookOutputEvent {
                                    message: msg,
                                    level: HookOutputLevel::Warn,
                                }),
                            })
                            .await;
                    }
                    crate::hooks::HookOutputLine::OutputError(msg) => {
                        let _ = event_tx
                            .send(Event {
                                id: event_id.to_string(),
                                msg: EventMsg::HookOutput(HookOutputEvent {
                                    message: msg,
                                    level: HookOutputLevel::Error,
                                }),
                            })
                            .await;
                    }
                    crate::hooks::HookOutputLine::Context(ctx) => {
                        if let Some(lock) = pending_hook_context {
                            let mut guard = lock.lock().await;
                            match guard.as_mut() {
                                Some(existing) => {
                                    existing.push('\n');
                                    existing.push_str(&ctx);
                                }
                                None => {
                                    *guard = Some(ctx);
                                }
                            }
                        } else {
                            warn!(
                                "Hook emitted ::context:: line but this hook type does not support context injection; line discarded: {ctx}"
                            );
                        }
                    }
                }
            }
        }
    }
}

async fn run_session_start_hooks(
    hooks: &[PathBuf],
    timeout: std::time::Duration,
    event_tx: &mpsc::Sender<Event>,
    pending_hook_context: Option<&Mutex<Option<String>>>,
) {
    if hooks.is_empty() {
        return;
    }
    let results = crate::hooks::execute_hooks(hooks, timeout).await;
    route_hook_results(&results, event_tx, "", pending_hook_context).await;
}

/// Generate a unique ID for operations
fn generate_id() -> String {
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    format!("acp-{}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Get a human-readable name for an Op variant
fn get_op_name(op: &Op) -> &'static str {
    match op {
        Op::Interrupt => "Interrupt",
        Op::UserInput { .. } => "UserInput",
        Op::UserTurn { .. } => "UserTurn",
        Op::OverrideTurnContext { .. } => "OverrideTurnContext",
        Op::ExecApproval { .. } => "ExecApproval",
        Op::PatchApproval { .. } => "PatchApproval",
        Op::ResolveElicitation { .. } => "ResolveElicitation",
        Op::AddToHistory { .. } => "AddToHistory",
        Op::GetHistoryEntryRequest { .. } => "GetHistoryEntryRequest",
        Op::SearchHistoryRequest { .. } => "SearchHistoryRequest",
        Op::ListMcpTools => "ListMcpTools",
        Op::ListCustomPrompts => "ListCustomPrompts",
        Op::Compact => "Compact",
        Op::Undo => "Undo",
        Op::UndoList => "UndoList",
        Op::UndoTo { .. } => "UndoTo",
        Op::Shutdown => "Shutdown",
        Op::RunUserShellCommand { .. } => "RunUserShellCommand",
        _ => "Unknown",
    }
}

/// Get a human-readable name for an EventMsg variant
fn get_event_msg_type(msg: &EventMsg) -> &'static str {
    match msg {
        EventMsg::SessionConfigured(_) => "SessionConfigured",
        EventMsg::TaskStarted(_) => "TaskStarted",
        EventMsg::TaskComplete(_) => "TaskComplete",
        EventMsg::AgentMessageDelta(_) => "AgentMessageDelta",
        EventMsg::AgentReasoningDelta(_) => "AgentReasoningDelta",
        EventMsg::ExecCommandBegin(_) => "ExecCommandBegin",
        EventMsg::ExecCommandEnd(_) => "ExecCommandEnd",
        EventMsg::ExecApprovalRequest(_) => "ExecApprovalRequest",
        EventMsg::TurnAborted(_) => "TurnAborted",
        EventMsg::Error(_) => "Error",
        EventMsg::ShutdownComplete => "ShutdownComplete",
        _ => "Other",
    }
}

/// Translate an ACP SessionUpdate to codex_protocol::EventMsg variants.
///
/// The `pending_patch_changes` map stores FileChange data from ToolCall events
/// so that it can be retrieved when ToolCallUpdate arrives (after approval).
fn translate_session_update_to_events(
    update: &acp::SessionUpdate,
    pending_patch_changes: &mut std::collections::HashMap<
        String,
        std::collections::HashMap<PathBuf, codex_protocol::protocol::FileChange>,
    >,
) -> Vec<EventMsg> {
    match update {
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                debug!(
                    target: "acp_event_flow",
                    event_type = "AgentMessageChunk",
                    delta_len = text.text.len(),
                    delta_preview = %truncate_for_log(&text.text, 50),
                    "ACP -> TUI: streaming text delta"
                );
                vec![EventMsg::AgentMessageDelta(
                    codex_protocol::protocol::AgentMessageDeltaEvent {
                        delta: text.text.clone(),
                    },
                )]
            } else {
                vec![]
            }
        }
        acp::SessionUpdate::AgentThoughtChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                debug!(
                    target: "acp_event_flow",
                    event_type = "AgentThoughtChunk",
                    delta_len = text.text.len(),
                    "ACP -> TUI: reasoning delta"
                );
                vec![EventMsg::AgentReasoningDelta(
                    codex_protocol::protocol::AgentReasoningDeltaEvent {
                        delta: text.text.clone(),
                    },
                )]
            } else {
                vec![]
            }
        }
        acp::SessionUpdate::ToolCall(tool_call) => {
            // Skip Begin events that don't have useful display information.
            // The ACP protocol emits multiple ToolCall events for the same call_id:
            // 1. First event: generic (title="Read File", raw_input={} or partial)
            // 2. Second event: detailed (title="Read /path/to/file.rs", raw_input={path: "..."})
            // We only want to emit the detailed one to avoid duplicate Begin events in the TUI.
            //
            // Check for useful info in EITHER:
            // - raw_input (has path, command, pattern, etc.)
            // - title itself (contains an absolute path like "Read /home/...")
            let display_args = tool_call
                .raw_input
                .as_ref()
                .and_then(|input| extract_display_args(&tool_call.title, input));
            let title_has_path = title_contains_useful_info(&tool_call.title);
            if display_args.is_none() && !title_has_path {
                debug!(
                    target: "acp_event_flow",
                    event_type = "ToolCall",
                    call_id = %tool_call.tool_call_id,
                    title = %tool_call.title,
                    has_raw_input = tool_call.raw_input.is_some(),
                    title_has_path = title_has_path,
                    "ACP: skipping generic ToolCall (no display args), waiting for detailed event"
                );
                return vec![];
            }

            // For patch operations (Edit/Write/Delete), don't emit anything on ToolCall.
            // Store the FileChange data so we can emit PatchApplyBegin on ToolCallUpdate.
            // The approval request will be shown first via ApplyPatchApprovalRequest.
            if is_patch_operation(
                Some(&tool_call.kind),
                &tool_call.title,
                tool_call.raw_input.as_ref(),
            ) && let Some((path, change)) =
                tool_call_to_file_change(Some(&tool_call.kind), tool_call.raw_input.as_ref())
            {
                let mut changes = std::collections::HashMap::new();
                changes.insert(path, change);

                // Store for retrieval on ToolCallUpdate
                pending_patch_changes.insert(tool_call.tool_call_id.to_string(), changes);

                debug!(
                    target: "acp_event_flow",
                    event_type = "ToolCall",
                    call_id = %tool_call.tool_call_id,
                    title = %tool_call.title,
                    kind = ?tool_call.kind,
                    "ACP: stored patch changes for later (will show after approval)"
                );
                return vec![];
            }

            // Format command with tool name and input arguments for better display
            let command = format_tool_call_command(&tool_call.title, tool_call.raw_input.as_ref());
            // Classify the tool call to enable proper TUI rendering (Exploring vs Command mode)
            let parsed_cmd = classify_tool_to_parsed_command(
                &tool_call.title,
                Some(&tool_call.kind),
                tool_call.raw_input.as_ref(),
            );
            debug!(
                target: "acp_event_flow",
                event_type = "ToolCall",
                call_id = %tool_call.tool_call_id,
                title = %tool_call.title,
                kind = ?tool_call.kind,
                command = %command,
                parsed_cmd_count = parsed_cmd.len(),
                has_raw_input = tool_call.raw_input.is_some(),
                "ACP -> TUI: ExecCommandBegin (tool call started)"
            );
            vec![EventMsg::ExecCommandBegin(
                codex_protocol::protocol::ExecCommandBeginEvent {
                    call_id: tool_call.tool_call_id.to_string(),
                    process_id: None,
                    turn_id: String::new(),
                    command: vec![command],
                    cwd: PathBuf::new(),
                    parsed_cmd,
                    source: codex_protocol::protocol::ExecCommandSource::Agent,
                    interaction_input: None,
                },
            )]
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            // Tool call updates can be mapped based on status
            let status = update.fields.status;
            let title = update.fields.title.clone().unwrap_or_default();
            debug!(
                target: "acp_event_flow",
                event_type = "ToolCallUpdate",
                call_id = %update.tool_call_id,
                status = ?status,
                title = %title,
                "ACP: tool call update received"
            );
            if status == Some(acp::ToolCallStatus::Completed) {
                // Check if we have stored patch changes from the original ToolCall event.
                // This data was stored when we first saw the ToolCall, before approval.
                let call_id = update.tool_call_id.to_string();
                if let Some(changes) = pending_patch_changes.remove(&call_id) {
                    debug!(
                        target: "acp_event_flow",
                        event_type = "ToolCallUpdate",
                        call_id = %call_id,
                        title = %title,
                        num_files = changes.len(),
                        "ACP -> TUI: PatchApplyBegin (showing completed file operation)"
                    );

                    // Use PatchApplyBegin to create the history cell with the diff
                    return vec![EventMsg::PatchApplyBegin(PatchApplyBeginEvent {
                        call_id,
                        turn_id: String::new(),
                        auto_approved: true, // Already approved by this point
                        changes,
                    })];
                }

                // Extract output from tool call content and raw_output
                let aggregated_output = extract_tool_output(&update.fields);
                let command = format_tool_call_command(&title, update.fields.raw_input.as_ref());
                // Classify the tool call to enable proper TUI rendering (Exploring vs Command mode)
                let parsed_cmd = classify_tool_to_parsed_command(
                    &title,
                    update.fields.kind.as_ref(),
                    update.fields.raw_input.as_ref(),
                );

                debug!(
                    target: "acp_event_flow",
                    event_type = "ToolCallUpdate",
                    call_id = %update.tool_call_id,
                    title = %title,
                    command = %command,
                    output_len = aggregated_output.len(),
                    "ACP -> TUI: ExecCommandEnd (tool call completed)"
                );
                vec![EventMsg::ExecCommandEnd(
                    codex_protocol::protocol::ExecCommandEndEvent {
                        call_id: update.tool_call_id.to_string(),
                        process_id: None,
                        turn_id: String::new(),
                        command: vec![command],
                        cwd: PathBuf::new(),
                        parsed_cmd,
                        source: codex_protocol::protocol::ExecCommandSource::Agent,
                        interaction_input: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        aggregated_output,
                        exit_code: 0,
                        duration: std::time::Duration::ZERO,
                        formatted_output: String::new(),
                    },
                )]
            } else {
                vec![]
            }
        }
        // Other update types don't have direct event mappings
        other => {
            debug!(
                target: "acp_event_flow",
                event_type = ?std::mem::discriminant(other),
                "ACP: unhandled update type (no event emitted)"
            );
            vec![]
        }
    }
}

/// Record tool call and result events to the transcript.
///
/// This handles recording both regular tool calls (as ToolCall/ToolResult entries)
/// and patch operations (as PatchApply entries). Patch operations (Edit/Write/Delete)
/// are recorded separately because they represent file modifications rather than
/// generic tool invocations.
async fn record_tool_events_to_transcript(
    update: &acp::SessionUpdate,
    recorder: &TranscriptRecorder,
    recorded_call_ids: &mut std::collections::HashSet<String>,
) {
    match update {
        acp::SessionUpdate::ToolCall(tool_call) => {
            let call_id = tool_call.tool_call_id.to_string();

            // Skip if we've already recorded this call_id (ACP may send multiple
            // ToolCall events for the same call_id as details become available)
            if recorded_call_ids.contains(&call_id) {
                return;
            }

            // Skip patch operations here - they're recorded on ToolCallUpdate completion
            if is_patch_operation(
                Some(&tool_call.kind),
                &tool_call.title,
                tool_call.raw_input.as_ref(),
            ) {
                return;
            }

            // Record non-patch tool calls
            let input = tool_call.raw_input.clone().unwrap_or(serde_json::json!({}));
            if let Err(e) = recorder
                .record_tool_call(&call_id, &tool_call.title, &input)
                .await
            {
                warn!("Failed to record tool call to transcript: {e}");
            } else {
                recorded_call_ids.insert(call_id);
            }
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            // Only record completed tool calls
            if update.fields.status != Some(acp::ToolCallStatus::Completed) {
                return;
            }

            let call_id = update.tool_call_id.to_string();
            let title = update.fields.title.clone().unwrap_or_default();
            let kind = update.fields.kind;

            // Check if this is a patch operation
            if is_patch_operation(kind.as_ref(), &title, update.fields.raw_input.as_ref()) {
                // Record as patch operation
                let operation = match kind {
                    Some(acp::ToolKind::Edit) => crate::transcript::PatchOperationType::Edit,
                    Some(acp::ToolKind::Delete) => crate::transcript::PatchOperationType::Delete,
                    _ => {
                        // Default to Write for other kinds (including None)
                        crate::transcript::PatchOperationType::Write
                    }
                };

                // Extract path from raw_input or locations
                let path = update
                    .fields
                    .raw_input
                    .as_ref()
                    .and_then(|input| {
                        input
                            .get("file_path")
                            .or_else(|| input.get("path"))
                            .and_then(|v| v.as_str())
                            .map(PathBuf::from)
                    })
                    .or_else(|| {
                        update
                            .fields
                            .locations
                            .as_ref()
                            .and_then(|locs| locs.first())
                            .map(|loc| loc.path.clone())
                    })
                    .unwrap_or_else(|| PathBuf::from("unknown"));

                // Completed status means success (Failed status handled separately)
                if let Err(e) = recorder
                    .record_patch_apply(&call_id, operation, &path, true, None)
                    .await
                {
                    warn!("Failed to record patch apply to transcript: {e}");
                }
            } else {
                // Record as tool result for non-patch operations
                let output = extract_tool_output(&update.fields);
                let truncated = output.len() > 10000;
                let output_to_record = if truncated {
                    let safe = codex_utils_string::take_bytes_at_char_boundary(&output, 10000);
                    format!("{safe}... (truncated)")
                } else {
                    output
                };

                // Extract exit_code from raw_output if available
                let exit_code = update
                    .fields
                    .raw_output
                    .as_ref()
                    .and_then(|v| v.get("exit_code"))
                    .and_then(serde_json::Value::as_i64)
                    .map(|v| v as i32);

                if let Err(e) = recorder
                    .record_tool_result(&call_id, &output_to_record, truncated, exit_code)
                    .await
                {
                    warn!("Failed to record tool result to transcript: {e}");
                }
            }
        }
        _ => {}
    }
}

/// Truncate a string for logging purposes
fn truncate_for_log(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let safe = codex_utils_string::take_bytes_at_char_boundary(s, max_len);
        format!("{safe}...")
    }
}

/// Check if a tool call title contains useful display information.
///
/// Some ACP providers include the path/command directly in the title
/// (e.g., "Read /home/user/file.rs" or "`git status`") rather than in raw_input.
/// This function detects such cases so we don't skip them.
fn title_contains_useful_info(title: &str) -> bool {
    // Check for absolute paths (Unix or Windows style)
    if title.contains(" /") || title.contains(" C:\\") || title.contains(" ~") {
        return true;
    }
    // Check for backtick-quoted commands (e.g., "`git status`")
    if title.contains('`') {
        return true;
    }
    // Check for patterns that suggest it's not a generic title
    // Generic titles are typically just the tool name like "Read File", "Terminal", "Search"
    let generic_patterns = [
        "Read File",
        "Read file",
        "Terminal",
        "Search",
        "Grep",
        "Glob",
        "List",
        "Write",
        "Edit",
    ];
    for pattern in &generic_patterns {
        if title == *pattern {
            return false;
        }
    }
    // If the title is longer than typical generic names and contains a space,
    // it likely has useful info
    title.len() > 15 && title.contains(' ')
}

/// Format a tool call command with its input arguments for display.
///
/// Creates a display string like "Read(path/to/file.rs)" or "Terminal(git status)"
fn format_tool_call_command(title: &str, raw_input: Option<&serde_json::Value>) -> String {
    let args = raw_input
        .and_then(|input| extract_display_args(title, input))
        .unwrap_or_default();

    if args.is_empty() {
        title.to_string()
    } else if title.contains(&args) {
        // Don't append args if they're already contained in the title
        title.to_string()
    } else {
        format!("{title}({args})")
    }
}

/// Extract display-friendly arguments from raw_input based on tool type.
fn extract_display_args(title: &str, input: &serde_json::Value) -> Option<String> {
    let title_lower = title.to_lowercase();

    // Try to extract the most relevant argument based on tool type
    // Note: Order matters - more specific matches should come first
    if title_lower.contains("search")
        || title_lower.contains("find")
        || title_lower.contains("grep")
    {
        // For search operations, show the pattern/query
        let pattern = input
            .get("pattern")
            .or_else(|| input.get("query"))
            .or_else(|| input.get("glob"))
            .and_then(|v| v.as_str());
        let path = input.get("path").and_then(|v| v.as_str());

        match (pattern, path) {
            (Some(p), Some(dir)) => Some(format!("{p} in {dir}")),
            (Some(p), None) => Some(p.to_string()),
            (None, Some(dir)) => Some(dir.to_string()),
            (None, None) => None,
        }
    } else if title_lower.contains("terminal")
        || title_lower.contains("shell")
        || title_lower.contains("bash")
        || title_lower.contains("exec")
    {
        // For shell commands, show the command
        input
            .get("command")
            .or_else(|| input.get("cmd"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else if title_lower.contains("list") || title_lower.contains("ls") {
        // For list operations, show the path
        input
            .get("path")
            .or_else(|| input.get("directory"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else if title_lower.contains("write") || title_lower.contains("edit") {
        // For write operations, show the path
        input
            .get("path")
            .or_else(|| input.get("file_path"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else if title_lower.contains("read") || title_lower.contains("file") {
        // For file read operations, show the path
        input
            .get("path")
            .or_else(|| input.get("file_path"))
            .or_else(|| input.get("file"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else {
        // Generic fallback: try common argument names
        input
            .get("path")
            .or_else(|| input.get("command"))
            .or_else(|| input.get("query"))
            .or_else(|| input.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    }
}

/// Extract tool output from ToolCallUpdateFields for display.
///
/// Returns a formatted string containing the tool's output content.
/// Prioritizes rawOutput fields (Codex format) over content field, and strips
/// markdown code blocks from the output.
fn extract_tool_output(fields: &acp::ToolCallUpdateFields) -> String {
    // Try rawOutput first (Codex provides structured output here)
    if let Some(raw_output) = &fields.raw_output {
        // Try to extract stdout (most common for shell commands)
        if let Some(stdout) = raw_output.get("stdout").and_then(|v| v.as_str())
            && !stdout.is_empty()
        {
            return strip_markdown_code_blocks(stdout);
        }

        // Try formatted_output next
        if let Some(formatted) = raw_output.get("formatted_output").and_then(|v| v.as_str())
            && !formatted.is_empty()
        {
            return strip_markdown_code_blocks(formatted);
        }

        // Try aggregated_output as fallback
        if let Some(aggregated) = raw_output.get("aggregated_output").and_then(|v| v.as_str())
            && !aggregated.is_empty()
        {
            return strip_markdown_code_blocks(aggregated);
        }

        // If none of the direct fields worked, try format_raw_output for summaries
        if let Some(output_str) = format_raw_output(raw_output, fields.title.as_deref()) {
            return output_str;
        }
    }

    // Fallback to content field (existing behavior for non-Codex agents)
    let mut output_parts: Vec<String> = Vec::new();
    if let Some(content) = &fields.content {
        for item in content {
            if let acp::ToolCallContent::Content(c) = item
                && let acp::ContentBlock::Text(text) = &c.content
                && !text.text.is_empty()
            {
                // Strip markdown from content field too
                output_parts.push(strip_markdown_code_blocks(&text.text));
            }
        }
    }

    output_parts.join("\n")
}

/// Strip markdown code block formatting from output.
///
/// Codex wraps output in markdown code blocks like:
/// ````text
/// ```sh
/// output here
/// ```
/// ````
///
/// This function removes the wrapper and returns just the content.
fn strip_markdown_code_blocks(text: &str) -> String {
    let text = text.trim();

    // Check for code block pattern: ```language\n...\n```
    if text.starts_with("```") {
        // Find the end of the opening marker (first newline after ```)
        if let Some(start) = text.find('\n') {
            // Find the closing ```
            if let Some(end) = text.rfind("\n```") {
                // Extract content between markers
                return text[start + 1..end].to_string();
            }
        }
    }

    // No markdown wrapper found, return as-is
    text.to_string()
}

/// Format raw_output JSON into a human-readable string based on tool type.
fn format_raw_output(raw_output: &serde_json::Value, title: Option<&str>) -> Option<String> {
    let title_lower = title.map(str::to_lowercase).unwrap_or_default();

    // Try to provide meaningful summaries based on common output patterns
    if let Some(obj) = raw_output.as_object() {
        // Check for line count (common in read operations)
        if let Some(lines) = obj.get("lines").and_then(serde_json::Value::as_u64) {
            return Some(format!("Read {lines} lines"));
        }

        // Check for file count (common in find/search operations)
        if let Some(count) = obj.get("count").and_then(serde_json::Value::as_u64) {
            if title_lower.contains("find") || title_lower.contains("search") {
                return Some(format!("Found {count} files"));
            }
            return Some(format!("{count} matches"));
        }

        // Check for files array
        if let Some(files) = obj.get("files").and_then(|v| v.as_array()) {
            let count = files.len();
            let file_list: Vec<&str> = files.iter().filter_map(|f| f.as_str()).take(5).collect();
            if count > 5 {
                return Some(format!(
                    "Found {} files\n{}...",
                    count,
                    file_list.join("\n")
                ));
            } else if !file_list.is_empty() {
                return Some(format!("Found {} files\n{}", count, file_list.join("\n")));
            }
        }

        // Check for exit_code (common in shell operations)
        if let Some(exit_code) = obj.get("exit_code").and_then(serde_json::Value::as_i64) {
            // Look for stdout/output
            let output = obj
                .get("stdout")
                .or_else(|| obj.get("output"))
                .and_then(|v| v.as_str());
            if let Some(out) = output {
                if exit_code != 0 {
                    return Some(format!("Exit code: {exit_code}\n{out}"));
                }
                return Some(out.to_string());
            }
            if exit_code != 0 {
                return Some(format!("Exit code: {exit_code}"));
            }
        }

        // Check for success boolean
        if let Some(success) = obj.get("success").and_then(serde_json::Value::as_bool)
            && !success
        {
            if let Some(error) = obj.get("error").and_then(|v| v.as_str()) {
                return Some(format!("Failed: {error}"));
            }
            return Some("Operation failed".to_string());
        }
    }

    // For arrays, show count
    if let Some(arr) = raw_output.as_array()
        && !arr.is_empty()
    {
        return Some(format!("{} items", arr.len()));
    }

    // For strings, return directly
    if let Some(s) = raw_output.as_str()
        && !s.is_empty()
    {
        return Some(s.to_string());
    }

    None
}

/// Classify a tool call into ParsedCommand variants based on ACP ToolKind.
///
/// This enables the TUI to render tool calls appropriately:
/// - `Read`, `ListFiles`, `Search` → "Exploring" mode with compact, grouped display
/// - `Unknown` → "Command" mode with full command text display
///
/// # ACP ToolKind mappings:
/// - `Read` → `ParsedCommand::Read` (exploring)
/// - `Search` → `ParsedCommand::Search` (exploring)
/// - `Edit`, `Delete`, `Move`, `Execute`, `Fetch` → `ParsedCommand::Unknown` (command)
/// - `Think`, `Other` → `ParsedCommand::Unknown` (command)
fn classify_tool_to_parsed_command(
    title: &str,
    kind: Option<&acp::ToolKind>,
    raw_input: Option<&serde_json::Value>,
) -> Vec<ParsedCommand> {
    match kind {
        // Read operations → Exploring mode
        Some(acp::ToolKind::Read) => {
            let path = raw_input
                .and_then(|i| {
                    i.get("path")
                        .or_else(|| i.get("file_path"))
                        .or_else(|| i.get("file"))
                })
                .and_then(|v| v.as_str())
                .unwrap_or("file");
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string());
            vec![ParsedCommand::Read {
                cmd: title.to_string(),
                name,
                path: std::path::PathBuf::from(path),
            }]
        }

        // Search operations → Exploring mode
        Some(acp::ToolKind::Search) => {
            let query = raw_input
                .and_then(|i| i.get("pattern").or_else(|| i.get("query")))
                .and_then(|v| v.as_str())
                .map(String::from);
            let path = raw_input
                .and_then(|i| i.get("path").or_else(|| i.get("directory")))
                .and_then(|v| v.as_str())
                .map(String::from);
            vec![ParsedCommand::Search {
                cmd: title.to_string(),
                query,
                path,
            }]
        }

        // Edit, Delete, Move → Command mode (mutating operations)
        Some(acp::ToolKind::Edit | acp::ToolKind::Delete | acp::ToolKind::Move) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Execute → Command mode (shell/terminal operations)
        Some(acp::ToolKind::Execute) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Fetch → Command mode (external data retrieval)
        Some(acp::ToolKind::Fetch) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Think → Command mode (internal reasoning)
        Some(acp::ToolKind::Think) => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }

        // Other or unknown → Command mode (fallback)
        Some(acp::ToolKind::Other) | None => {
            // Try to infer from title as fallback
            classify_tool_by_title(title, raw_input)
        }

        // Catch any future ToolKind variants
        #[allow(unreachable_patterns)]
        _ => {
            vec![ParsedCommand::Unknown {
                cmd: format_tool_call_command(title, raw_input),
            }]
        }
    }
}

/// Fallback classification based on tool title when ToolKind is not available.
///
/// Uses heuristics to detect common tool patterns.
fn classify_tool_by_title(
    title: &str,
    raw_input: Option<&serde_json::Value>,
) -> Vec<ParsedCommand> {
    let title_lower = title.to_lowercase();

    // List/Glob operations → Exploring mode
    if title_lower.contains("list")
        || title_lower.contains("glob")
        || title_lower.contains("ls")
        || title_lower == "find"
        || title_lower.contains("find files")
    {
        let path = raw_input
            .and_then(|i| i.get("path").or_else(|| i.get("directory")))
            .and_then(|v| v.as_str())
            .map(String::from);
        return vec![ParsedCommand::ListFiles {
            cmd: title.to_string(),
            path,
        }];
    }

    // Search/Grep operations → Exploring mode
    if title_lower.contains("search") || title_lower.contains("grep") {
        let query = raw_input
            .and_then(|i| i.get("pattern").or_else(|| i.get("query")))
            .and_then(|v| v.as_str())
            .map(String::from);
        let path = raw_input
            .and_then(|i| i.get("path"))
            .and_then(|v| v.as_str())
            .map(String::from);
        return vec![ParsedCommand::Search {
            cmd: title.to_string(),
            query,
            path,
        }];
    }

    // Read operations → Exploring mode
    if title_lower.contains("read") || title_lower == "file" {
        let path = raw_input
            .and_then(|i| {
                i.get("path")
                    .or_else(|| i.get("file_path"))
                    .or_else(|| i.get("file"))
            })
            .and_then(|v| v.as_str())
            .unwrap_or("file");
        let name = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string());
        return vec![ParsedCommand::Read {
            cmd: title.to_string(),
            name,
            path: std::path::PathBuf::from(path),
        }];
    }

    // Default: Command mode
    vec![ParsedCommand::Unknown {
        cmd: format_tool_call_command(title, raw_input),
    }]
}

// =============================================================================
// Transcript Replay Helpers
// =============================================================================

/// Maximum character length for the transcript summary text.
const TRANSCRIPT_SUMMARY_MAX_CHARS: usize = 20_000;

/// Convert a loaded transcript into a list of `EventMsg` suitable for
/// `SessionConfiguredEvent.initial_messages` (UI replay).
///
/// Only `User` and `Assistant` entries are converted; tool calls, results,
/// patches, and session metadata are skipped since the UI does not need to
/// replay the full tool lifecycle for display purposes.
pub fn transcript_to_replay_events(transcript: &crate::transcript::Transcript) -> Vec<EventMsg> {
    use codex_protocol::protocol::AgentMessageEvent;
    use codex_protocol::protocol::UserMessageEvent;

    transcript
        .entries
        .iter()
        .filter_map(|line| match &line.entry {
            crate::transcript::TranscriptEntry::User(user) => {
                Some(EventMsg::UserMessage(UserMessageEvent {
                    message: user.content.clone(),
                    images: None,
                }))
            }
            crate::transcript::TranscriptEntry::Assistant(assistant) => {
                let text: String = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        ContentBlock::Thinking { .. } => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if text.is_empty() {
                    None
                } else {
                    Some(EventMsg::AgentMessage(AgentMessageEvent { message: text }))
                }
            }
            _ => None,
        })
        .collect()
}

/// Convert a loaded transcript into a human-readable summary string suitable
/// for injecting into the first prompt via `pending_compact_summary`.
///
/// The summary captures user messages, assistant responses, and tool call
/// names so the agent has context about the previous conversation without
/// needing the full tool lifecycle details.
pub fn transcript_to_summary(transcript: &crate::transcript::Transcript) -> String {
    let mut summary = String::new();

    for line in &transcript.entries {
        if summary.len() >= TRANSCRIPT_SUMMARY_MAX_CHARS {
            summary.push_str("\n[...transcript truncated...]");
            break;
        }

        match &line.entry {
            crate::transcript::TranscriptEntry::User(user) => {
                summary.push_str(&format!("User: {}\n", user.content));
            }
            crate::transcript::TranscriptEntry::Assistant(assistant) => {
                let text: String = assistant
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        ContentBlock::Thinking { .. } => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if !text.is_empty() {
                    summary.push_str(&format!("Assistant: {text}\n"));
                }
            }
            crate::transcript::TranscriptEntry::ToolCall(tool) => {
                summary.push_str(&format!("[Tool: {}]\n", tool.name));
            }
            _ => {}
        }
    }

    // Final truncation guard: find the nearest char boundary at or before
    // the limit to avoid panicking on multi-byte UTF-8 (CJK, emoji, etc.).
    if summary.len() > TRANSCRIPT_SUMMARY_MAX_CHARS {
        let mut boundary = TRANSCRIPT_SUMMARY_MAX_CHARS;
        while !summary.is_char_boundary(boundary) {
            boundary -= 1;
        }
        summary.truncate(boundary);
        summary.push_str("\n[...truncated...]");
    }

    summary
}

#[cfg(test)]
mod tests;
