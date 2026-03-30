use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
#[allow(unused_imports)]
use std::time::Duration;
use std::time::Instant;

#[allow(unused_imports)]
use codex_app_server_protocol::AuthMode;
use codex_core::config::Config;
use codex_core::project_doc::DEFAULT_PROJECT_DOC_FILENAME;
use codex_core::protocol::AgentMessageDeltaEvent;
use codex_core::protocol::AgentMessageEvent;
use codex_core::protocol::AgentReasoningDeltaEvent;
use codex_core::protocol::AgentReasoningEvent;
use codex_core::protocol::AgentReasoningRawContentDeltaEvent;
use codex_core::protocol::AgentReasoningRawContentEvent;
use codex_core::protocol::ApplyPatchApprovalRequestEvent;
use codex_core::protocol::BackgroundEventEvent;
use codex_core::protocol::DeprecationNoticeEvent;
use codex_core::protocol::ErrorEvent;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::ExecApprovalRequestEvent;
use codex_core::protocol::ExecCommandBeginEvent;
use codex_core::protocol::ExecCommandEndEvent;
use codex_core::protocol::ExecCommandSource;
use codex_core::protocol::HookOutputEvent;
use codex_core::protocol::HookOutputLevel;
use codex_core::protocol::ListCustomPromptsResponseEvent;
use codex_core::protocol::McpListToolsResponseEvent;
use codex_core::protocol::McpStartupCompleteEvent;
use codex_core::protocol::McpStartupStatus;
use codex_core::protocol::McpStartupUpdateEvent;
use codex_core::protocol::McpToolCallBeginEvent;
use codex_core::protocol::McpToolCallEndEvent;
use codex_core::protocol::Op;
use codex_core::protocol::PatchApplyBeginEvent;
use codex_core::protocol::RateLimitSnapshot;
use codex_core::protocol::StreamErrorEvent;
use codex_core::protocol::TaskCompleteEvent;
use codex_core::protocol::TokenUsage;
use codex_core::protocol::TokenUsageInfo;
use codex_core::protocol::TurnAbortReason;
use codex_core::protocol::TurnDiffEvent;
use codex_core::protocol::UndoCompletedEvent;
use codex_core::protocol::UndoListResultEvent;
use codex_core::protocol::UndoStartedEvent;
use codex_core::protocol::UserMessageEvent;
use codex_core::protocol::ViewImageToolCallEvent;
use codex_core::protocol::WarningEvent;
use codex_core::protocol::WebSearchBeginEvent;
use codex_core::protocol::WebSearchEndEvent;
use codex_protocol::ConversationId;
use codex_protocol::approvals::ElicitationRequestEvent;
use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::user_input::UserInput;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use rand::Rng;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use tracing::debug;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::ApprovalRequest;
use crate::bottom_pane::BottomPane;
use crate::bottom_pane::BottomPaneParams;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::InputResult;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::clipboard_paste::paste_image_to_temp_png;
use crate::diff_render::display_path_for;
use crate::effective_cwd_tracker::EffectiveCwdTracker;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::ExecCell;
use crate::exec_cell::new_active_exec_command;
use crate::get_git_diff::get_git_diff;
use crate::history_cell;
use crate::history_cell::HistoryCell;
use crate::history_cell::McpToolCallCell;
use crate::history_cell::PlainHistoryCell;
use crate::login_handler::AgentLoginSupport;
use crate::login_handler::LoginHandler;
#[allow(unused_imports)]
use crate::login_handler::LoginMethod;
use crate::render::Insets;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::FlexRenderable;
use crate::render::renderable::Renderable;
use crate::render::renderable::RenderableExt;
use crate::render::renderable::RenderableItem;
use crate::session_stats::SessionStats;
use crate::session_stats::extract_skill_from_raw_input;
use crate::session_stats::extract_skill_from_read_path;
use crate::session_stats::extract_skills_from_text;
use crate::session_stats::extract_subagent_from_raw_input;
use crate::slash_command::SlashCommand;
use crate::status::RateLimitSnapshotDisplay;
use crate::text_formatting::truncate_text;
use crate::tui::FrameRequester;
mod interrupts;
use self::interrupts::InterruptManager;
mod pending_exec_cells;
use self::pending_exec_cells::PendingExecCellTracker;
mod agent;
#[cfg(feature = "unstable")]
pub(crate) use self::agent::AcpAgentHandle;
use self::agent::spawn_acp_agent_resume;
use self::agent::spawn_agent;
mod session_header;

mod approvals;
mod constructors;
mod event_handlers;
mod helpers;
mod key_handling;
mod login;
mod pickers;
mod user_input;
use self::session_header::SessionHeader;
use crate::streaming::controller::StreamController;
use chrono::Local;
use codex_common::approval_presets::ApprovalPreset;
use codex_common::approval_presets::approval_mode_label;
use codex_common::approval_presets::builtin_approval_presets;

use codex_core::AuthManager;
#[allow(unused_imports)]
use codex_core::CodexAuth;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::SandboxPolicy;
use codex_core::protocol_config_types::ReasoningEffort as ReasoningEffortConfig;
use codex_file_search::FileMatch;
use codex_protocol::plan_tool::UpdatePlanArgs;

const USER_SHELL_COMMAND_HELP_TITLE: &str = "Prefix a command with ! to run it locally";
const USER_SHELL_COMMAND_HELP_HINT: &str = "Example: !ls";
// Track information about an in-flight exec command.
struct RunningCommand {
    command: Vec<String>,
    parsed_cmd: Vec<ParsedCommand>,
    source: ExecCommandSource,
}

struct UnifiedExecWaitState {
    command_display: String,
}

impl UnifiedExecWaitState {
    fn new(command_display: String) -> Self {
        Self { command_display }
    }

    fn is_duplicate(&self, command_display: &str) -> bool {
        self.command_display == command_display
    }
}

const RATE_LIMIT_WARNING_THRESHOLDS: [f64; 3] = [75.0, 90.0, 95.0];

#[derive(Default)]
struct RateLimitWarningState {
    secondary_index: usize,
    primary_index: usize,
}

impl RateLimitWarningState {
    fn take_warnings(
        &mut self,
        secondary_used_percent: Option<f64>,
        secondary_window_minutes: Option<i64>,
        primary_used_percent: Option<f64>,
        primary_window_minutes: Option<i64>,
    ) -> Vec<String> {
        let reached_secondary_cap =
            matches!(secondary_used_percent, Some(percent) if percent == 100.0);
        let reached_primary_cap = matches!(primary_used_percent, Some(percent) if percent == 100.0);
        if reached_secondary_cap || reached_primary_cap {
            return Vec::new();
        }

        let mut warnings = Vec::new();

        if let Some(secondary_used_percent) = secondary_used_percent {
            let mut highest_secondary: Option<f64> = None;
            while self.secondary_index < RATE_LIMIT_WARNING_THRESHOLDS.len()
                && secondary_used_percent >= RATE_LIMIT_WARNING_THRESHOLDS[self.secondary_index]
            {
                highest_secondary = Some(RATE_LIMIT_WARNING_THRESHOLDS[self.secondary_index]);
                self.secondary_index += 1;
            }
            if let Some(threshold) = highest_secondary {
                let limit_label = secondary_window_minutes
                    .map(get_limits_duration)
                    .unwrap_or_else(|| "weekly".to_string());
                warnings.push(format!(
                    "Heads up, you've used over {threshold:.0}% of your {limit_label} limit. Run /status for a breakdown."
                ));
            }
        }

        if let Some(primary_used_percent) = primary_used_percent {
            let mut highest_primary: Option<f64> = None;
            while self.primary_index < RATE_LIMIT_WARNING_THRESHOLDS.len()
                && primary_used_percent >= RATE_LIMIT_WARNING_THRESHOLDS[self.primary_index]
            {
                highest_primary = Some(RATE_LIMIT_WARNING_THRESHOLDS[self.primary_index]);
                self.primary_index += 1;
            }
            if let Some(threshold) = highest_primary {
                let limit_label = primary_window_minutes
                    .map(get_limits_duration)
                    .unwrap_or_else(|| "5h".to_string());
                warnings.push(format!(
                    "Heads up, you've used over {threshold:.0}% of your {limit_label} limit. Run /status for a breakdown."
                ));
            }
        }

        warnings
    }
}

pub(crate) fn get_limits_duration(windows_minutes: i64) -> String {
    const MINUTES_PER_HOUR: i64 = 60;
    const MINUTES_PER_DAY: i64 = 24 * MINUTES_PER_HOUR;
    const MINUTES_PER_WEEK: i64 = 7 * MINUTES_PER_DAY;
    const MINUTES_PER_MONTH: i64 = 30 * MINUTES_PER_DAY;
    const ROUNDING_BIAS_MINUTES: i64 = 3;

    let windows_minutes = windows_minutes.max(0);

    if windows_minutes <= MINUTES_PER_DAY.saturating_add(ROUNDING_BIAS_MINUTES) {
        let adjusted = windows_minutes.saturating_add(ROUNDING_BIAS_MINUTES);
        let hours = std::cmp::max(1, adjusted / MINUTES_PER_HOUR);
        format!("{hours}h")
    } else if windows_minutes <= MINUTES_PER_WEEK.saturating_add(ROUNDING_BIAS_MINUTES) {
        "weekly".to_string()
    } else if windows_minutes <= MINUTES_PER_MONTH.saturating_add(ROUNDING_BIAS_MINUTES) {
        "monthly".to_string()
    } else {
        "annual".to_string()
    }
}

/// Strip ANSI escape codes from a string.
/// Uses a simple state machine approach to handle common escape sequences.
#[cfg(feature = "login")]
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip until we hit a letter (the terminator)
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else if chars.peek() == Some(&']') {
                // OSC sequence (Operating System Command)
                chars.next(); // consume ']'
                // Skip until BEL (\x07) or ST (ESC \)
                while let Some(&next) = chars.peek() {
                    if next == '\x07' {
                        chars.next();
                        break;
                    } else if next == '\x1b' {
                        chars.next();
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                    chars.next();
                }
            }
        } else if c == '\r' {
            // Skip carriage return (handle Windows line endings)
            continue;
        } else {
            result.push(c);
        }
    }

    result
}

/// Common initialization parameters shared by all `ChatWidget` constructors.
pub(crate) struct ChatWidgetInit {
    pub(crate) config: Config,
    pub(crate) frame_requester: FrameRequester,
    pub(crate) app_event_tx: AppEventSender,
    pub(crate) initial_prompt: Option<String>,
    pub(crate) initial_images: Vec<PathBuf>,
    pub(crate) enhanced_keys_supported: bool,
    pub(crate) auth_manager: Arc<AuthManager>,
    pub(crate) vertical_footer: bool,
    /// Expected agent name for this widget. When set, events from other agents
    /// (e.g., from a previous agent) are ignored until SessionConfigured arrives
    /// with a matching agent. This prevents race conditions when switching agents.
    pub(crate) expected_agent: Option<String>,
    /// When true, skip spawning the agent in `new()`. The caller must later
    /// call `spawn_deferred_agent()` once pre-session setup (e.g., skillset
    /// switch) is complete.
    pub(crate) deferred_spawn: bool,
    /// Optional conversation context to inject into the first prompt.
    /// Used by `/fork` to pass prior conversation history to the new session.
    pub(crate) fork_context: Option<String>,
}

/// Controls the pinned plan drawer visibility and display mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanDrawerMode {
    /// Drawer is hidden; plan updates go to history cells.
    Off,
    /// Drawer shows a single-line progress summary.
    Collapsed,
    /// Drawer shows the full plan checklist.
    Expanded,
}

pub(crate) struct ChatWidget {
    app_event_tx: AppEventSender,
    codex_op_tx: UnboundedSender<Op>,
    bottom_pane: BottomPane,
    active_cell: Option<Box<dyn HistoryCell>>,
    config: Config,
    auth_manager: Arc<AuthManager>,
    session_header: SessionHeader,
    initial_user_message: Option<UserMessage>,
    token_info: Option<TokenUsageInfo>,
    rate_limit_snapshot: Option<RateLimitSnapshotDisplay>,
    rate_limit_warnings: RateLimitWarningState,
    rate_limit_poller: Option<JoinHandle<()>>,
    // Stream lifecycle controller
    stream_controller: Option<StreamController>,
    running_commands: HashMap<String, RunningCommand>,
    suppressed_exec_calls: HashSet<String>,
    completed_client_tool_calls: HashSet<String>,
    last_unified_wait: Option<UnifiedExecWaitState>,
    task_complete_pending: bool,
    mcp_startup_status: Option<HashMap<String, McpStartupStatus>>,
    mcp_auth_statuses: HashMap<String, codex_core::protocol::McpAuthStatus>,
    // Queue of interruptive UI events deferred during an active write cycle
    interrupts: InterruptManager,
    // Accumulates the current reasoning block text to extract a header
    reasoning_buffer: String,
    // Accumulates full reasoning content for transcript-only recording
    full_reasoning_buffer: String,
    // Current status header shown in the status indicator.
    current_status_header: String,
    // Previous status header to restore after a transient stream retry.
    retry_status_header: Option<String>,
    conversation_id: Option<ConversationId>,
    frame_requester: FrameRequester,
    // Whether to include the initial welcome banner on session configured
    show_welcome_banner: bool,
    // When resuming an existing session (selected via resume picker), avoid an
    // immediate redraw on SessionConfigured to prevent a gratuitous UI flicker.
    suppress_session_configured_redraw: bool,
    // User messages queued while a turn is in progress
    queued_user_messages: VecDeque<UserMessage>,
    // Pending notification to show when unfocused on next Draw
    pending_notification: Option<Notification>,
    // Whether to add a final message separator after the last message
    needs_final_message_separator: bool,

    last_rendered_width: std::cell::Cell<Option<usize>>,
    // Current session rollout path (if known)
    current_rollout_path: Option<PathBuf>,
    // Tracks incomplete ExecCells that were flushed before completion.
    pending_exec_cells: PendingExecCellTracker,
    // Tracks the effective CWD based on tool call locations for footer updates.
    effective_cwd_tracker: EffectiveCwdTracker,
    // Pending agent selection for next prompt submission
    pending_agent: Option<PendingAgentInfo>,
    // Expected agent name for agent switch synchronization.
    // When set, events are ignored until SessionConfigured arrives with this agent.
    expected_agent: Option<String>,
    // Whether SessionConfigured has been received for this widget.
    // Used with expected_agent to filter events from previous agents.
    session_configured_received: bool,
    // ACP agent handle for model switching (only present in ACP mode)
    #[cfg(feature = "unstable")]
    acp_handle: Option<AcpAgentHandle>,
    // Session statistics tracking
    session_stats: SessionStats,
    // Login handler for /login command
    login_handler: Option<LoginHandler>,
    // The first user prompt text, preserved for /first-prompt command
    first_prompt_text: Option<String>,
    // Loop mode state: remaining iterations (None = not looping)
    loop_remaining: Option<i32>,
    // Loop mode state: total iterations configured
    loop_total: Option<i32>,
    // Ephemeral per-session override for loop_count (set via /config menu).
    // Outer Option: whether overridden; inner Option<i32>: the value.
    #[cfg(feature = "nori-config")]
    loop_count_override: Option<Option<i32>>,
    // Gate: set when AgentMessage is received, cleared on next TaskStarted.
    // While true, late-arriving tool events are silently discarded.
    turn_finished: bool,
    /// Whether and how plan updates are rendered in a pinned drawer instead of
    /// history cells.
    plan_drawer_mode: PlanDrawerMode,
    /// Latest plan state, always updated on every plan event. Used by the
    /// pinned plan drawer when enabled; retained when disabled so toggling
    /// the drawer on shows the most recent plan immediately.
    pinned_plan: Option<UpdatePlanArgs>,

    // Terminal title state: baseline instant for computing spinner frame index.
    terminal_title_animation_origin: Instant,
    // Terminal title state: cache to avoid redundant OSC writes.
    last_terminal_title: Option<String>,
}

/// Information about a pending agent switch in ChatWidget.
#[derive(Debug, Clone)]
pub(crate) struct PendingAgentInfo {
    pub agent_name: String,
    pub display_name: String,
}

struct UserMessage {
    text: String,
    image_paths: Vec<PathBuf>,
}

impl From<String> for UserMessage {
    fn from(text: String) -> Self {
        Self {
            text,
            image_paths: Vec::new(),
        }
    }
}

impl From<&str> for UserMessage {
    fn from(text: &str) -> Self {
        Self {
            text: text.to_string(),
            image_paths: Vec::new(),
        }
    }
}

fn create_initial_user_message(text: String, image_paths: Vec<PathBuf>) -> Option<UserMessage> {
    if text.is_empty() && image_paths.is_empty() {
        None
    } else {
        Some(UserMessage { text, image_paths })
    }
}

impl ChatWidget {}

impl Drop for ChatWidget {
    fn drop(&mut self) {
        self.stop_rate_limit_poller();
        if let Err(err) = self.clear_managed_terminal_title() {
            tracing::debug!(error = %err, "failed to clear terminal title on drop");
        }
    }
}

impl Renderable for ChatWidget {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.as_renderable().render(area, buf);
        self.last_rendered_width.set(Some(area.width as usize));
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.as_renderable().desired_height(width)
    }

    fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.as_renderable().cursor_pos(area)
    }
}

enum Notification {
    AgentTurnComplete { response: String },
    ExecApprovalRequested { command: String },
    EditApprovalRequested { cwd: PathBuf, changes: Vec<PathBuf> },
    ElicitationRequested { server_name: String },
}

impl Notification {
    fn display(&self) -> String {
        match self {
            Notification::AgentTurnComplete { response } => {
                Notification::agent_turn_preview(response)
                    .unwrap_or_else(|| "Agent turn complete".to_string())
            }
            Notification::ExecApprovalRequested { command } => {
                format!("Approval requested: {}", truncate_text(command, 30))
            }
            Notification::EditApprovalRequested { cwd, changes } => {
                format!(
                    "Nori wants to edit {}",
                    if changes.len() == 1 {
                        #[allow(clippy::unwrap_used)]
                        display_path_for(changes.first().unwrap(), cwd)
                    } else {
                        format!("{} files", changes.len())
                    }
                )
            }
            Notification::ElicitationRequested { server_name } => {
                format!("Approval requested by {server_name}")
            }
        }
    }

    fn agent_turn_preview(response: &str) -> Option<String> {
        let mut normalized = String::new();
        for part in response.split_whitespace() {
            if !normalized.is_empty() {
                normalized.push(' ');
            }
            normalized.push_str(part);
        }
        let trimmed = normalized.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(truncate_text(trimmed, AGENT_NOTIFICATION_PREVIEW_GRAPHEMES))
        }
    }
}

const AGENT_NOTIFICATION_PREVIEW_GRAPHEMES: usize = 200;

const EXAMPLE_PROMPTS: [&str; 6] = [
    "Explain this codebase",
    "Summarize recent commits",
    "Implement {feature}",
    "Find and fix a bug in @filename",
    "Write tests for @filename",
    "Improve documentation in @filename",
];

// Extract the first bold (Markdown) element in the form **...** from `s`.
// Returns the inner text if found; otherwise `None`.
/// Truncate a string for logging purposes.
#[allow(dead_code)]
fn truncate_for_log(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.replace('\n', "\\n")
    } else {
        format!("{}...", s[..max_len].replace('\n', "\\n"))
    }
}

fn extract_first_bold(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'*' && bytes[i + 1] == b'*' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() {
                if bytes[j] == b'*' && bytes[j + 1] == b'*' {
                    // Found closing **
                    let inner = &s[start..j];
                    let trimmed = inner.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    } else {
                        return None;
                    }
                }
                j += 1;
            }
            // No closing; stop searching (wait for more deltas)
            return None;
        }
        i += 1;
    }
    None
}

#[cfg(test)]
pub(crate) mod tests;
