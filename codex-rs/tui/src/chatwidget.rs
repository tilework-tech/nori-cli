use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
#[allow(unused_imports)]
use std::time::Duration;

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
use self::agent::spawn_agent_from_existing;
mod session_header;
use self::session_header::SessionHeader;
use crate::streaming::controller::StreamController;
use chrono::Local;
use codex_common::approval_presets::ApprovalPreset;
use codex_common::approval_presets::approval_mode_label;
use codex_common::approval_presets::builtin_approval_presets;
use codex_common::model_presets::ModelPreset;
use codex_common::model_presets::builtin_model_presets;
use codex_core::AuthManager;
#[allow(unused_imports)]
use codex_core::CodexAuth;
use codex_core::ConversationManager;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::SandboxPolicy;
use codex_core::protocol_config_types::ReasoningEffort as ReasoningEffortConfig;
use codex_file_search::FileMatch;
use codex_protocol::plan_tool::UpdatePlanArgs;
use strum::IntoEnumIterator;

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
const NUDGE_MODEL_SLUG: &str = "gpt-5.1-codex-mini";
const RATE_LIMIT_SWITCH_PROMPT_THRESHOLD: f64 = 90.0;

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
    /// Expected model name for this widget. When set, events from other models
    /// (e.g., from a previous agent) are ignored until SessionConfigured arrives
    /// with a matching model. This prevents race conditions when switching agents.
    pub(crate) expected_model: Option<String>,
}

#[derive(Default)]
enum RateLimitSwitchPromptState {
    #[default]
    Idle,
    Pending,
    Shown,
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
    rate_limit_switch_prompt: RateLimitSwitchPromptState,
    rate_limit_poller: Option<JoinHandle<()>>,
    // Stream lifecycle controller
    stream_controller: Option<StreamController>,
    running_commands: HashMap<String, RunningCommand>,
    suppressed_exec_calls: HashSet<String>,
    last_unified_wait: Option<UnifiedExecWaitState>,
    task_complete_pending: bool,
    mcp_startup_status: Option<HashMap<String, McpStartupStatus>>,
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
    // Expected model name for agent switch synchronization.
    // When set, events are ignored until SessionConfigured arrives with this model.
    expected_model: Option<String>,
    // Whether SessionConfigured has been received for this widget.
    // Used with expected_model to filter events from previous agents.
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
}

/// Information about a pending agent switch in ChatWidget.
#[derive(Debug, Clone)]
pub(crate) struct PendingAgentInfo {
    pub model_name: String,
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

impl ChatWidget {
    fn flush_answer_stream_with_separator(&mut self) {
        if let Some(mut controller) = self.stream_controller.take()
            && let Some(cell) = controller.finalize()
        {
            self.add_boxed_history(cell);
        }
    }

    fn set_status_header(&mut self, header: String) {
        self.current_status_header = header.clone();
        self.bottom_pane.update_status_header(header);
    }

    // --- Small event handlers ---
    fn on_session_configured(&mut self, event: codex_core::protocol::SessionConfiguredEvent) {
        // Mark that we've received SessionConfigured - this unlocks event processing
        // when expected_model is set (during agent switching)
        self.session_configured_received = true;

        // Clear the "Connecting to [Agent]" status indicator shown during agent startup
        self.bottom_pane.hide_status_indicator();

        // Update footer with current approval mode
        self.update_approval_mode_label();

        self.bottom_pane
            .set_history_metadata(event.history_log_id, event.history_entry_count);
        self.conversation_id = Some(event.session_id);
        self.current_rollout_path = Some(event.rollout_path.clone());
        let initial_messages = event.initial_messages.clone();
        let model_for_header = event.model.clone();
        self.session_header.set_model(&model_for_header);
        self.add_to_history(history_cell::new_session_info(
            &self.config,
            event,
            self.show_welcome_banner,
        ));
        if let Some(messages) = initial_messages {
            self.replay_initial_messages(messages);
        }
        // Ask codex-core to enumerate custom prompts for this session.
        self.submit_op(Op::ListCustomPrompts);
        if let Some(user_message) = self.initial_user_message.take() {
            self.submit_user_message(user_message);
        }
        if !self.suppress_session_configured_redraw {
            self.request_redraw();
        }
    }

    fn on_agent_message(&mut self, message: String) {
        // Track assistant message for session statistics
        self.session_stats.record_assistant_message();

        // If we have a stream_controller, then the final agent message is redundant and will be a
        // duplicate of what has already been streamed.
        if self.stream_controller.is_none() {
            self.handle_streaming_delta(message);
        }
        self.flush_answer_stream_with_separator();
        self.handle_stream_finished();
        self.request_redraw();
    }

    fn on_agent_message_delta(&mut self, delta: String) {
        self.handle_streaming_delta(delta);
    }

    fn on_agent_reasoning_delta(&mut self, delta: String) {
        // For reasoning deltas, do not stream to history. Accumulate the
        // current reasoning block and extract the first bold element
        // (between **/**) as the chunk header. Show this header as status.
        self.reasoning_buffer.push_str(&delta);

        if let Some(header) = extract_first_bold(&self.reasoning_buffer) {
            // Update the shimmer header to the extracted reasoning chunk header.
            self.set_status_header(header);
        } else {
            // Fallback while we don't yet have a bold header: leave existing header as-is.
        }
        self.request_redraw();
    }

    fn on_agent_reasoning_final(&mut self) {
        // At the end of a reasoning block, record transcript-only content.
        self.full_reasoning_buffer.push_str(&self.reasoning_buffer);
        if !self.full_reasoning_buffer.is_empty() {
            let cell = history_cell::new_reasoning_summary_block(
                self.full_reasoning_buffer.clone(),
                &self.config,
            );
            self.add_boxed_history(cell);
        }
        self.reasoning_buffer.clear();
        self.full_reasoning_buffer.clear();
        self.request_redraw();
    }

    fn on_reasoning_section_break(&mut self) {
        // Start a new reasoning block for header extraction and accumulate transcript.
        self.full_reasoning_buffer.push_str(&self.reasoning_buffer);
        self.full_reasoning_buffer.push_str("\n\n");
        self.reasoning_buffer.clear();
    }

    // Raw reasoning uses the same flow as summarized reasoning

    fn on_task_started(&mut self) {
        self.bottom_pane.clear_ctrl_c_quit_hint();
        self.bottom_pane.set_task_running(true);
        self.retry_status_header = None;
        self.bottom_pane.set_interrupt_hint_visible(true);
        self.set_status_header(String::from("Working"));
        self.full_reasoning_buffer.clear();
        self.reasoning_buffer.clear();
        self.request_redraw();
    }

    fn on_task_complete(&mut self, last_agent_message: Option<String>) {
        // If a stream is currently active, finalize it.
        self.flush_answer_stream_with_separator();
        // Process any deferred completion events (ExecEnd, McpEnd, PatchEnd) so
        // in-progress tool cells transition to their finished state ("Running" →
        // "Ran"). Discard begin events that would create new cells below the
        // agent's final message.
        let mut mgr = std::mem::take(&mut self.interrupts);
        let discarded = mgr.flush_completions_and_clear(self);
        self.interrupts = mgr;
        if discarded > 0 {
            debug!("on_task_complete: discarded {discarded} deferred begin/other interrupt events");
        }

        // Drain any pending ExecCells that weren't completed (e.g., due to interruption).
        self.pending_exec_cells.drain_failed();

        // Mark task stopped and request redraw now that all content is in history.
        self.bottom_pane.set_task_running(false);
        self.running_commands.clear();
        self.suppressed_exec_calls.clear();
        self.last_unified_wait = None;
        self.request_redraw();

        // Refresh system info (including git branch) on task completion.
        // This catches any branch changes that occurred during the agent's turn.
        self.app_event_tx
            .send(AppEvent::RefreshSystemInfoForDirectory {
                dir: self.config.cwd.clone(),
                model: Some(self.config.model.clone()),
            });

        // If there is a queued user message, send exactly one now to begin the next turn.
        self.maybe_send_next_queued_input();
        // Emit a notification when the turn completes (suppressed if focused).
        self.notify(Notification::AgentTurnComplete {
            response: last_agent_message.unwrap_or_default(),
        });

        self.maybe_show_pending_rate_limit_prompt();

        // Loop mode: if iterations remain, fire the next iteration.
        #[cfg(feature = "nori-config")]
        if let Some(remaining) = self.loop_remaining
            && remaining > 0
            && let Some(prompt) = self.first_prompt_text.clone()
        {
            let total = self.loop_total.unwrap_or(0);
            self.app_event_tx.send(AppEvent::LoopIteration {
                prompt,
                remaining: remaining - 1,
                total,
            });
        }
    }

    pub(crate) fn set_token_info(&mut self, info: Option<TokenUsageInfo>) {
        match info {
            Some(info) => self.apply_token_info(info),
            None => {
                self.bottom_pane.set_context_window_percent(None);
                self.token_info = None;
            }
        }
    }

    fn apply_token_info(&mut self, info: TokenUsageInfo) {
        let percent = self.context_used_percent(&info);
        self.bottom_pane.set_context_window_percent(percent);
        self.token_info = Some(info);
    }

    fn context_used_percent(&self, info: &TokenUsageInfo) -> Option<i64> {
        info.model_context_window
            .or(self.config.model_context_window)
            .map(|window| {
                let remaining = info
                    .last_token_usage
                    .percent_of_context_window_remaining(window);
                (100 - remaining).clamp(0, 100)
            })
    }

    pub(crate) fn on_rate_limit_snapshot(&mut self, snapshot: Option<RateLimitSnapshot>) {
        if let Some(snapshot) = snapshot {
            let warnings = self.rate_limit_warnings.take_warnings(
                snapshot
                    .secondary
                    .as_ref()
                    .map(|window| window.used_percent),
                snapshot
                    .secondary
                    .as_ref()
                    .and_then(|window| window.window_minutes),
                snapshot.primary.as_ref().map(|window| window.used_percent),
                snapshot
                    .primary
                    .as_ref()
                    .and_then(|window| window.window_minutes),
            );

            let high_usage = snapshot
                .secondary
                .as_ref()
                .map(|w| w.used_percent >= RATE_LIMIT_SWITCH_PROMPT_THRESHOLD)
                .unwrap_or(false)
                || snapshot
                    .primary
                    .as_ref()
                    .map(|w| w.used_percent >= RATE_LIMIT_SWITCH_PROMPT_THRESHOLD)
                    .unwrap_or(false);

            if high_usage
                && !self.rate_limit_switch_prompt_hidden()
                && self.config.model != NUDGE_MODEL_SLUG
                && !matches!(
                    self.rate_limit_switch_prompt,
                    RateLimitSwitchPromptState::Shown
                )
            {
                self.rate_limit_switch_prompt = RateLimitSwitchPromptState::Pending;
            }

            let display = crate::status::rate_limit_snapshot_display(&snapshot, Local::now());
            self.rate_limit_snapshot = Some(display);

            if !warnings.is_empty() {
                for warning in warnings {
                    self.add_to_history(history_cell::new_warning_event(warning));
                }
                self.request_redraw();
            }
        } else {
            self.rate_limit_snapshot = None;
        }
    }
    /// Finalize any active exec as failed and stop/clear running UI state.
    fn finalize_turn(&mut self) {
        // Ensure any spinner is replaced by a red ✗ and flushed into history.
        self.finalize_active_cell_as_failed();
        // Reset running state and clear streaming buffers.
        self.bottom_pane.set_task_running(false);
        self.running_commands.clear();
        self.suppressed_exec_calls.clear();
        self.last_unified_wait = None;
        self.stream_controller = None;
        self.maybe_show_pending_rate_limit_prompt();
    }

    fn on_error(&mut self, message: String) {
        self.finalize_turn();
        self.cancel_loop();
        self.add_to_history(history_cell::new_error_event(message));
        self.request_redraw();

        // After an error ends the turn, try sending the next queued input.
        self.maybe_send_next_queued_input();
    }

    fn on_warning(&mut self, message: impl Into<String>) {
        self.add_to_history(history_cell::new_warning_event(message.into()));
        self.request_redraw();
    }

    fn on_mcp_startup_update(&mut self, ev: McpStartupUpdateEvent) {
        let mut status = self.mcp_startup_status.take().unwrap_or_default();
        if let McpStartupStatus::Failed { error } = &ev.status {
            self.on_warning(error);
        }
        status.insert(ev.server, ev.status);
        self.mcp_startup_status = Some(status);
        self.bottom_pane.set_task_running(true);
        if let Some(current) = &self.mcp_startup_status {
            let total = current.len();
            let mut starting: Vec<_> = current
                .iter()
                .filter_map(|(name, state)| {
                    if matches!(state, McpStartupStatus::Starting) {
                        Some(name)
                    } else {
                        None
                    }
                })
                .collect();
            starting.sort();
            if let Some(first) = starting.first() {
                let completed = total.saturating_sub(starting.len());
                let max_to_show = 3;
                let mut to_show: Vec<String> = starting
                    .iter()
                    .take(max_to_show)
                    .map(ToString::to_string)
                    .collect();
                if starting.len() > max_to_show {
                    to_show.push("…".to_string());
                }
                let header = if total > 1 {
                    format!(
                        "Starting MCP servers ({completed}/{total}): {}",
                        to_show.join(", ")
                    )
                } else {
                    format!("Booting MCP server: {first}")
                };
                self.set_status_header(header);
            }
        }
        self.request_redraw();
    }

    fn on_mcp_startup_complete(&mut self, ev: McpStartupCompleteEvent) {
        let mut parts = Vec::new();
        if !ev.failed.is_empty() {
            let failed_servers: Vec<_> = ev.failed.iter().map(|f| f.server.clone()).collect();
            parts.push(format!("failed: {}", failed_servers.join(", ")));
        }
        if !ev.cancelled.is_empty() {
            self.on_warning(format!(
                "MCP startup interrupted. The following servers were not initialized: {}",
                ev.cancelled.join(", ")
            ));
        }
        if !parts.is_empty() {
            self.on_warning(format!("MCP startup incomplete ({})", parts.join("; ")));
        }

        self.mcp_startup_status = None;
        self.bottom_pane.set_task_running(false);
        self.maybe_send_next_queued_input();
        self.request_redraw();
    }

    /// Handle a turn aborted due to user interrupt (Esc).
    /// When there are queued user messages, restore them into the composer
    /// separated by newlines rather than auto‑submitting the next one.
    fn on_interrupted_turn(&mut self, _reason: TurnAbortReason) {
        // Finalize, log a gentle prompt, and clear running state.
        self.finalize_turn();
        self.cancel_loop();

        self.add_to_history(history_cell::new_error_event(
            "Conversation interrupted - tell the model what to do differently. Something went wrong? Report the issue at https://github.com/tilework-tech/nori-cli/issues".to_owned(),
        ));

        // If any messages were queued during the task, restore them into the composer.
        if !self.queued_user_messages.is_empty() {
            let queued_text = self
                .queued_user_messages
                .iter()
                .map(|m| m.text.clone())
                .collect::<Vec<_>>()
                .join("\n");
            let existing_text = self.bottom_pane.composer_text();
            let combined = if existing_text.is_empty() {
                queued_text
            } else if queued_text.is_empty() {
                existing_text
            } else {
                format!("{queued_text}\n{existing_text}")
            };
            self.bottom_pane.set_composer_text(combined);
            // Clear the queue and update the status indicator list.
            self.queued_user_messages.clear();
            self.refresh_queued_user_messages();
        }

        self.request_redraw();
    }

    fn on_plan_update(&mut self, update: UpdatePlanArgs) {
        self.add_to_history(history_cell::new_plan_update(update));
    }

    fn on_exec_approval_request(&mut self, id: String, ev: ExecApprovalRequestEvent) {
        // Approval requests must be handled immediately, not deferred. In ACP mode,
        // the agent subprocess is blocked waiting for the user's approval decision.
        // If we defer the approval popup, we create a deadlock: the agent waits for
        // approval, but TaskComplete (which would flush the queue) won't arrive until
        // the agent finishes, which won't happen until approval is granted.
        self.handle_exec_approval_now(id, ev);
    }

    fn on_apply_patch_approval_request(&mut self, id: String, ev: ApplyPatchApprovalRequestEvent) {
        // Same as on_exec_approval_request: handle immediately to avoid deadlock.
        self.handle_apply_patch_approval_now(id, ev);
    }

    fn on_elicitation_request(&mut self, ev: ElicitationRequestEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(
            |q| q.push_elicitation(ev),
            |s| s.handle_elicitation_request_now(ev2),
        );
    }

    fn on_exec_command_begin(&mut self, ev: ExecCommandBeginEvent) {
        self.flush_answer_stream_with_separator();
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_exec_begin(ev), |s| s.handle_exec_begin_now(ev2));
    }

    fn on_exec_command_output_delta(
        &mut self,
        _ev: codex_core::protocol::ExecCommandOutputDeltaEvent,
    ) {
        // TODO: Handle streaming exec output if/when implemented
    }

    fn on_patch_apply_begin(&mut self, event: PatchApplyBeginEvent) {
        // Track Edit tool call for session statistics
        self.session_stats.record_tool_call("Edit");

        // Observe directories from file paths to potentially update footer git info.
        self.observe_directories_from_changes(&event.changes);

        self.add_to_history(history_cell::new_patch_event(
            event.changes,
            &self.config.cwd,
        ));
    }

    fn on_view_image_tool_call(&mut self, event: ViewImageToolCallEvent) {
        // Track ViewImage tool call for session statistics
        self.session_stats.record_tool_call("ViewImage");

        self.flush_answer_stream_with_separator();
        self.add_to_history(history_cell::new_view_image_tool_call(
            event.path,
            &self.config.cwd,
        ));
        self.request_redraw();
    }

    fn on_patch_apply_end(&mut self, event: codex_core::protocol::PatchApplyEndEvent) {
        let ev2 = event.clone();
        self.defer_or_handle(
            |q| q.push_patch_end(event),
            |s| s.handle_patch_apply_end_now(ev2),
        );
    }

    fn on_exec_command_end(&mut self, ev: ExecCommandEndEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_exec_end(ev), |s| s.handle_exec_end_now(ev2));
    }

    fn on_mcp_tool_call_begin(&mut self, ev: McpToolCallBeginEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_mcp_begin(ev), |s| s.handle_mcp_begin_now(ev2));
    }

    fn on_mcp_tool_call_end(&mut self, ev: McpToolCallEndEvent) {
        let ev2 = ev.clone();
        self.defer_or_handle(|q| q.push_mcp_end(ev), |s| s.handle_mcp_end_now(ev2));
    }

    fn on_web_search_begin(&mut self, _ev: WebSearchBeginEvent) {
        self.flush_answer_stream_with_separator();
    }

    fn on_web_search_end(&mut self, ev: WebSearchEndEvent) {
        // Track WebSearch tool call for session statistics
        self.session_stats.record_tool_call("WebSearch");

        self.flush_answer_stream_with_separator();
        self.add_to_history(history_cell::new_web_search_call(format!(
            "Searched: {}",
            ev.query
        )));
    }

    fn on_get_history_entry_response(
        &mut self,
        event: codex_core::protocol::GetHistoryEntryResponseEvent,
    ) {
        let codex_core::protocol::GetHistoryEntryResponseEvent {
            offset,
            log_id,
            entry,
        } = event;
        self.bottom_pane
            .on_history_entry_response(log_id, offset, entry.map(|e| e.text));
    }

    fn on_shutdown_complete(&mut self) {
        self.request_exit();
    }

    fn on_turn_diff(&mut self, unified_diff: String) {
        debug!("TurnDiffEvent: {unified_diff}");
    }

    fn on_deprecation_notice(&mut self, event: DeprecationNoticeEvent) {
        let DeprecationNoticeEvent { summary, details } = event;
        self.add_to_history(history_cell::new_deprecation_notice(summary, details));
        self.request_redraw();
    }

    fn on_background_event(&mut self, message: String) {
        debug!("BackgroundEvent: {message}");
        self.bottom_pane.ensure_status_indicator();
        self.bottom_pane.set_interrupt_hint_visible(true);
        self.set_status_header(message);
    }

    fn on_prompt_summary(&mut self, summary: String) {
        self.bottom_pane.set_prompt_summary(Some(summary));
    }

    fn on_undo_started(&mut self, event: UndoStartedEvent) {
        self.bottom_pane.ensure_status_indicator();
        self.bottom_pane.set_interrupt_hint_visible(false);
        let message = event
            .message
            .unwrap_or_else(|| "Undo in progress...".to_string());
        self.set_status_header(message);
    }

    fn on_undo_completed(&mut self, event: UndoCompletedEvent) {
        let UndoCompletedEvent { success, message } = event;
        self.bottom_pane.hide_status_indicator();
        let message = message.unwrap_or_else(|| {
            if success {
                "Undo completed successfully.".to_string()
            } else {
                "Undo failed.".to_string()
            }
        });
        if success {
            self.add_info_message(message, None);
        } else {
            self.add_error_message(message);
        }
    }

    fn on_undo_list_result(&mut self, event: UndoListResultEvent) {
        if event.snapshots.is_empty() {
            self.add_info_message("No undo snapshots available.".to_string(), None);
            return;
        }

        let items: Vec<SelectionItem> = event
            .snapshots
            .into_iter()
            .map(|snap| {
                let index = snap.index;
                let label = truncate_text(&snap.label, 60);
                let name = format!("[{}] {label}", snap.short_id);
                let tx = self.app_event_tx.clone();
                SelectionItem {
                    name,
                    display_shortcut: None,
                    description: None,
                    selected_description: None,
                    is_current: false,
                    actions: vec![Box::new(move |_| {
                        tx.send(AppEvent::CodexOp(Op::UndoTo { index }));
                    })],
                    dismiss_on_select: true,
                    search_value: None,
                }
            })
            .collect();

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Undo to snapshot".to_string()),
            subtitle: None,
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(()),
            is_searchable: false,
            ..Default::default()
        });
        self.request_redraw();
    }

    fn on_stream_error(&mut self, message: String) {
        if self.retry_status_header.is_none() {
            self.retry_status_header = Some(self.current_status_header.clone());
        }
        self.set_status_header(message);
    }

    /// Periodic tick to commit at most one queued line to history with a small delay,
    /// animating the output.
    pub(crate) fn on_commit_tick(&mut self) {
        if let Some(controller) = self.stream_controller.as_mut() {
            let (cell, is_idle) = controller.on_commit_tick();
            if let Some(cell) = cell {
                // NOTE: Do NOT hide the status indicator here. The "Working (Xs)"
                // message should remain visible until the conversational turn fully
                // completes (when TaskComplete event arrives and set_task_running(false)
                // is called). Hiding it during streaming commits causes the indicator
                // to disappear prematurely while the agent is still processing.
                self.add_boxed_history(cell);
            }
            if is_idle {
                self.app_event_tx.send(AppEvent::StopCommitAnimation);
            }
        }
    }

    fn flush_interrupt_queue(&mut self) {
        let mut mgr = std::mem::take(&mut self.interrupts);
        mgr.flush_all(self);
        self.interrupts = mgr;
    }

    #[inline]
    fn defer_or_handle(
        &mut self,
        push: impl FnOnce(&mut InterruptManager),
        handle: impl FnOnce(&mut Self),
    ) {
        // Preserve deterministic FIFO across queued interrupts: once anything
        // is queued due to an active write cycle, continue queueing until the
        // queue is flushed to avoid reordering (e.g., ExecEnd before ExecBegin).
        if self.stream_controller.is_some() || !self.interrupts.is_empty() {
            push(&mut self.interrupts);
        } else {
            handle(self);
        }
    }

    fn handle_stream_finished(&mut self) {
        if self.task_complete_pending {
            self.bottom_pane.hide_status_indicator();
            self.task_complete_pending = false;
        }
        // A completed stream indicates non-exec content was just inserted.
        self.flush_interrupt_queue();
    }

    #[inline]
    fn handle_streaming_delta(&mut self, delta: String) {
        // Before streaming agent content, flush any active exec cell group.
        // EXCEPT: Don't flush incomplete ExecCells - they should remain visible in
        // active_cell during streaming. Streaming content goes to history (scrollback),
        // while active_cell renders separately at the bottom. Flushing incomplete
        // ExecCells would move them to pending_exec_cells, making them invisible
        // until task completion.
        let should_flush = self
            .active_cell
            .as_ref()
            .map(|cell| {
                cell.as_any()
                    .downcast_ref::<ExecCell>()
                    .map(|exec| !exec.is_active())
                    .unwrap_or(true)
            })
            .unwrap_or(true);

        if should_flush {
            self.flush_active_cell();
        }

        if self.stream_controller.is_none() {
            if self.needs_final_message_separator {
                let elapsed_seconds = self
                    .bottom_pane
                    .status_widget()
                    .map(super::status_indicator_widget::StatusIndicatorWidget::elapsed_seconds);
                self.add_to_history(history_cell::FinalMessageSeparator::new(elapsed_seconds));
                self.needs_final_message_separator = false;
            }
            self.stream_controller = Some(StreamController::new(
                self.last_rendered_width.get().map(|w| w.saturating_sub(2)),
            ));
        }
        if let Some(controller) = self.stream_controller.as_mut()
            && controller.push(&delta)
        {
            self.app_event_tx.send(AppEvent::StartCommitAnimation);
        }
        self.request_redraw();
    }

    pub(crate) fn handle_exec_end_now(&mut self, ev: ExecCommandEndEvent) {
        let running = self.running_commands.remove(&ev.call_id);
        if self.suppressed_exec_calls.remove(&ev.call_id) {
            return;
        }
        let (command, parsed, source) = match running {
            Some(rc) => (rc.command, rc.parsed_cmd, rc.source),
            None => (
                vec![ev.call_id.clone()],
                Vec::new(),
                ExecCommandSource::Agent,
            ),
        };
        let is_unified_exec_interaction =
            matches!(source, ExecCommandSource::UnifiedExecInteraction);

        // First check if there's a pending ExecCell for this call_id
        // (saved when the incomplete cell was flushed due to streaming)
        if let Some(pending_cell) = self.pending_exec_cells.retrieve(&ev.call_id) {
            // Preserve any existing active_cell before replacing with pending cell.
            // This ensures cells aren't lost when multiple ExecCells exist concurrently
            // (e.g., when a new tool call begins after text streaming flushes an incomplete cell).
            self.flush_active_cell();
            // Move the pending cell to active_cell so we can complete it
            self.active_cell = Some(pending_cell);
        } else {
            // Normal flow: check if active_cell is an ExecCell
            let needs_new = self
                .active_cell
                .as_ref()
                .map(|cell| cell.as_any().downcast_ref::<ExecCell>().is_none())
                .unwrap_or(true);

            if needs_new {
                self.flush_active_cell();
                self.active_cell = Some(Box::new(new_active_exec_command(
                    ev.call_id.clone(),
                    command,
                    parsed,
                    source,
                    None,
                    self.config.animations,
                )));
            }
        }

        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<ExecCell>())
        {
            let output = if is_unified_exec_interaction {
                CommandOutput {
                    exit_code: ev.exit_code,
                    formatted_output: String::new(),
                    aggregated_output: String::new(),
                }
            } else {
                CommandOutput {
                    exit_code: ev.exit_code,
                    formatted_output: ev.formatted_output.clone(),
                    aggregated_output: ev.aggregated_output.clone(),
                }
            };
            cell.complete_call(&ev.call_id, output, ev.duration);

            let is_active = cell.is_active();
            let is_exploring = cell.is_exploring_cell();

            // After completing a call, decide whether to keep the cell or flush it:
            //
            // 1. If cell still has pending calls (is_active), KEEP IT IN active_cell
            //    so it remains visible during streaming. Previously it was saved to
            //    pending_exec_cells which made it invisible - that was the bug.
            //
            // 2. If cell is fully complete AND is an exploring cell, keep it in
            //    active_cell to allow grouping with subsequent exploring commands.
            //
            // 3. If cell is fully complete AND is NOT an exploring cell, flush it
            //    to history immediately.
            if !is_active && !is_exploring {
                self.flush_active_cell();
            }
        }
    }

    pub(crate) fn handle_patch_apply_end_now(
        &mut self,
        event: codex_core::protocol::PatchApplyEndEvent,
    ) {
        // Observe directories from file paths to potentially update footer git info.
        self.observe_directories_from_changes(&event.changes);

        // If the patch was successful, just let the "Edited" block stand.
        // Otherwise, add a failure block.
        if !event.success {
            self.add_to_history(history_cell::new_patch_apply_failure(event.stderr));
        }
    }

    /// Observes the parent directories of changed files to update the effective CWD tracker.
    /// If the effective CWD changes (after debounce), triggers a system info refresh.
    ///
    /// Uses the git repository root for the refresh directory rather than the file's parent
    /// to ensure git commands work correctly. Also skips directories that don't exist yet
    /// (which can happen when creating new files in new directories).
    fn observe_directories_from_changes(
        &mut self,
        changes: &std::collections::HashMap<PathBuf, codex_core::protocol::FileChange>,
    ) {
        for file_path in changes.keys() {
            // Resolve relative paths against config.cwd before extracting parent
            let absolute_path = if file_path.is_absolute() {
                file_path.clone()
            } else {
                self.config.cwd.join(file_path)
            };

            if self.effective_cwd_tracker.observe_file_path(&absolute_path) {
                // Find the git root for this path, falling back to parent directory
                // This ensures git commands work correctly even when the immediate
                // parent directory doesn't exist yet (new file in new directory)
                let refresh_dir = crate::effective_cwd_tracker::find_git_root(&absolute_path)
                    .or_else(|| {
                        // Fall back to parent directory only if it exists
                        absolute_path
                            .parent()
                            .filter(|p| p.exists())
                            .map(std::path::Path::to_path_buf)
                    });

                if let Some(dir) = refresh_dir {
                    self.app_event_tx
                        .send(AppEvent::RefreshSystemInfoForDirectory {
                            dir,
                            model: Some(self.config.model.clone()),
                        });
                }
            }
        }
    }

    pub(crate) fn handle_exec_approval_now(&mut self, id: String, ev: ExecApprovalRequestEvent) {
        self.flush_answer_stream_with_separator();
        let command = shlex::try_join(ev.command.iter().map(String::as_str))
            .unwrap_or_else(|_| ev.command.join(" "));
        self.notify(Notification::ExecApprovalRequested { command });

        let request = ApprovalRequest::Exec {
            id,
            command: ev.command,
            reason: ev.reason,
            risk: ev.risk,
        };
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
    }

    pub(crate) fn handle_apply_patch_approval_now(
        &mut self,
        id: String,
        ev: ApplyPatchApprovalRequestEvent,
    ) {
        self.flush_answer_stream_with_separator();

        let request = ApprovalRequest::ApplyPatch {
            id,
            reason: ev.reason,
            changes: ev.changes.clone(),
            cwd: self.config.cwd.clone(),
        };
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
        self.notify(Notification::EditApprovalRequested {
            cwd: self.config.cwd.clone(),
            changes: ev.changes.keys().cloned().collect(),
        });
    }

    pub(crate) fn handle_elicitation_request_now(&mut self, ev: ElicitationRequestEvent) {
        self.flush_answer_stream_with_separator();

        self.notify(Notification::ElicitationRequested {
            server_name: ev.server_name.clone(),
        });

        let request = ApprovalRequest::McpElicitation {
            server_name: ev.server_name,
            request_id: ev.id,
            message: ev.message,
        };
        self.bottom_pane.push_approval_request(request);
        self.request_redraw();
    }

    pub(crate) fn handle_exec_begin_now(&mut self, ev: ExecCommandBeginEvent) {
        // Track Bash tool call for session statistics
        self.session_stats.record_tool_call("Bash");

        // Check if any parsed commands are Read operations to SKILL.md files
        for parsed_cmd in &ev.parsed_cmd {
            if let codex_protocol::parse_command::ParsedCommand::Read { path, .. } = parsed_cmd
                && let Some(skill_name) = extract_skill_from_read_path(path.to_str())
            {
                self.session_stats.record_skill(&skill_name);
            }
        }

        // Observe the command's working directory to potentially update footer git info.
        // If the effective CWD changes (after debounce), trigger a system info refresh.
        if self.effective_cwd_tracker.observe_directory(ev.cwd.clone()) {
            self.app_event_tx
                .send(AppEvent::RefreshSystemInfoForDirectory {
                    dir: ev.cwd.clone(),
                    model: Some(self.config.model.clone()),
                });
        }

        // Ensure the status indicator is visible while the command runs.
        self.running_commands.insert(
            ev.call_id.clone(),
            RunningCommand {
                command: ev.command.clone(),
                parsed_cmd: ev.parsed_cmd.clone(),
                source: ev.source,
            },
        );
        let is_wait_interaction = matches!(ev.source, ExecCommandSource::UnifiedExecInteraction)
            && ev
                .interaction_input
                .as_deref()
                .map(str::is_empty)
                .unwrap_or(true);
        let command_display = ev.command.join(" ");
        let should_suppress_unified_wait = is_wait_interaction
            && self
                .last_unified_wait
                .as_ref()
                .is_some_and(|wait| wait.is_duplicate(&command_display));
        if is_wait_interaction {
            self.last_unified_wait = Some(UnifiedExecWaitState::new(command_display));
        } else {
            self.last_unified_wait = None;
        }
        if should_suppress_unified_wait {
            self.suppressed_exec_calls.insert(ev.call_id);
            return;
        }
        let interaction_input = ev.interaction_input.clone();

        // Check if we can add this call to an existing ExecCell
        if let Some(cell) = self
            .active_cell
            .as_mut()
            .and_then(|c| c.as_any_mut().downcast_mut::<ExecCell>())
            && let Some(new_exec) = cell.with_added_call(
                ev.call_id.clone(),
                ev.command.clone(),
                ev.parsed_cmd.clone(),
                ev.source,
                interaction_input.clone(),
            )
        {
            *cell = new_exec;
        } else {
            self.flush_active_cell();

            self.active_cell = Some(Box::new(new_active_exec_command(
                ev.call_id.clone(),
                ev.command.clone(),
                ev.parsed_cmd,
                ev.source,
                interaction_input,
                self.config.animations,
            )));
        }

        self.request_redraw();
    }

    pub(crate) fn handle_mcp_begin_now(&mut self, ev: McpToolCallBeginEvent) {
        // Track tool call for session statistics
        self.session_stats.record_tool_call(&ev.invocation.tool);

        // Check if this is a Skill tool call and extract skill name
        if ev.invocation.tool == "Skill"
            && let Some(skill_name) = extract_skill_from_raw_input(ev.invocation.arguments.as_ref())
        {
            self.session_stats.record_skill(&skill_name);
        }

        // Check if this is a Task tool call and extract subagent type
        if ev.invocation.tool == "Task"
            && let Some(subagent_type) =
                extract_subagent_from_raw_input(ev.invocation.arguments.as_ref())
        {
            self.session_stats.record_subagent(&subagent_type);
        }

        self.flush_answer_stream_with_separator();
        self.flush_active_cell();
        self.active_cell = Some(Box::new(history_cell::new_active_mcp_tool_call(
            ev.call_id,
            ev.invocation,
            self.config.animations,
        )));
        self.request_redraw();
    }
    pub(crate) fn handle_mcp_end_now(&mut self, ev: McpToolCallEndEvent) {
        self.flush_answer_stream_with_separator();

        let McpToolCallEndEvent {
            call_id,
            invocation,
            duration,
            result,
        } = ev;

        // If this is a Task tool call, scan the result text for skill paths
        // This captures skills used by subagents whose tool calls are not directly visible
        if invocation.tool == "Task"
            && let Ok(tool_result) = &result
        {
            for content_block in &tool_result.content {
                if let mcp_types::ContentBlock::TextContent(text_content) = content_block {
                    for skill_name in extract_skills_from_text(&text_content.text) {
                        self.session_stats.record_skill(&skill_name);
                    }
                }
            }
        }

        let extra_cell = match self
            .active_cell
            .as_mut()
            .and_then(|cell| cell.as_any_mut().downcast_mut::<McpToolCallCell>())
        {
            Some(cell) if cell.call_id() == call_id => cell.complete(duration, result),
            _ => {
                self.flush_active_cell();
                let mut cell = history_cell::new_active_mcp_tool_call(
                    call_id,
                    invocation,
                    self.config.animations,
                );
                let extra_cell = cell.complete(duration, result);
                self.active_cell = Some(Box::new(cell));
                extra_cell
            }
        };

        self.flush_active_cell();
        if let Some(extra) = extra_cell {
            self.add_boxed_history(extra);
        }
    }

    pub(crate) fn new(
        common: ChatWidgetInit,
        conversation_manager: Arc<ConversationManager>,
    ) -> Self {
        let ChatWidgetInit {
            config,
            frame_requester,
            app_event_tx,
            initial_prompt,
            initial_images,
            enhanced_keys_supported,
            auth_manager,
            vertical_footer,
            expected_model,
        } = common;
        let mut rng = rand::rng();
        let placeholder = EXAMPLE_PROMPTS[rng.random_range(0..EXAMPLE_PROMPTS.len())].to_string();
        let spawn_result = spawn_agent(config.clone(), app_event_tx.clone(), conversation_manager);

        let first_prompt_text = initial_prompt.clone();
        let mut widget = Self {
            app_event_tx: app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            codex_op_tx: spawn_result.op_tx,
            bottom_pane: BottomPane::new(BottomPaneParams {
                frame_requester,
                app_event_tx,
                has_input_focus: true,
                enhanced_keys_supported,
                placeholder_text: placeholder,
                disable_paste_burst: config.disable_paste_burst,
                animations_enabled: config.animations,
                vertical_footer,
                model_display_name: crate::nori::agent_picker::get_agent_info(&config.model)
                    .map(|info| info.display_name)
                    .unwrap_or_else(|| config.model.clone()),
            }),
            active_cell: None,
            config: config.clone(),
            auth_manager,
            session_header: SessionHeader::new(config.model),
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            token_info: None,
            rate_limit_snapshot: None,
            rate_limit_warnings: RateLimitWarningState::default(),
            rate_limit_switch_prompt: RateLimitSwitchPromptState::default(),
            rate_limit_poller: None,
            stream_controller: None,
            running_commands: HashMap::new(),
            suppressed_exec_calls: HashSet::new(),
            last_unified_wait: None,
            task_complete_pending: false,
            mcp_startup_status: None,
            interrupts: InterruptManager::new(),
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: String::from("Working"),
            retry_status_header: None,
            conversation_id: None,
            queued_user_messages: VecDeque::new(),
            show_welcome_banner: true,
            suppress_session_configured_redraw: false,
            pending_notification: None,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            current_rollout_path: None,
            pending_exec_cells: PendingExecCellTracker::new(),
            effective_cwd_tracker: EffectiveCwdTracker::with_initial_cwd(config.cwd),
            pending_agent: None,
            expected_model,
            session_configured_received: false,
            #[cfg(feature = "unstable")]
            acp_handle: spawn_result.acp_handle,
            session_stats: SessionStats::new(),
            login_handler: None,
            first_prompt_text,
            loop_remaining: None,
            loop_total: None,
        };

        widget.prefetch_rate_limits();

        widget
    }

    /// Create a ChatWidget attached to an existing conversation (e.g., a fork).
    pub(crate) fn new_from_existing(
        common: ChatWidgetInit,
        conversation: std::sync::Arc<codex_core::CodexConversation>,
        session_configured: codex_core::protocol::SessionConfiguredEvent,
    ) -> Self {
        let ChatWidgetInit {
            config,
            frame_requester,
            app_event_tx,
            initial_prompt,
            initial_images,
            enhanced_keys_supported,
            auth_manager,
            vertical_footer,
            expected_model,
        } = common;
        let mut rng = rand::rng();
        let placeholder = EXAMPLE_PROMPTS[rng.random_range(0..EXAMPLE_PROMPTS.len())].to_string();

        let codex_op_tx =
            spawn_agent_from_existing(conversation, session_configured, app_event_tx.clone());

        let first_prompt_text = initial_prompt.clone();
        let mut widget = Self {
            app_event_tx: app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            codex_op_tx,
            bottom_pane: BottomPane::new(BottomPaneParams {
                frame_requester,
                app_event_tx,
                has_input_focus: true,
                enhanced_keys_supported,
                placeholder_text: placeholder,
                disable_paste_burst: config.disable_paste_burst,
                animations_enabled: config.animations,
                vertical_footer,
                model_display_name: crate::nori::agent_picker::get_agent_info(&config.model)
                    .map(|info| info.display_name)
                    .unwrap_or_else(|| config.model.clone()),
            }),
            active_cell: None,
            config: config.clone(),
            auth_manager,
            session_header: SessionHeader::new(config.model),
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            token_info: None,
            rate_limit_snapshot: None,
            rate_limit_warnings: RateLimitWarningState::default(),
            rate_limit_switch_prompt: RateLimitSwitchPromptState::default(),
            rate_limit_poller: None,
            stream_controller: None,
            running_commands: HashMap::new(),
            suppressed_exec_calls: HashSet::new(),
            last_unified_wait: None,
            task_complete_pending: false,
            mcp_startup_status: None,
            interrupts: InterruptManager::new(),
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: String::from("Working"),
            retry_status_header: None,
            conversation_id: None,
            queued_user_messages: VecDeque::new(),
            show_welcome_banner: true,
            suppress_session_configured_redraw: true,
            pending_notification: None,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            current_rollout_path: None,
            pending_exec_cells: PendingExecCellTracker::new(),
            effective_cwd_tracker: EffectiveCwdTracker::with_initial_cwd(config.cwd),
            pending_agent: None,
            expected_model,
            // For existing conversations, we've already received SessionConfigured
            session_configured_received: true,
            // No ACP handle for existing conversations (they are HTTP mode only)
            #[cfg(feature = "unstable")]
            acp_handle: None,
            session_stats: SessionStats::new(),
            login_handler: None,
            first_prompt_text,
            loop_remaining: None,
            loop_total: None,
        };

        widget.prefetch_rate_limits();

        widget
    }

    /// Create a ChatWidget that resumes an ACP session via `session/load`
    /// or client-side replay when the agent doesn't support `session/load`.
    pub(crate) fn new_resumed_acp(
        common: ChatWidgetInit,
        acp_session_id: Option<String>,
        transcript: codex_acp::transcript::Transcript,
    ) -> Self {
        let ChatWidgetInit {
            config,
            frame_requester,
            app_event_tx,
            initial_prompt,
            initial_images,
            enhanced_keys_supported,
            auth_manager,
            vertical_footer,
            expected_model,
        } = common;
        let mut rng = rand::rng();
        let placeholder = EXAMPLE_PROMPTS[rng.random_range(0..EXAMPLE_PROMPTS.len())].to_string();
        let spawn_result = spawn_acp_agent_resume(
            config.clone(),
            acp_session_id,
            transcript,
            app_event_tx.clone(),
        );

        let first_prompt_text = initial_prompt.clone();
        let mut widget = Self {
            app_event_tx: app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            codex_op_tx: spawn_result.op_tx,
            bottom_pane: BottomPane::new(BottomPaneParams {
                frame_requester,
                app_event_tx,
                has_input_focus: true,
                enhanced_keys_supported,
                placeholder_text: placeholder,
                disable_paste_burst: config.disable_paste_burst,
                animations_enabled: config.animations,
                vertical_footer,
                model_display_name: crate::nori::agent_picker::get_agent_info(&config.model)
                    .map(|info| info.display_name)
                    .unwrap_or_else(|| config.model.clone()),
            }),
            active_cell: None,
            config: config.clone(),
            auth_manager,
            session_header: SessionHeader::new(config.model),
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            token_info: None,
            rate_limit_snapshot: None,
            rate_limit_warnings: RateLimitWarningState::default(),
            rate_limit_switch_prompt: RateLimitSwitchPromptState::default(),
            rate_limit_poller: None,
            stream_controller: None,
            running_commands: HashMap::new(),
            suppressed_exec_calls: HashSet::new(),
            last_unified_wait: None,
            task_complete_pending: false,
            mcp_startup_status: None,
            interrupts: InterruptManager::new(),
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: String::from("Working"),
            retry_status_header: None,
            conversation_id: None,
            queued_user_messages: VecDeque::new(),
            show_welcome_banner: false,
            suppress_session_configured_redraw: false,
            pending_notification: None,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            current_rollout_path: None,
            pending_exec_cells: PendingExecCellTracker::new(),
            effective_cwd_tracker: EffectiveCwdTracker::with_initial_cwd(config.cwd),
            pending_agent: None,
            expected_model,
            session_configured_received: false,
            #[cfg(feature = "unstable")]
            acp_handle: spawn_result.acp_handle,
            session_stats: SessionStats::new(),
            login_handler: None,
            first_prompt_text,
            loop_remaining: None,
            loop_total: None,
        };

        widget.prefetch_rate_limits();

        widget
    }

    /// Set a pending agent to switch to on the next prompt submission.
    pub(crate) fn set_pending_agent(&mut self, model_name: String, display_name: String) {
        // Update the bottom pane's model display name for approval dialogs
        self.bottom_pane
            .set_model_display_name(display_name.clone());
        self.pending_agent = Some(PendingAgentInfo {
            model_name,
            display_name,
        });
    }

    pub(crate) fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event {
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) && c.eq_ignore_ascii_case(&'c') => {
                self.on_ctrl_c();
                return;
            }
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) && c.eq_ignore_ascii_case(&'v') => {
                match paste_image_to_temp_png() {
                    Ok((path, info)) => {
                        self.attach_image(
                            path,
                            info.width,
                            info.height,
                            info.encoded_format.label(),
                        );
                    }
                    Err(err) => {
                        tracing::warn!("failed to paste image: {err}");
                        self.add_to_history(history_cell::new_error_event(format!(
                            "Failed to paste image: {err}",
                        )));
                    }
                }
                return;
            }
            other if other.kind == KeyEventKind::Press => {
                self.bottom_pane.clear_ctrl_c_quit_hint();
            }
            _ => {}
        }

        match key_event {
            KeyEvent {
                code: KeyCode::Up,
                modifiers: KeyModifiers::ALT,
                kind: KeyEventKind::Press,
                ..
            } if !self.queued_user_messages.is_empty() => {
                // Prefer the most recently queued item.
                if let Some(user_message) = self.queued_user_messages.pop_back() {
                    self.bottom_pane.set_composer_text(user_message.text);
                    self.refresh_queued_user_messages();
                    self.request_redraw();
                }
            }
            _ => {
                match self.bottom_pane.handle_key_event(key_event) {
                    InputResult::Submitted(text) => {
                        // If a task is running, queue the user input to be sent after the turn completes.
                        let user_message = UserMessage {
                            text,
                            image_paths: self.bottom_pane.take_recent_submission_images(),
                        };
                        self.queue_user_message(user_message);
                    }
                    InputResult::Command(cmd) => {
                        self.dispatch_command(cmd);
                    }
                    InputResult::None => {}
                }
            }
        }
    }

    pub(crate) fn attach_image(
        &mut self,
        path: PathBuf,
        width: u32,
        height: u32,
        format_label: &str,
    ) {
        tracing::info!(
            "attach_image path={path:?} width={width} height={height} format={format_label}",
        );
        self.bottom_pane
            .attach_image(path, width, height, format_label);
        self.request_redraw();
    }

    fn dispatch_command(&mut self, cmd: SlashCommand) {
        if !cmd.available_during_task() && self.bottom_pane.is_task_running() {
            let message = format!(
                "'/{}' is disabled while a task is in progress.",
                cmd.command()
            );
            self.add_to_history(history_cell::new_error_event(message));
            self.request_redraw();
            return;
        }
        match cmd {
            SlashCommand::New => {
                self.app_event_tx.send(AppEvent::NewSession);
            }
            SlashCommand::Resume => {
                self.open_resume_session_picker();
            }
            SlashCommand::ResumeViewonly => {
                self.open_viewonly_session_picker();
            }
            SlashCommand::Init => {
                let init_target = self.config.cwd.join(DEFAULT_PROJECT_DOC_FILENAME);
                if init_target.exists() {
                    let message = format!(
                        "{DEFAULT_PROJECT_DOC_FILENAME} already exists here. Skipping /init to avoid overwriting it."
                    );
                    self.add_info_message(message, None);
                    return;
                }
                const INIT_PROMPT: &str = include_str!("../prompt_for_init_command.md");
                self.submit_user_message(INIT_PROMPT.to_string().into());
            }
            SlashCommand::Compact => {
                self.clear_token_usage();
                self.app_event_tx.send(AppEvent::CodexOp(Op::Compact));
            }
            SlashCommand::Agent => {
                self.open_agent_popup();
            }
            SlashCommand::Model => {
                self.open_model_popup();
            }
            SlashCommand::Approvals => {
                self.open_approvals_popup();
            }
            #[cfg(feature = "nori-config")]
            SlashCommand::Config => {
                // Load NoriConfig from the default path and open the config popup
                match codex_acp::config::NoriConfig::load() {
                    Ok(nori_config) => {
                        self.open_config_popup(&nori_config);
                    }
                    Err(err) => {
                        self.add_error_message(format!("Failed to load config: {err}"));
                    }
                }
            }
            #[cfg(not(feature = "nori-config"))]
            SlashCommand::Config => {
                self.add_info_message(
                    "Config command requires the nori-config feature".to_string(),
                    None,
                );
            }
            SlashCommand::Quit | SlashCommand::Exit => {
                self.submit_op(Op::Shutdown);
            }
            SlashCommand::Login => {
                self.handle_login_command();
            }
            SlashCommand::Logout => {
                self.add_info_message(
                    "To logout, run the agent's logout command directly (e.g., `claude /logout`)"
                        .to_string(),
                    None,
                );
            }
            SlashCommand::Undo => {
                self.app_event_tx.send(AppEvent::CodexOp(Op::UndoList));
            }
            SlashCommand::Diff => {
                self.add_diff_in_progress();
                let tx = self.app_event_tx.clone();
                tokio::spawn(async move {
                    let text = match get_git_diff().await {
                        Ok((is_git_repo, diff_text)) => {
                            if is_git_repo {
                                diff_text
                            } else {
                                "`/diff` — _not inside a git repository_".to_string()
                            }
                        }
                        Err(e) => format!("Failed to compute diff: {e}"),
                    };
                    tx.send(AppEvent::DiffResult(text));
                });
            }
            SlashCommand::Mention => {
                self.insert_str("@");
            }
            SlashCommand::Status => {
                self.add_status_output();
            }
            SlashCommand::FirstPrompt => {
                if let Some(text) = &self.first_prompt_text {
                    self.add_info_message(text.clone(), None);
                } else {
                    self.add_info_message("No prompt has been submitted yet.".to_string(), None);
                }
            }
            SlashCommand::Mcp => {
                self.add_mcp_output();
            }
            SlashCommand::Rollout => {
                if let Some(path) = self.rollout_path() {
                    self.add_info_message(
                        format!("Current rollout path: {}", path.display()),
                        None,
                    );
                } else {
                    self.add_info_message("Rollout path is not available yet.".to_string(), None);
                }
            }
            SlashCommand::TestApproval => {
                use codex_core::protocol::EventMsg;
                use std::collections::HashMap;

                use codex_core::protocol::ApplyPatchApprovalRequestEvent;
                use codex_core::protocol::FileChange;

                self.app_event_tx.send(AppEvent::CodexEvent(Event {
                    id: "1".to_string(),
                    // msg: EventMsg::ExecApprovalRequest(ExecApprovalRequestEvent {
                    //     call_id: "1".to_string(),
                    //     command: vec!["git".into(), "apply".into()],
                    //     cwd: self.config.cwd.clone(),
                    //     reason: Some("test".to_string()),
                    // }),
                    msg: EventMsg::ApplyPatchApprovalRequest(ApplyPatchApprovalRequestEvent {
                        call_id: "1".to_string(),
                        turn_id: "turn-1".to_string(),
                        changes: HashMap::from([
                            (
                                PathBuf::from("/tmp/test.txt"),
                                FileChange::Add {
                                    content: "test".to_string(),
                                },
                            ),
                            (
                                PathBuf::from("/tmp/test2.txt"),
                                FileChange::Update {
                                    unified_diff: "+test\n-test2".to_string(),
                                    move_path: None,
                                },
                            ),
                        ]),
                        reason: None,
                        grant_root: Some(PathBuf::from("/tmp")),
                    }),
                }));
            }
            SlashCommand::SwitchSkillset => {
                self.handle_switch_skillset_command();
            }
        }
    }

    pub(crate) fn handle_paste(&mut self, text: String) {
        self.bottom_pane.handle_paste(text);
    }

    // Returns true if caller should skip rendering this frame (a future frame is scheduled).
    pub(crate) fn handle_paste_burst_tick(&mut self, frame_requester: FrameRequester) -> bool {
        if self.bottom_pane.flush_paste_burst_if_due() {
            // A paste just flushed; request an immediate redraw and skip this frame.
            self.request_redraw();
            true
        } else if self.bottom_pane.is_in_paste_burst() {
            // While capturing a burst, schedule a follow-up tick and skip this frame
            // to avoid redundant renders between ticks.
            frame_requester.schedule_frame_in(
                crate::bottom_pane::ChatComposer::recommended_paste_flush_delay(),
            );
            true
        } else {
            false
        }
    }

    fn flush_active_cell(&mut self) {
        if let Some(active) = self.active_cell.take() {
            // Check if this is an incomplete ExecCell that should be saved to pending
            // instead of being flushed to history. This prevents duplicate entries when
            // the ExecCommandEnd event arrives later.
            if let Some(exec_cell) = active.as_any().downcast_ref::<ExecCell>()
                && exec_cell.is_active()
            {
                // Get the pending call_ids before we consume the cell
                let pending_ids = exec_cell.pending_call_ids();
                if !pending_ids.is_empty() {
                    // Save to pending map with ALL pending call_ids
                    // This allows the cell to be retrieved when any of them completes
                    self.pending_exec_cells.save_pending(pending_ids, active);
                    return;
                }
            }
            // Normal flush path - cell is complete or not an ExecCell
            self.needs_final_message_separator = true;
            self.app_event_tx.send(AppEvent::InsertHistoryCell(active));
        }
    }

    fn add_to_history(&mut self, cell: impl HistoryCell + 'static) {
        self.add_boxed_history(Box::new(cell));
    }

    pub(crate) fn add_boxed_history(&mut self, cell: Box<dyn HistoryCell>) {
        if !cell.display_lines(u16::MAX).is_empty() {
            // Only break exec grouping if the cell renders visible lines.
            // EXCEPT: Don't flush incomplete ExecCells - they should remain visible
            // in active_cell while streaming content is added to history.
            let should_flush = self
                .active_cell
                .as_ref()
                .map(|c| {
                    c.as_any()
                        .downcast_ref::<ExecCell>()
                        .map(|exec| !exec.is_active())
                        .unwrap_or(true)
                })
                .unwrap_or(true);
            if should_flush {
                self.flush_active_cell();
            }
            self.needs_final_message_separator = true;
        }
        self.app_event_tx.send(AppEvent::InsertHistoryCell(cell));
    }

    fn queue_user_message(&mut self, user_message: UserMessage) {
        if self.bottom_pane.is_task_running() {
            self.queued_user_messages.push_back(user_message);
            self.refresh_queued_user_messages();
        } else {
            self.submit_user_message(user_message);
        }
    }

    fn submit_user_message(&mut self, user_message: UserMessage) {
        let UserMessage { text, image_paths } = user_message;
        if text.is_empty() && image_paths.is_empty() {
            return;
        }

        // Special-case: "/login <agent>" triggers login for a specific agent
        // This intercepts before the message is sent to the agent
        if let Some(agent_name) = text.strip_prefix("/login ").map(str::trim)
            && !agent_name.is_empty()
        {
            self.handle_login_command_with_agent(agent_name);
            return;
        }

        if self.first_prompt_text.is_none() {
            self.first_prompt_text = Some(text.clone());

            // Initialize loop mode from NoriConfig on the very first prompt.
            #[cfg(feature = "nori-config")]
            {
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                if let Some(count) = nori_config.loop_count
                    && count > 1
                {
                    self.loop_remaining = Some(count - 1);
                    self.loop_total = Some(count);
                    self.add_info_message(format!("Loop mode: will run {count} iterations."), None);
                }
            }
        }

        // Track user message for session statistics
        self.session_stats.record_user_message();

        // Refresh system info (including git branch) on user message submission.
        // This catches branch changes that happened between interactions
        // (e.g., user switched branches in another terminal).
        self.app_event_tx
            .send(AppEvent::RefreshSystemInfoForDirectory {
                dir: self.config.cwd.clone(),
                model: Some(self.config.model.clone()),
            });

        // Check if there's a pending agent switch - if so, send the message through
        // the App to trigger the switch first
        if let Some(pending) = self.pending_agent.take() {
            self.app_event_tx.send(AppEvent::SubmitWithAgentSwitch {
                model_name: pending.model_name,
                display_name: pending.display_name,
                message_text: text,
                image_paths,
            });
            return;
        }

        let mut items: Vec<UserInput> = Vec::new();

        // Special-case: "!cmd" executes a local shell command instead of sending to the model.
        if let Some(stripped) = text.strip_prefix('!') {
            let cmd = stripped.trim();
            if cmd.is_empty() {
                self.app_event_tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_info_event(
                        USER_SHELL_COMMAND_HELP_TITLE.to_string(),
                        Some(USER_SHELL_COMMAND_HELP_HINT.to_string()),
                    ),
                )));
                return;
            }
            self.submit_op(Op::RunUserShellCommand {
                command: cmd.to_string(),
            });
            return;
        }

        if !text.is_empty() {
            items.push(UserInput::Text { text: text.clone() });
        }

        for path in image_paths {
            items.push(UserInput::LocalImage { path });
        }

        self.codex_op_tx
            .send(Op::UserInput { items })
            .unwrap_or_else(|e| {
                tracing::error!("failed to send message: {e}");
            });

        // Persist the text to cross-session message history.
        if !text.is_empty() {
            self.codex_op_tx
                .send(Op::AddToHistory { text: text.clone() })
                .unwrap_or_else(|e| {
                    tracing::error!("failed to send AddHistory op: {e}");
                });
        }

        // Only show the text portion in conversation history.
        if !text.is_empty() {
            self.add_to_history(history_cell::new_user_prompt(text));
        }
        self.needs_final_message_separator = false;
    }

    /// Replay a subset of initial events into the UI to seed the transcript when
    /// resuming an existing session. This approximates the live event flow and
    /// is intentionally conservative: only safe-to-replay items are rendered to
    /// avoid triggering side effects. Event ids are passed as `None` to
    /// distinguish replayed events from live ones.
    fn replay_initial_messages(&mut self, events: Vec<EventMsg>) {
        for msg in events {
            if matches!(msg, EventMsg::SessionConfigured(_)) {
                continue;
            }
            // `id: None` indicates a synthetic/fake id coming from replay.
            self.dispatch_event_msg(None, msg, true);
        }
    }

    pub(crate) fn handle_codex_event(&mut self, event: Event) {
        let Event { id, msg } = event;

        // When expected_model is set (during agent switching), we need to filter events
        // to prevent events from the OLD agent from affecting the NEW widget.
        if let Some(ref expected) = self.expected_model {
            tracing::debug!(
                "Event filtering active: expected_model={}, session_configured_received={}",
                expected,
                self.session_configured_received
            );
            if !self.session_configured_received {
                // Only process SessionConfigured events, and only if the model matches
                match &msg {
                    EventMsg::SessionConfigured(e) => {
                        if e.model.to_lowercase() != expected.to_lowercase() {
                            tracing::debug!(
                                "Ignoring SessionConfigured from wrong model: expected={}, got={}",
                                expected,
                                e.model
                            );
                            return;
                        }
                        tracing::debug!(
                            "SessionConfigured received with matching model: {}",
                            e.model
                        );
                        // Model matches, proceed with processing
                    }
                    // Ignore all other events until SessionConfigured arrives
                    _ => {
                        tracing::debug!(
                            "Ignoring event before SessionConfigured: {:?} (waiting for model={})",
                            std::mem::discriminant(&msg),
                            expected
                        );
                        return;
                    }
                }
            }
        }

        self.dispatch_event_msg(Some(id), msg, false);
    }

    /// Dispatch a protocol `EventMsg` to the appropriate handler.
    ///
    /// `id` is `Some` for live events and `None` for replayed events from
    /// `replay_initial_messages()`. Callers should treat `None` as a "fake" id
    /// that must not be used to correlate follow-up actions.
    fn dispatch_event_msg(&mut self, id: Option<String>, msg: EventMsg, from_replay: bool) {
        match msg {
            EventMsg::AgentMessageDelta(_)
            | EventMsg::AgentReasoningDelta(_)
            | EventMsg::ExecCommandOutputDelta(_) => {}
            _ => {
                tracing::trace!("handle_codex_event: {:?}", msg);
            }
        }

        match msg {
            EventMsg::SessionConfigured(e) => self.on_session_configured(e),
            EventMsg::AgentMessage(AgentMessageEvent { message }) => self.on_agent_message(message),
            EventMsg::AgentMessageDelta(AgentMessageDeltaEvent { delta }) => {
                self.on_agent_message_delta(delta)
            }
            EventMsg::AgentReasoningDelta(AgentReasoningDeltaEvent { delta })
            | EventMsg::AgentReasoningRawContentDelta(AgentReasoningRawContentDeltaEvent {
                delta,
            }) => self.on_agent_reasoning_delta(delta),
            EventMsg::AgentReasoning(AgentReasoningEvent { .. }) => self.on_agent_reasoning_final(),
            EventMsg::AgentReasoningRawContent(AgentReasoningRawContentEvent { text }) => {
                self.on_agent_reasoning_delta(text);
                self.on_agent_reasoning_final()
            }
            EventMsg::AgentReasoningSectionBreak(_) => self.on_reasoning_section_break(),
            EventMsg::TaskStarted(_) => self.on_task_started(),
            EventMsg::TaskComplete(TaskCompleteEvent { last_agent_message }) => {
                self.on_task_complete(last_agent_message)
            }
            EventMsg::TokenCount(ev) => {
                self.set_token_info(ev.info);
                self.on_rate_limit_snapshot(ev.rate_limits);
            }
            EventMsg::Warning(WarningEvent { message }) => self.on_warning(message),
            EventMsg::Error(ErrorEvent { message, .. }) => self.on_error(message),
            EventMsg::McpStartupUpdate(ev) => self.on_mcp_startup_update(ev),
            EventMsg::McpStartupComplete(ev) => self.on_mcp_startup_complete(ev),
            EventMsg::TurnAborted(ev) => match ev.reason {
                TurnAbortReason::Interrupted => {
                    self.on_interrupted_turn(ev.reason);
                }
                TurnAbortReason::Replaced => {
                    self.on_error("Turn aborted: replaced by a new task".to_owned())
                }
            },
            EventMsg::PlanUpdate(update) => self.on_plan_update(update),
            EventMsg::ExecApprovalRequest(ev) => {
                // For replayed events, synthesize an empty id (these should not occur).
                self.on_exec_approval_request(id.unwrap_or_default(), ev)
            }
            EventMsg::ApplyPatchApprovalRequest(ev) => {
                self.on_apply_patch_approval_request(id.unwrap_or_default(), ev)
            }
            EventMsg::ElicitationRequest(ev) => {
                self.on_elicitation_request(ev);
            }
            EventMsg::ExecCommandBegin(ev) => self.on_exec_command_begin(ev),
            EventMsg::ExecCommandOutputDelta(delta) => self.on_exec_command_output_delta(delta),
            EventMsg::PatchApplyBegin(ev) => self.on_patch_apply_begin(ev),
            EventMsg::PatchApplyEnd(ev) => self.on_patch_apply_end(ev),
            EventMsg::ExecCommandEnd(ev) => self.on_exec_command_end(ev),
            EventMsg::ViewImageToolCall(ev) => self.on_view_image_tool_call(ev),
            EventMsg::McpToolCallBegin(ev) => self.on_mcp_tool_call_begin(ev),
            EventMsg::McpToolCallEnd(ev) => self.on_mcp_tool_call_end(ev),
            EventMsg::WebSearchBegin(ev) => self.on_web_search_begin(ev),
            EventMsg::WebSearchEnd(ev) => self.on_web_search_end(ev),
            EventMsg::GetHistoryEntryResponse(ev) => self.on_get_history_entry_response(ev),
            EventMsg::McpListToolsResponse(ev) => self.on_list_mcp_tools(ev),
            EventMsg::ListCustomPromptsResponse(ev) => self.on_list_custom_prompts(ev),
            EventMsg::ShutdownComplete => self.on_shutdown_complete(),
            EventMsg::TurnDiff(TurnDiffEvent { unified_diff }) => self.on_turn_diff(unified_diff),
            EventMsg::DeprecationNotice(ev) => self.on_deprecation_notice(ev),
            EventMsg::BackgroundEvent(BackgroundEventEvent { message }) => {
                self.on_background_event(message)
            }
            EventMsg::UndoStarted(ev) => self.on_undo_started(ev),
            EventMsg::UndoCompleted(ev) => self.on_undo_completed(ev),
            EventMsg::UndoListResult(ev) => self.on_undo_list_result(ev),
            EventMsg::StreamError(StreamErrorEvent { message, .. }) => {
                self.on_stream_error(message)
            }
            EventMsg::UserMessage(ev) => {
                if from_replay {
                    self.on_user_message_event(ev);
                }
            }
            EventMsg::ContextCompacted(_) => self.on_agent_message("Context compacted".to_owned()),
            EventMsg::RawResponseItem(_)
            | EventMsg::ItemStarted(_)
            | EventMsg::ItemCompleted(_)
            | EventMsg::AgentMessageContentDelta(_)
            | EventMsg::ReasoningContentDelta(_)
            | EventMsg::ReasoningRawContentDelta(_) => {}
            EventMsg::PromptSummary(ev) => self.on_prompt_summary(ev.summary),
        }
    }

    fn on_user_message_event(&mut self, event: UserMessageEvent) {
        let message = event.message.trim();
        if !message.is_empty() {
            self.add_to_history(history_cell::new_user_prompt(message.to_string()));
        }
    }

    fn request_exit(&mut self) {
        // Clear the ctrl-c quit hint to make room for the exit message
        self.bottom_pane.clear_ctrl_c_quit_hint();
        self.request_redraw();

        // Send exit request - app.rs will handle adding the exit message cell before exiting
        self.app_event_tx.send(AppEvent::ExitRequest);
    }

    /// Create an exit message cell with session statistics.
    /// Called by app.rs before exiting to display final session summary.
    pub(crate) fn create_exit_message_cell(&self) -> Box<dyn HistoryCell> {
        use crate::nori::exit_message::ExitMessageCell;

        let session_id = self
            .conversation_id()
            .map(|id| id.to_string())
            .unwrap_or_else(|| "(no session)".to_string());

        let stats = self.session_stats().clone();

        Box::new(ExitMessageCell::new(session_id, stats))
    }

    fn request_redraw(&mut self) {
        self.frame_requester.schedule_frame();
    }

    fn notify(&mut self, notification: Notification) {
        if !self.config.tui_notifications {
            return;
        }
        self.pending_notification = Some(notification);
        self.request_redraw();
    }

    pub(crate) fn maybe_post_pending_notification(&mut self, tui: &mut crate::tui::Tui) {
        if let Some(notif) = self.pending_notification.take() {
            tui.notify(notif.display());
        }
    }

    /// Mark the active cell as failed (✗) and flush it into history.
    fn finalize_active_cell_as_failed(&mut self) {
        if let Some(mut cell) = self.active_cell.take() {
            // Insert finalized cell into history and keep grouping consistent.
            if let Some(exec) = cell.as_any_mut().downcast_mut::<ExecCell>() {
                exec.mark_failed();
            } else if let Some(tool) = cell.as_any_mut().downcast_mut::<McpToolCallCell>() {
                tool.mark_failed();
            }
            self.add_boxed_history(cell);
        }
    }

    // If idle and there are queued inputs, submit exactly one to start the next turn.
    fn maybe_send_next_queued_input(&mut self) {
        if self.bottom_pane.is_task_running() {
            return;
        }
        if let Some(user_message) = self.queued_user_messages.pop_front() {
            self.submit_user_message(user_message);
        }
        // Update the list to reflect the remaining queued messages (if any).
        self.refresh_queued_user_messages();
    }

    /// Rebuild and update the queued user messages from the current queue.
    fn refresh_queued_user_messages(&mut self) {
        let messages: Vec<String> = self
            .queued_user_messages
            .iter()
            .map(|m| m.text.clone())
            .collect();
        self.bottom_pane.set_queued_user_messages(messages);
    }

    pub(crate) fn add_diff_in_progress(&mut self) {
        self.request_redraw();
    }

    pub(crate) fn on_diff_complete(&mut self) {
        self.request_redraw();
    }

    pub(crate) fn add_status_output(&mut self) {
        self.add_to_history(crate::nori::session_header::new_nori_status_output(
            &self.config.model,
            self.config.cwd.clone(),
        ));
    }
    fn stop_rate_limit_poller(&mut self) {
        if let Some(handle) = self.rate_limit_poller.take() {
            handle.abort();
        }
    }

    fn prefetch_rate_limits(&mut self) {
        // Rate limit prefetching is not used in Nori (no backend-client)
    }

    fn lower_cost_preset(&self) -> Option<ModelPreset> {
        let auth_mode = self.auth_manager.auth().map(|auth| auth.mode);
        builtin_model_presets(auth_mode)
            .into_iter()
            .find(|preset| preset.model == NUDGE_MODEL_SLUG)
    }

    fn rate_limit_switch_prompt_hidden(&self) -> bool {
        self.config
            .notices
            .hide_rate_limit_model_nudge
            .unwrap_or(false)
    }

    fn maybe_show_pending_rate_limit_prompt(&mut self) {
        if self.rate_limit_switch_prompt_hidden() {
            self.rate_limit_switch_prompt = RateLimitSwitchPromptState::Idle;
            return;
        }
        if !matches!(
            self.rate_limit_switch_prompt,
            RateLimitSwitchPromptState::Pending
        ) {
            return;
        }
        if let Some(preset) = self.lower_cost_preset() {
            self.open_rate_limit_switch_prompt(preset);
            self.rate_limit_switch_prompt = RateLimitSwitchPromptState::Shown;
        } else {
            self.rate_limit_switch_prompt = RateLimitSwitchPromptState::Idle;
        }
    }

    fn open_rate_limit_switch_prompt(&mut self, preset: ModelPreset) {
        let switch_model = preset.model.to_string();
        let display_name = preset.display_name.to_string();
        let default_effort: ReasoningEffortConfig = preset.default_reasoning_effort;

        let switch_actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
            tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
                cwd: None,
                approval_policy: None,
                sandbox_policy: None,
                model: Some(switch_model.clone()),
                effort: Some(Some(default_effort)),
                summary: None,
            }));
            tx.send(AppEvent::UpdateModel(switch_model.clone()));
            tx.send(AppEvent::UpdateReasoningEffort(Some(default_effort)));
        })];

        let keep_actions: Vec<SelectionAction> = Vec::new();
        let never_actions: Vec<SelectionAction> = vec![Box::new(|tx| {
            tx.send(AppEvent::UpdateRateLimitSwitchPromptHidden(true));
            tx.send(AppEvent::PersistRateLimitSwitchPromptHidden);
        })];
        let description = if preset.description.is_empty() {
            Some("Uses fewer credits for upcoming turns.".to_string())
        } else {
            Some(preset.description.to_string())
        };

        let items = vec![
            SelectionItem {
                name: format!("Switch to {display_name}"),
                description,
                selected_description: None,
                is_current: false,
                actions: switch_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Keep current model".to_string(),
                description: None,
                selected_description: None,
                is_current: false,
                actions: keep_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Keep current model (never show again)".to_string(),
                description: Some(
                    "Hide future rate limit reminders about switching models.".to_string(),
                ),
                selected_description: None,
                is_current: false,
                actions: never_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Approaching rate limits".to_string()),
            subtitle: Some(format!("Switch to {display_name} for lower credit usage?")),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
    }

    /// Open the agent picker popup for ACP mode.
    pub(crate) fn open_agent_popup(&mut self) {
        let current_model = self.config.model.clone();
        let params = crate::nori::agent_picker::agent_picker_params(
            &current_model,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Show a selection view in the bottom pane.
    pub(crate) fn show_selection_view(&mut self, params: SelectionViewParams) {
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the viewonly session picker to select a previous session to view.
    pub(crate) fn open_viewonly_session_picker(&mut self) {
        let cwd = self.config.cwd.clone();
        let tx = self.app_event_tx.clone();

        // Get NORI_HOME - if not available, show error
        let nori_home = match crate::nori::config_adapter::get_nori_home() {
            Ok(home) => home,
            Err(e) => {
                self.add_error_message(format!("Failed to find NORI_HOME: {e}"));
                return;
            }
        };

        let nori_home_for_event = nori_home.clone();
        tokio::spawn(async move {
            match crate::nori::viewonly_session_picker::load_sessions_with_preview(&nori_home, &cwd)
                .await
            {
                Ok(sessions) => {
                    if sessions.is_empty() {
                        tx.send(crate::app_event::AppEvent::InsertHistoryCell(Box::new(
                            crate::history_cell::new_error_event(
                                "No previous sessions found for this project.".to_string(),
                            ),
                        )));
                    } else {
                        tx.send(crate::app_event::AppEvent::ShowViewonlySessionPicker {
                            sessions,
                            nori_home: nori_home_for_event,
                        });
                    }
                }
                Err(e) => {
                    tx.send(crate::app_event::AppEvent::InsertHistoryCell(Box::new(
                        crate::history_cell::new_error_event(format!(
                            "Failed to load sessions: {e}"
                        )),
                    )));
                }
            }
        });
    }

    pub(crate) fn open_resume_session_picker(&mut self) {
        let cwd = self.config.cwd.clone();
        let tx = self.app_event_tx.clone();
        let model = self.config.model.clone();

        let nori_home = match crate::nori::config_adapter::get_nori_home() {
            Ok(home) => home,
            Err(e) => {
                self.add_error_message(format!("Failed to find NORI_HOME: {e}"));
                return;
            }
        };

        let nori_home_for_event = nori_home.clone();
        tokio::spawn(async move {
            match crate::nori::resume_session_picker::load_resumable_sessions(
                &nori_home, &cwd, &model,
            )
            .await
            {
                Ok(sessions) => {
                    if sessions.is_empty() {
                        tx.send(crate::app_event::AppEvent::InsertHistoryCell(Box::new(
                            crate::history_cell::new_error_event(
                                "No resumable sessions found for this project and agent."
                                    .to_string(),
                            ),
                        )));
                    } else {
                        tx.send(crate::app_event::AppEvent::ShowResumeSessionPicker {
                            sessions,
                            nori_home: nori_home_for_event,
                        });
                    }
                }
                Err(e) => {
                    tx.send(crate::app_event::AppEvent::InsertHistoryCell(Box::new(
                        crate::history_cell::new_error_event(format!(
                            "Failed to load sessions: {e}"
                        )),
                    )));
                }
            }
        });
    }

    /// Open the config popup for TUI settings.
    #[cfg(feature = "nori-config")]
    pub(crate) fn open_config_popup(&mut self, nori_config: &codex_acp::config::NoriConfig) {
        let params = crate::nori::config_picker::config_picker_params(
            nori_config,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the notify-after-idle sub-picker.
    #[cfg(feature = "nori-config")]
    pub(crate) fn open_notify_after_idle_picker(
        &mut self,
        current: codex_acp::config::NotifyAfterIdle,
    ) {
        let params = crate::nori::config_picker::notify_after_idle_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the script timeout sub-picker.
    #[cfg(feature = "nori-config")]
    pub(crate) fn open_script_timeout_picker(&mut self, current: codex_acp::config::ScriptTimeout) {
        let params = crate::nori::config_picker::script_timeout_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the loop count sub-picker.
    #[cfg(feature = "nori-config")]
    pub(crate) fn open_loop_count_picker(&mut self, current: Option<i32>) {
        let params = crate::nori::config_picker::loop_count_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Set the loop state for a new iteration.
    #[cfg(feature = "nori-config")]
    pub(crate) fn set_loop_state(&mut self, remaining: i32, total: i32) {
        self.loop_remaining = Some(remaining);
        self.loop_total = Some(total);
    }

    /// Cancel any active loop.
    fn cancel_loop(&mut self) {
        if self.loop_remaining.is_some() {
            self.loop_remaining = None;
            self.loop_total = None;
            self.add_info_message("Loop cancelled.".to_string(), None);
        }
    }

    /// Open the hotkey picker sub-view.
    pub(crate) fn open_hotkey_picker(&mut self, hotkey_config: codex_acp::config::HotkeyConfig) {
        let view = crate::nori::hotkey_picker::HotkeyPickerView::new(
            &hotkey_config,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    /// Update the hotkey configuration used by the textarea for editing bindings.
    pub(crate) fn set_hotkey_config(&mut self, config: codex_acp::config::HotkeyConfig) {
        self.bottom_pane.set_hotkey_config(config);
    }

    pub(crate) fn set_vim_mode_enabled(&mut self, enabled: bool) {
        self.bottom_pane.set_vim_mode_enabled(enabled);
    }

    /// Handle the /switch-skillset command.
    /// Checks if nori-skillsets is available and lists available skillsets.
    fn handle_switch_skillset_command(&mut self) {
        use crate::nori::skillset_picker;

        // Check if nori-skillsets is available in PATH
        if !skillset_picker::is_nori_skillsets_available() {
            self.add_info_message(skillset_picker::not_installed_message(), None);
            return;
        }

        // Spawn async task to list skillsets
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            match skillset_picker::list_skillsets().await {
                Ok(names) if names.is_empty() => {
                    tx.send(AppEvent::SkillsetListResult {
                        names: Some(vec![]),
                        error: Some("No skillsets available.".to_string()),
                    });
                }
                Ok(names) => {
                    tx.send(AppEvent::SkillsetListResult {
                        names: Some(names),
                        error: None,
                    });
                }
                Err(message) => {
                    tx.send(AppEvent::SkillsetListResult {
                        names: None,
                        error: Some(message),
                    });
                }
            }
        });
    }

    /// Handle the result of listing skillsets.
    pub(crate) fn on_skillset_list_result(
        &mut self,
        names: Option<Vec<String>>,
        error: Option<String>,
    ) {
        match (names, error) {
            (Some(names), None) if !names.is_empty() => {
                // Open the skillset picker
                let params = crate::nori::skillset_picker::skillset_picker_params(names);
                self.bottom_pane.show_selection_view(params);
            }
            (_, Some(error)) => {
                self.add_error_message(error);
            }
            _ => {
                self.add_info_message("No skillsets available.".to_string(), None);
            }
        }
    }

    /// Handle a request to install a skillset.
    pub(crate) fn on_install_skillset_request(&mut self, name: &str) {
        use crate::nori::skillset_picker;

        let name = name.to_string();
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            match skillset_picker::install_skillset(&name).await {
                Ok(message) => {
                    tx.send(AppEvent::SkillsetInstallResult {
                        name,
                        success: true,
                        message,
                    });
                }
                Err(message) => {
                    tx.send(AppEvent::SkillsetInstallResult {
                        name,
                        success: false,
                        message,
                    });
                }
            }
        });
    }

    /// Handle the result of installing a skillset.
    pub(crate) fn on_skillset_install_result(&mut self, name: &str, success: bool, message: &str) {
        if success {
            self.add_info_message(message.to_string(), None);
        } else {
            self.add_error_message(format!("Failed to install skillset '{name}': {message}"));
        }
    }

    /// Open a popup to choose the model (stage 1). After selecting a model,
    /// a second popup is shown to choose the reasoning effort.
    ///
    /// In ACP mode (when current model is an ACP agent), this fetches available
    /// models from the agent and shows them for selection.
    pub(crate) fn open_model_popup(&mut self) {
        let current_model = self.config.model.clone();

        // Check if we're in ACP mode by checking if the current model is registered
        // in the ACP agent registry
        if codex_acp::get_agent_config(&current_model).is_ok() {
            #[cfg(feature = "unstable")]
            {
                // ACP mode with unstable features - try to get model state from the agent
                if let Some(handle) = self.acp_handle.clone() {
                    let app_event_tx = self.app_event_tx.clone();
                    tokio::spawn(async move {
                        if let Some(model_state) = handle.get_model_state().await {
                            let models: Vec<crate::app_event::AcpModelInfo> = model_state
                                .available_models
                                .iter()
                                .map(|m| {
                                    let display_name = if m.name.is_empty() {
                                        m.model_id.to_string()
                                    } else {
                                        m.name.clone()
                                    };
                                    crate::app_event::AcpModelInfo {
                                        model_id: m.model_id.to_string(),
                                        display_name,
                                        description: m.description.clone(),
                                    }
                                })
                                .collect();
                            let current_model_id =
                                model_state.current_model_id.map(|id| id.to_string());
                            app_event_tx.send(AppEvent::OpenAcpModelPicker {
                                models,
                                current_model_id,
                            });
                        } else {
                            // Failed to get model state - show empty picker with explanation
                            tracing::warn!("Failed to get ACP model state");
                            app_event_tx.send(AppEvent::OpenAcpModelPicker {
                                models: vec![],
                                current_model_id: None,
                            });
                        }
                    });
                    return;
                }
            }
            // ACP mode but no handle or unstable not enabled - show disabled model picker
            let params = crate::nori::agent_picker::acp_model_picker_params();
            self.bottom_pane.show_selection_view(params);
            return;
        }

        // Standard HTTP mode - show normal model picker
        let auth_mode = self.auth_manager.auth().map(|auth| auth.mode);
        let presets: Vec<ModelPreset> = builtin_model_presets(auth_mode);

        let mut items: Vec<SelectionItem> = Vec::new();
        for preset in presets.into_iter() {
            let description = if preset.description.is_empty() {
                None
            } else {
                Some(preset.description.to_string())
            };
            let is_current = preset.model == current_model;
            let single_supported_effort = preset.supported_reasoning_efforts.len() == 1;
            let preset_for_action = preset.clone();
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                let preset_for_event = preset_for_action.clone();
                tx.send(AppEvent::OpenReasoningPopup {
                    model: preset_for_event,
                });
            })];
            items.push(SelectionItem {
                name: preset.display_name.to_string(),
                description,
                is_current,
                actions,
                dismiss_on_select: single_supported_effort,
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select Model and Effort".to_string()),
            subtitle: Some(
                "Access legacy models by running codex -m <model_name> or in your config.toml"
                    .to_string(),
            ),
            footer_hint: Some("Press enter to select reasoning effort, or esc to dismiss.".into()),
            items,
            ..Default::default()
        });
    }

    /// Open the ACP model picker with fetched models.
    #[cfg(feature = "unstable")]
    pub(crate) fn open_acp_model_picker(
        &mut self,
        models: Vec<crate::app_event::AcpModelInfo>,
        current_model_id: Option<String>,
    ) {
        let params = crate::nori::agent_picker::acp_model_picker_params_with_models(
            &models,
            current_model_id.as_deref(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Set the ACP model via the agent handle.
    #[cfg(feature = "unstable")]
    pub(crate) fn set_acp_model(&mut self, model_id: String, display_name: String) {
        if let Some(handle) = self.acp_handle.clone() {
            let app_event_tx = self.app_event_tx.clone();
            let model_id_for_result = model_id.clone();
            let display_name_for_result = display_name.clone();
            tokio::spawn(async move {
                match handle.set_model(model_id).await {
                    Ok(()) => {
                        app_event_tx.send(AppEvent::AcpModelSetResult {
                            success: true,
                            model_id: model_id_for_result,
                            display_name: display_name_for_result,
                            error: None,
                        });
                    }
                    Err(e) => {
                        app_event_tx.send(AppEvent::AcpModelSetResult {
                            success: false,
                            model_id: model_id_for_result,
                            display_name: display_name_for_result,
                            error: Some(e.to_string()),
                        });
                    }
                }
            });
            self.add_info_message(format!("Switching to model: {display_name}..."), None);
        } else {
            self.add_info_message(
                "No ACP agent handle available for model switching".to_string(),
                None,
            );
        }
    }

    /// Open a popup to choose the reasoning effort (stage 2) for the given model.
    pub(crate) fn open_reasoning_popup(&mut self, preset: ModelPreset) {
        let default_effort: ReasoningEffortConfig = preset.default_reasoning_effort;
        let supported = preset.supported_reasoning_efforts;

        let warn_effort = if supported
            .iter()
            .any(|option| option.effort == ReasoningEffortConfig::XHigh)
        {
            Some(ReasoningEffortConfig::XHigh)
        } else if supported
            .iter()
            .any(|option| option.effort == ReasoningEffortConfig::High)
        {
            Some(ReasoningEffortConfig::High)
        } else {
            None
        };
        let warning_text = warn_effort.map(|effort| {
            let effort_label = Self::reasoning_effort_label(effort);
            format!("⚠ {effort_label} reasoning effort can quickly consume Plus plan rate limits.")
        });
        let warn_for_model = preset.model.starts_with("gpt-5.1-codex")
            || preset.model.starts_with("gpt-5.1-codex-max");

        struct EffortChoice {
            stored: Option<ReasoningEffortConfig>,
            display: ReasoningEffortConfig,
        }
        let mut choices: Vec<EffortChoice> = Vec::new();
        for effort in ReasoningEffortConfig::iter() {
            if supported.iter().any(|option| option.effort == effort) {
                choices.push(EffortChoice {
                    stored: Some(effort),
                    display: effort,
                });
            }
        }
        if choices.is_empty() {
            choices.push(EffortChoice {
                stored: Some(default_effort),
                display: default_effort,
            });
        }

        if choices.len() == 1 {
            if let Some(effort) = choices.first().and_then(|c| c.stored) {
                self.apply_model_and_effort(preset.model.to_string(), Some(effort));
            } else {
                self.apply_model_and_effort(preset.model.to_string(), None);
            }
            return;
        }

        let default_choice: Option<ReasoningEffortConfig> = choices
            .iter()
            .any(|choice| choice.stored == Some(default_effort))
            .then_some(Some(default_effort))
            .flatten()
            .or_else(|| choices.iter().find_map(|choice| choice.stored))
            .or(Some(default_effort));

        let model_slug = preset.model.to_string();
        let is_current_model = self.config.model == preset.model;
        let highlight_choice = if is_current_model {
            self.config.model_reasoning_effort
        } else {
            default_choice
        };
        let selection_choice = highlight_choice.or(default_choice);
        let initial_selected_idx = choices
            .iter()
            .position(|choice| choice.stored == selection_choice)
            .or_else(|| {
                selection_choice
                    .and_then(|effort| choices.iter().position(|choice| choice.display == effort))
            });
        let mut items: Vec<SelectionItem> = Vec::new();
        for choice in choices.iter() {
            let effort = choice.display;
            let mut effort_label = Self::reasoning_effort_label(effort).to_string();
            if choice.stored == default_choice {
                effort_label.push_str(" (default)");
            }

            let description = choice
                .stored
                .and_then(|effort| {
                    supported
                        .iter()
                        .find(|option| option.effort == effort)
                        .map(|option| option.description.to_string())
                })
                .filter(|text| !text.is_empty());

            let show_warning = warn_for_model && warn_effort == Some(effort);
            let selected_description = if show_warning {
                warning_text.as_ref().map(|warning_message| {
                    description.as_ref().map_or_else(
                        || warning_message.clone(),
                        |d| format!("{d}\n{warning_message}"),
                    )
                })
            } else {
                None
            };

            let model_for_action = model_slug.clone();
            let effort_for_action = choice.stored;
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
                    cwd: None,
                    approval_policy: None,
                    sandbox_policy: None,
                    model: Some(model_for_action.clone()),
                    effort: Some(effort_for_action),
                    summary: None,
                }));
                tx.send(AppEvent::UpdateModel(model_for_action.clone()));
                tx.send(AppEvent::UpdateReasoningEffort(effort_for_action));
                tx.send(AppEvent::PersistModelSelection {
                    model: model_for_action.clone(),
                    effort: effort_for_action,
                });
                tracing::info!(
                    "Selected model: {}, Selected effort: {}",
                    model_for_action,
                    effort_for_action
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "default".to_string())
                );
            })];

            items.push(SelectionItem {
                name: effort_label,
                description,
                selected_description,
                is_current: is_current_model && choice.stored == highlight_choice,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        let mut header = ColumnRenderable::new();
        header.push(Line::from(
            format!("Select Reasoning Level for {model_slug}").bold(),
        ));

        self.bottom_pane.show_selection_view(SelectionViewParams {
            header: Box::new(header),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            initial_selected_idx,
            ..Default::default()
        });
    }

    fn reasoning_effort_label(effort: ReasoningEffortConfig) -> &'static str {
        match effort {
            ReasoningEffortConfig::None => "None",
            ReasoningEffortConfig::Minimal => "Minimal",
            ReasoningEffortConfig::Low => "Low",
            ReasoningEffortConfig::Medium => "Medium",
            ReasoningEffortConfig::High => "High",
            ReasoningEffortConfig::XHigh => "Extra high",
        }
    }

    fn apply_model_and_effort(&self, model: String, effort: Option<ReasoningEffortConfig>) {
        self.app_event_tx
            .send(AppEvent::CodexOp(Op::OverrideTurnContext {
                cwd: None,
                approval_policy: None,
                sandbox_policy: None,
                model: Some(model.clone()),
                effort: Some(effort),
                summary: None,
            }));
        self.app_event_tx.send(AppEvent::UpdateModel(model.clone()));
        self.app_event_tx
            .send(AppEvent::UpdateReasoningEffort(effort));
        self.app_event_tx.send(AppEvent::PersistModelSelection {
            model: model.clone(),
            effort,
        });
        tracing::info!(
            "Selected model: {}, Selected effort: {}",
            model,
            effort
                .map(|e| e.to_string())
                .unwrap_or_else(|| "default".to_string())
        );
    }

    /// Open a popup to choose the approvals mode (ask for approval policy + sandbox policy).
    pub(crate) fn open_approvals_popup(&mut self) {
        let current_approval = self.config.approval_policy;
        let current_sandbox = self.config.sandbox_policy.clone();
        let mut items: Vec<SelectionItem> = Vec::new();
        let presets: Vec<ApprovalPreset> = builtin_approval_presets();
        for preset in presets.into_iter() {
            let is_current =
                Self::preset_matches_current(current_approval, &current_sandbox, &preset);
            let name = preset.label.to_string();
            let description_text = preset.description;
            let description = Some(description_text.to_string());
            let requires_confirmation = preset.id == "full-access"
                && !self
                    .config
                    .notices
                    .hide_full_access_warning
                    .unwrap_or(false);
            let actions: Vec<SelectionAction> = if requires_confirmation {
                let preset_clone = preset.clone();
                vec![Box::new(move |tx| {
                    tx.send(AppEvent::OpenFullAccessConfirmation {
                        preset: preset_clone.clone(),
                    });
                })]
            } else if preset.id == "auto" {
                #[cfg(target_os = "windows")]
                {
                    if codex_core::get_platform_sandbox().is_none() {
                        let preset_clone = preset.clone();
                        vec![Box::new(move |tx| {
                            tx.send(AppEvent::OpenWindowsSandboxEnablePrompt {
                                preset: preset_clone.clone(),
                            });
                        })]
                    } else if let Some((sample_paths, extra_count, failed_scan)) =
                        self.world_writable_warning_details()
                    {
                        let preset_clone = preset.clone();
                        vec![Box::new(move |tx| {
                            tx.send(AppEvent::OpenWorldWritableWarningConfirmation {
                                preset: Some(preset_clone.clone()),
                                sample_paths: sample_paths.clone(),
                                extra_count,
                                failed_scan,
                            });
                        })]
                    } else {
                        Self::approval_preset_actions(preset.approval, preset.sandbox.clone())
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Self::approval_preset_actions(preset.approval, preset.sandbox.clone())
                }
            } else {
                Self::approval_preset_actions(preset.approval, preset.sandbox.clone())
            };
            items.push(SelectionItem {
                name,
                description,
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select Approval Mode".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(()),
            ..Default::default()
        });
    }

    fn approval_preset_actions(
        approval: AskForApproval,
        sandbox: SandboxPolicy,
    ) -> Vec<SelectionAction> {
        vec![Box::new(move |tx| {
            let sandbox_clone = sandbox.clone();
            tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
                cwd: None,
                approval_policy: Some(approval),
                sandbox_policy: Some(sandbox_clone.clone()),
                model: None,
                effort: None,
                summary: None,
            }));
            tx.send(AppEvent::UpdateAskForApprovalPolicy(approval));
            tx.send(AppEvent::UpdateSandboxPolicy(sandbox_clone));
        })]
    }

    fn preset_matches_current(
        current_approval: AskForApproval,
        current_sandbox: &SandboxPolicy,
        preset: &ApprovalPreset,
    ) -> bool {
        if current_approval != preset.approval {
            return false;
        }
        matches!(
            (&preset.sandbox, current_sandbox),
            (SandboxPolicy::ReadOnly, SandboxPolicy::ReadOnly)
                | (
                    SandboxPolicy::DangerFullAccess,
                    SandboxPolicy::DangerFullAccess
                )
                | (
                    SandboxPolicy::WorkspaceWrite { .. },
                    SandboxPolicy::WorkspaceWrite { .. }
                )
        )
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn world_writable_warning_details(&self) -> Option<(Vec<String>, usize, bool)> {
        if self
            .config
            .notices
            .hide_world_writable_warning
            .unwrap_or(false)
        {
            return None;
        }
        let cwd = self.config.cwd.clone();
        let env_map: std::collections::HashMap<String, String> = std::env::vars().collect();
        match codex_windows_sandbox::apply_world_writable_scan_and_denies(
            self.config.codex_home.as_path(),
            cwd.as_path(),
            &env_map,
            &self.config.sandbox_policy,
            Some(self.config.codex_home.as_path()),
        ) {
            Ok(_) => None,
            Err(_) => Some((Vec::new(), 0, true)),
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code)]
    pub(crate) fn world_writable_warning_details(&self) -> Option<(Vec<String>, usize, bool)> {
        None
    }

    pub(crate) fn open_full_access_confirmation(&mut self, preset: ApprovalPreset) {
        let approval = preset.approval;
        let sandbox = preset.sandbox;
        let mut header_children: Vec<Box<dyn Renderable>> = Vec::new();
        let title_line = Line::from("Enable full access?").bold();
        let info_line = Line::from(vec![
            "When Nori runs with full access, it can edit any file on your computer and run commands with network, without your approval."
                .into(),
            "Exercise caution when enabling full access. This significantly increases the risk of data loss, leaks, or unexpected behavior."
                .fg(Color::Red),
        ]);
        header_children.push(Box::new(title_line));
        header_children.push(Box::new(
            Paragraph::new(vec![info_line]).wrap(Wrap { trim: false }),
        ));
        let header = ColumnRenderable::with(header_children);

        let mut accept_actions = Self::approval_preset_actions(approval, sandbox.clone());
        accept_actions.push(Box::new(|tx| {
            tx.send(AppEvent::UpdateFullAccessWarningAcknowledged(true));
        }));

        let mut accept_and_remember_actions = Self::approval_preset_actions(approval, sandbox);
        accept_and_remember_actions.push(Box::new(|tx| {
            tx.send(AppEvent::UpdateFullAccessWarningAcknowledged(true));
            tx.send(AppEvent::PersistFullAccessWarningAcknowledged);
        }));

        let deny_actions: Vec<SelectionAction> = vec![Box::new(|tx| {
            tx.send(AppEvent::OpenApprovalsPopup);
        })];

        let items = vec![
            SelectionItem {
                name: "Yes, continue anyway".to_string(),
                description: Some("Apply full access for this session".to_string()),
                actions: accept_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Yes, and don't ask again".to_string(),
                description: Some("Enable full access and remember this choice".to_string()),
                actions: accept_and_remember_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Cancel".to_string(),
                description: Some("Go back without enabling full access".to_string()),
                actions: deny_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(header),
            ..Default::default()
        });
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn open_world_writable_warning_confirmation(
        &mut self,
        preset: Option<ApprovalPreset>,
        sample_paths: Vec<String>,
        extra_count: usize,
        failed_scan: bool,
    ) {
        let (approval, sandbox) = match &preset {
            Some(p) => (Some(p.approval), Some(p.sandbox.clone())),
            None => (None, None),
        };
        let mut header_children: Vec<Box<dyn Renderable>> = Vec::new();
        let describe_policy = |policy: &SandboxPolicy| match policy {
            SandboxPolicy::WorkspaceWrite { .. } => "Agent mode",
            SandboxPolicy::ReadOnly => "Read-Only mode",
            _ => "Agent mode",
        };
        let mode_label = preset
            .as_ref()
            .map(|p| describe_policy(&p.sandbox))
            .unwrap_or_else(|| describe_policy(&self.config.sandbox_policy));
        let info_line = if failed_scan {
            Line::from(vec![
                "We couldn't complete the world-writable scan, so protections cannot be verified. "
                    .into(),
                format!("The Windows sandbox cannot guarantee protection in {mode_label}.")
                    .fg(Color::Red),
            ])
        } else {
            Line::from(vec![
                "The Windows sandbox cannot protect writes to folders that are writable by Everyone.".into(),
                " Consider removing write access for Everyone from the following folders:".into(),
            ])
        };
        header_children.push(Box::new(
            Paragraph::new(vec![info_line]).wrap(Wrap { trim: false }),
        ));

        if !sample_paths.is_empty() {
            // Show up to three examples and optionally an "and X more" line.
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(""));
            for p in &sample_paths {
                lines.push(Line::from(format!("  - {p}")));
            }
            if extra_count > 0 {
                lines.push(Line::from(format!("and {extra_count} more")));
            }
            header_children.push(Box::new(Paragraph::new(lines).wrap(Wrap { trim: false })));
        }
        let header = ColumnRenderable::with(header_children);

        // Build actions ensuring acknowledgement happens before applying the new sandbox policy,
        // so downstream policy-change hooks don't re-trigger the warning.
        let mut accept_actions: Vec<SelectionAction> = Vec::new();
        // Suppress the immediate re-scan only when a preset will be applied (i.e., via /approvals),
        // to avoid duplicate warnings from the ensuing policy change.
        if preset.is_some() {
            accept_actions.push(Box::new(|tx| {
                tx.send(AppEvent::SkipNextWorldWritableScan);
            }));
        }
        if let (Some(approval), Some(sandbox)) = (approval, sandbox.clone()) {
            accept_actions.extend(Self::approval_preset_actions(approval, sandbox));
        }

        let mut accept_and_remember_actions: Vec<SelectionAction> = Vec::new();
        accept_and_remember_actions.push(Box::new(|tx| {
            tx.send(AppEvent::UpdateWorldWritableWarningAcknowledged(true));
            tx.send(AppEvent::PersistWorldWritableWarningAcknowledged);
        }));
        if let (Some(approval), Some(sandbox)) = (approval, sandbox) {
            accept_and_remember_actions.extend(Self::approval_preset_actions(approval, sandbox));
        }

        let items = vec![
            SelectionItem {
                name: "Continue".to_string(),
                description: Some(format!("Apply {mode_label} for this session")),
                actions: accept_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Continue and don't warn again".to_string(),
                description: Some(format!("Enable {mode_label} and remember this choice")),
                actions: accept_and_remember_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(header),
            ..Default::default()
        });
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn open_world_writable_warning_confirmation(
        &mut self,
        _preset: Option<ApprovalPreset>,
        _sample_paths: Vec<String>,
        _extra_count: usize,
        _failed_scan: bool,
    ) {
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn open_windows_sandbox_enable_prompt(&mut self, preset: ApprovalPreset) {
        use ratatui_macros::line;

        let mut header = ColumnRenderable::new();
        header.push(*Box::new(
            Paragraph::new(vec![
                line!["Agent mode on Windows uses an experimental sandbox to limit network and filesystem access.".bold()],
                line![
                    "Learn more: https://github.com/tilework-tech/nori-cli/blob/main/docs/windows.md"
                ],
            ])
            .wrap(Wrap { trim: false }),
        ));

        let preset_clone = preset;
        let items = vec![
            SelectionItem {
                name: "Enable experimental sandbox".to_string(),
                description: None,
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::EnableWindowsSandboxForAgentMode {
                        preset: preset_clone.clone(),
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Go back".to_string(),
                description: None,
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::OpenApprovalsPopup);
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: None,
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(header),
            ..Default::default()
        });
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn open_windows_sandbox_enable_prompt(&mut self, _preset: ApprovalPreset) {}

    #[cfg(target_os = "windows")]
    pub(crate) fn maybe_prompt_windows_sandbox_enable(&mut self) {
        if self.config.forced_auto_mode_downgraded_on_windows
            && codex_core::get_platform_sandbox().is_none()
            && let Some(preset) = builtin_approval_presets()
                .into_iter()
                .find(|preset| preset.id == "auto")
        {
            self.open_windows_sandbox_enable_prompt(preset);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn maybe_prompt_windows_sandbox_enable(&mut self) {}

    #[cfg(target_os = "windows")]
    pub(crate) fn clear_forced_auto_mode_downgrade(&mut self) {
        self.config.forced_auto_mode_downgraded_on_windows = false;
    }

    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code)]
    pub(crate) fn clear_forced_auto_mode_downgrade(&mut self) {}

    /// Set the approval policy in the widget's config copy.
    pub(crate) fn set_approval_policy(&mut self, policy: AskForApproval) {
        self.config.approval_policy = policy;
        self.update_approval_mode_label();
    }

    /// Set the sandbox policy in the widget's config copy.
    pub(crate) fn set_sandbox_policy(&mut self, policy: SandboxPolicy) {
        #[cfg(target_os = "windows")]
        let should_clear_downgrade = !matches!(policy, SandboxPolicy::ReadOnly)
            || codex_core::get_platform_sandbox().is_some();

        self.config.sandbox_policy = policy;

        #[cfg(target_os = "windows")]
        if should_clear_downgrade {
            self.config.forced_auto_mode_downgraded_on_windows = false;
        }

        self.update_approval_mode_label();
    }

    /// Update the approval mode label displayed in the footer based on current config.
    fn update_approval_mode_label(&mut self) {
        let label = approval_mode_label(self.config.approval_policy, &self.config.sandbox_policy);
        self.bottom_pane.set_approval_mode_label(label);
    }

    pub(crate) fn set_full_access_warning_acknowledged(&mut self, acknowledged: bool) {
        self.config.notices.hide_full_access_warning = Some(acknowledged);
    }

    pub(crate) fn set_world_writable_warning_acknowledged(&mut self, acknowledged: bool) {
        self.config.notices.hide_world_writable_warning = Some(acknowledged);
    }

    pub(crate) fn set_rate_limit_switch_prompt_hidden(&mut self, hidden: bool) {
        self.config.notices.hide_rate_limit_model_nudge = Some(hidden);
        if hidden {
            self.rate_limit_switch_prompt = RateLimitSwitchPromptState::Idle;
        }
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn world_writable_warning_hidden(&self) -> bool {
        self.config
            .notices
            .hide_world_writable_warning
            .unwrap_or(false)
    }

    /// Set the reasoning effort in the widget's config copy.
    pub(crate) fn set_reasoning_effort(&mut self, effort: Option<ReasoningEffortConfig>) {
        self.config.model_reasoning_effort = effort;
    }

    /// Set the model in the widget's config copy.
    pub(crate) fn set_model(&mut self, model: &str) {
        self.session_header.set_model(model);
        self.config.model = model.to_string();
        // Update the bottom pane's model display name for approval dialogs
        let display_name = crate::nori::agent_picker::get_agent_info(model)
            .map(|info| info.display_name)
            .unwrap_or_else(|| model.to_string());
        self.bottom_pane.set_model_display_name(display_name);
    }

    /// Set the vertical footer layout flag for the TUI.
    pub(crate) fn set_vertical_footer(&mut self, enabled: bool) {
        self.bottom_pane.set_vertical_footer(enabled);
    }

    /// Update the model display name shown in approval dialogs.
    /// Used when ACP model switch completes successfully.
    #[cfg(feature = "unstable")]
    pub(crate) fn update_model_display_name(&mut self, display_name: String) {
        self.bottom_pane.set_model_display_name(display_name);
    }

    pub(crate) fn add_info_message(&mut self, message: String, hint: Option<String>) {
        self.add_to_history(history_cell::new_info_event(message, hint));
        self.request_redraw();
    }

    pub(crate) fn add_plain_history_lines(&mut self, lines: Vec<Line<'static>>) {
        self.add_boxed_history(Box::new(PlainHistoryCell::new(lines)));
        self.request_redraw();
    }

    pub(crate) fn add_error_message(&mut self, message: String) {
        self.add_to_history(history_cell::new_error_event(message));
        self.request_redraw();
    }

    pub(crate) fn add_warning_message(&mut self, message: String) {
        self.add_to_history(history_cell::new_warning_event(message));
        self.request_redraw();
    }

    /// Queue a plain text message to be submitted as a user turn. If no task
    /// is currently running the message is submitted immediately; otherwise
    /// it is appended to the pending queue.
    pub(crate) fn queue_text_as_user_message(&mut self, text: String) {
        self.queue_user_message(UserMessage::from(text));
    }

    /// Show "Connecting to [Agent]" status indicator during agent startup.
    ///
    /// Called when an ACP agent is being spawned and may take time
    /// (e.g., npx/bunx resolving dependencies).
    pub(crate) fn show_connecting_status(&mut self, display_name: &str) {
        let header = format!("Connecting to {display_name}");
        self.bottom_pane.ensure_status_indicator();
        self.bottom_pane.set_interrupt_hint_visible(false); // Can't interrupt during connect
        self.set_status_header(header);
        self.request_redraw();
    }

    /// Handle the /login slash command
    fn handle_login_command(&mut self) {
        // Use pending agent if set (user selected via /agent picker but hasn't submitted yet),
        // otherwise use the current config model
        let model_name = self
            .pending_agent
            .as_ref()
            .map(|p| p.model_name.as_str())
            .unwrap_or(&self.config.model);

        match LoginHandler::check_agent_support(model_name) {
            AgentLoginSupport::Supported {
                agent,
                is_installed,
                login_method,
            } => {
                if !is_installed {
                    // Agent not installed - show installation instructions
                    let display_name = agent.display_name();
                    let npm_package = agent.npm_package();
                    self.add_info_message(
                        format!(
                            "{display_name} is not installed. To install, run:\n\n  npm install -g {npm_package}\n\nThen run /login again to authenticate."
                        ),
                        Some("Install the agent first, then authenticate".to_string()),
                    );
                    return;
                }

                match login_method {
                    LoginMethod::OAuthBrowser => {
                        // Create and start the login handler
                        let mut handler = LoginHandler::new();
                        handler.start_oauth();

                        // Show auth method selection message
                        self.add_info_message(
                            "Starting authentication...\n\nA browser window will open for you to sign in with your OpenAI account.\n\nAlternatively, you can set the OPENAI_API_KEY environment variable.".to_string(),
                            Some("Press Esc to cancel".to_string()),
                        );

                        // Start the actual login server
                        self.start_oauth_login_flow(handler);
                    }
                    LoginMethod::ExternalCli { command, args } => {
                        // Create and start the login handler
                        let mut handler = LoginHandler::new();
                        let agent_display_name = agent.display_name().to_string();
                        handler.start_external_cli(agent_display_name.clone());

                        // Show starting message
                        self.add_info_message(
                            format!(
                                "Starting authentication for {agent_display_name}...\n\nThe {agent_display_name} login process will run in-app.",
                            ),
                            Some("Press Esc to cancel".to_string()),
                        );

                        // Start the external CLI login flow
                        self.start_external_cli_login_flow(
                            handler,
                            command,
                            args,
                            agent_display_name,
                        );
                    }
                }
            }
            AgentLoginSupport::NotSupported { agent_name } => {
                // Provide agent-specific instructions
                let instructions = match agent_name.as_str() {
                    "Claude Code" => {
                        "In-app login for Claude Code is not yet supported.\n\n\
                         To authenticate, run `claude` in a separate terminal and use the /login command.\n\n\
                         Alternatively, set the ANTHROPIC_API_KEY environment variable."
                    }
                    _ => {
                        "In-app login for this agent is not yet supported. Please authenticate externally using the agent's native login command or API keys."
                    }
                };
                self.add_info_message(instructions.to_string(), None);
            }
            AgentLoginSupport::Unknown { model_name } => {
                self.add_info_message(
                    format!("Unknown agent '{model_name}'. Cannot determine login method."),
                    None,
                );
            }
        }
    }

    /// Handle the /login <agent> command with explicit agent name
    fn handle_login_command_with_agent(&mut self, agent_name: &str) {
        match LoginHandler::check_agent_support(agent_name) {
            AgentLoginSupport::Supported {
                agent,
                is_installed,
                login_method,
            } => {
                if !is_installed {
                    let display_name = agent.display_name();
                    let npm_package = agent.npm_package();
                    self.add_info_message(
                        format!(
                            "{display_name} is not installed. To install, run:\n\n  npm install -g {npm_package}\n\nThen run /login again to authenticate."
                        ),
                        Some("Install the agent first, then authenticate".to_string()),
                    );
                    return;
                }

                match login_method {
                    LoginMethod::OAuthBrowser => {
                        let mut handler = LoginHandler::new();
                        handler.start_oauth();

                        self.add_info_message(
                            "Starting authentication...\n\nA browser window will open for you to sign in with your OpenAI account.\n\nAlternatively, you can set the OPENAI_API_KEY environment variable.".to_string(),
                            Some("Press Esc to cancel".to_string()),
                        );

                        self.start_oauth_login_flow(handler);
                    }
                    LoginMethod::ExternalCli { command, args } => {
                        let mut handler = LoginHandler::new();
                        let agent_display_name = agent.display_name().to_string();
                        handler.start_external_cli(agent_display_name.clone());

                        self.add_info_message(
                            format!(
                                "Starting authentication for {agent_display_name}...\n\nThe {agent_display_name} login process will run in-app.",
                            ),
                            Some("Press Esc to cancel".to_string()),
                        );

                        self.start_external_cli_login_flow(
                            handler,
                            command,
                            args,
                            agent_display_name,
                        );
                    }
                }
            }
            AgentLoginSupport::NotSupported { agent_name } => {
                let instructions = match agent_name.as_str() {
                    "Claude Code" => {
                        "In-app login for Claude Code is not yet supported.\n\n\
                         To authenticate, run `claude` in a separate terminal and use the /login command.\n\n\
                         Alternatively, set the ANTHROPIC_API_KEY environment variable."
                    }
                    _ => {
                        "In-app login for this agent is not yet supported. Please authenticate externally using the agent's native login command or API keys."
                    }
                };
                self.add_info_message(instructions.to_string(), None);
            }
            AgentLoginSupport::Unknown { model_name } => {
                self.add_info_message(
                    format!("Unknown agent '{model_name}'. Cannot determine login method."),
                    None,
                );
            }
        }
    }

    /// Start the OAuth login flow
    fn start_oauth_login_flow(&mut self, mut handler: LoginHandler) {
        use codex_core::auth::CLIENT_ID;
        use codex_login::ServerOptions;
        use codex_login::run_login_server;

        let opts = ServerOptions::new(
            self.config.codex_home.clone(),
            CLIENT_ID.to_string(),
            None, // No forced workspace ID
            self.config.cli_auth_credentials_store_mode,
        );

        match run_login_server(opts) {
            Ok(child) => {
                let auth_url = child.auth_url.clone();
                handler.set_shutdown_handle(child.cancel_handle());

                // Store the handler
                self.login_handler = Some(handler);

                // Update the info message with the URL
                self.add_info_message(
                    format!(
                        "Opening browser for authentication...\n\nIf the browser doesn't open automatically, visit:\n{auth_url}\n\nWaiting for authentication to complete..."
                    ),
                    Some("Press Esc to cancel".to_string()),
                );

                // Spawn a task to wait for completion
                let app_event_tx = self.app_event_tx.clone();
                let auth_manager = self.auth_manager.clone();
                tokio::spawn(async move {
                    match child.block_until_done().await {
                        Ok(()) => {
                            auth_manager.reload();
                            app_event_tx.send(AppEvent::LoginComplete { success: true });
                        }
                        Err(e) => {
                            tracing::error!("OAuth login failed: {e}");
                            app_event_tx.send(AppEvent::LoginComplete { success: false });
                        }
                    }
                });
            }
            Err(e) => {
                self.add_error_message(format!("Failed to start login server: {e}"));
            }
        }
    }

    /// Start the external CLI login flow (e.g., gemini login)
    #[cfg(feature = "login")]
    fn start_external_cli_login_flow(
        &mut self,
        mut handler: LoginHandler,
        command: String,
        args: Vec<String>,
        agent_display_name: String,
    ) {
        use std::collections::HashMap;

        let app_event_tx = self.app_event_tx.clone();
        let cwd = self.config.cwd.clone();

        // Spawn the PTY process and stream output
        let task_handle = tokio::spawn(async move {
            // Build environment - inherit current environment
            let mut env: HashMap<String, String> = std::env::vars().collect();
            // Ensure TERM is set for proper terminal behavior
            env.entry("TERM".to_string())
                .or_insert_with(|| "xterm-256color".to_string());

            match codex_utils_pty::spawn_pty_process(&command, &args, &cwd, &env, &None).await {
                Ok(spawned) => {
                    // Keep session alive so process keeps running
                    let _session = spawned.session;
                    let mut output_rx = spawned.output_rx;
                    let exit_rx = spawned.exit_rx;

                    // Spawn a task to stream output
                    let output_event_tx = app_event_tx.clone();
                    let output_task = tokio::spawn(async move {
                        loop {
                            match output_rx.recv().await {
                                Ok(data) => {
                                    // Convert bytes to string, stripping invalid UTF-8
                                    let text = String::from_utf8_lossy(&data);
                                    // Strip ANSI escape codes using a simple regex-like approach
                                    let stripped = strip_ansi_codes(&text);
                                    if !stripped.is_empty() {
                                        output_event_tx.send(AppEvent::ExternalCliLoginOutput {
                                            data: stripped,
                                        });
                                    }
                                }
                                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                    // Receiver lagged, continue
                                    continue;
                                }
                            }
                        }
                    });

                    // Wait for process exit
                    let exit_code = exit_rx.await.unwrap_or(-1);

                    // Cancel output task
                    output_task.abort();

                    // Send completion event
                    let success = exit_code == 0;
                    app_event_tx.send(AppEvent::ExternalCliLoginComplete {
                        success,
                        agent_name: agent_display_name,
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to spawn external CLI login: {e}");
                    app_event_tx.send(AppEvent::ExternalCliLoginComplete {
                        success: false,
                        agent_name: agent_display_name,
                    });
                }
            }
        });

        // Store the task handle for cancellation support
        handler.set_pty_task_handle(task_handle);
        self.login_handler = Some(handler);
    }

    /// Start the external CLI login flow (stub for non-login builds)
    #[cfg(not(feature = "login"))]
    fn start_external_cli_login_flow(
        &mut self,
        _handler: LoginHandler,
        _command: String,
        _args: Vec<String>,
        _agent_display_name: String,
    ) {
        self.add_error_message(
            "Login feature is not enabled. Rebuild with --features login".to_string(),
        );
    }

    /// Handle login completion event
    pub(crate) fn handle_login_complete(&mut self, success: bool) {
        if let Some(mut handler) = self.login_handler.take() {
            if success {
                handler.oauth_complete();
                self.add_info_message(
                    "Successfully authenticated with OpenAI!\n\nYou can now use Codex.".to_string(),
                    None,
                );
            } else {
                handler.cancel();
                self.add_info_message("Login cancelled or failed.".to_string(), None);
            }
        }
        self.request_redraw();
    }

    /// Handle external CLI login output (streaming text from the PTY process)
    pub(crate) fn handle_external_cli_login_output(&mut self, data: String) {
        // Display the output as an info message (append to existing or create new)
        self.add_info_message(data, None);
        self.request_redraw();
    }

    /// Handle external CLI login completion
    pub(crate) fn handle_external_cli_login_complete(&mut self, success: bool, agent_name: String) {
        if let Some(mut handler) = self.login_handler.take() {
            handler.cancel(); // Clear any handler state
        }

        if success {
            self.add_info_message(
                format!(
                    "Successfully authenticated with {agent_name}!\n\nYou can now use {agent_name}."
                ),
                None,
            );
        } else {
            self.add_info_message(format!("{agent_name} login failed or was cancelled."), None);
        }
        self.request_redraw();
    }

    pub(crate) fn add_mcp_output(&mut self) {
        if self.config.mcp_servers.is_empty() {
            self.add_to_history(history_cell::empty_mcp_output());
        } else {
            self.submit_op(Op::ListMcpTools);
        }
    }

    /// Forward file-search results to the bottom pane.
    pub(crate) fn apply_file_search_result(&mut self, query: String, matches: Vec<FileMatch>) {
        self.bottom_pane.on_file_search_result(query, matches);
    }

    /// Update system info in the footer (for background refresh).
    pub(crate) fn apply_system_info_refresh(&mut self, info: crate::system_info::SystemInfo) {
        self.bottom_pane.set_system_info(info);
    }

    /// Handle Ctrl-C key press.
    fn on_ctrl_c(&mut self) {
        if self.bottom_pane.on_ctrl_c() == CancellationEvent::Handled {
            return;
        }

        if self.bottom_pane.is_task_running() {
            self.bottom_pane.show_ctrl_c_quit_hint();
            self.submit_op(Op::Interrupt);
            return;
        }

        self.submit_op(Op::Shutdown);
    }

    pub(crate) fn composer_text(&self) -> String {
        self.bottom_pane.composer_text()
    }

    /// Returns the first prompt text for this session, used for transcript matching.
    pub(crate) fn first_prompt_text(&self) -> Option<String> {
        self.first_prompt_text.clone()
    }

    /// Returns true if a popup or custom view is currently active in the bottom pane.
    pub(crate) fn has_active_popup(&self) -> bool {
        self.bottom_pane.has_active_view()
    }

    pub(crate) fn composer_is_empty(&self) -> bool {
        self.bottom_pane.composer_is_empty()
    }

    /// True when the UI is in the regular composer state with no running task,
    /// no modal overlay (e.g. approvals or status indicator), and no composer popups.
    /// In this state Esc-Esc backtracking is enabled.
    pub(crate) fn is_normal_backtrack_mode(&self) -> bool {
        self.bottom_pane.is_normal_backtrack_mode()
    }

    pub(crate) fn insert_str(&mut self, text: &str) {
        self.bottom_pane.insert_str(text);
    }

    /// Replace the composer content with the provided text and reset cursor.
    pub(crate) fn set_composer_text(&mut self, text: String) {
        self.bottom_pane.set_composer_text(text);
    }

    pub(crate) fn show_esc_backtrack_hint(&mut self) {
        self.bottom_pane.show_esc_backtrack_hint();
    }

    pub(crate) fn clear_esc_backtrack_hint(&mut self) {
        self.bottom_pane.clear_esc_backtrack_hint();
    }
    /// Forward an `Op` directly to codex.
    pub(crate) fn submit_op(&self, op: Op) {
        // Record outbound operation for session replay fidelity.
        crate::session_log::log_outbound_op(&op);
        if let Err(e) = self.codex_op_tx.send(op) {
            tracing::error!("failed to submit op: {e}");
        }
    }

    fn on_list_mcp_tools(&mut self, ev: McpListToolsResponseEvent) {
        self.add_to_history(history_cell::new_mcp_tools_output(
            &self.config,
            ev.tools,
            ev.resources,
            ev.resource_templates,
            &ev.auth_statuses,
        ));
    }

    fn on_list_custom_prompts(&mut self, ev: ListCustomPromptsResponseEvent) {
        let len = ev.custom_prompts.len();
        debug!("received {len} custom prompts");
        // Forward to bottom pane so the slash popup can show them now.
        self.bottom_pane.set_custom_prompts(ev.custom_prompts);
    }

    pub(crate) fn token_usage(&self) -> TokenUsage {
        self.token_info
            .as_ref()
            .map(|ti| ti.total_token_usage.clone())
            .unwrap_or_default()
    }

    pub(crate) fn conversation_id(&self) -> Option<ConversationId> {
        self.conversation_id
    }

    pub(crate) fn rollout_path(&self) -> Option<PathBuf> {
        self.current_rollout_path.clone()
    }

    /// Return a reference to the widget's current config (includes any
    /// runtime overrides applied via TUI, e.g., model or approval policy).
    pub(crate) fn config_ref(&self) -> &Config {
        &self.config
    }

    /// Get a reference to the session statistics tracker.
    pub(crate) fn session_stats(&self) -> &SessionStats {
        &self.session_stats
    }

    pub(crate) fn clear_token_usage(&mut self) {
        self.token_info = None;
    }

    fn as_renderable(&self) -> RenderableItem<'_> {
        let active_cell_renderable = match &self.active_cell {
            Some(cell) => RenderableItem::Borrowed(cell).inset(Insets::tlbr(1, 0, 0, 0)),
            None => RenderableItem::Owned(Box::new(())),
        };
        let mut flex = FlexRenderable::new();
        flex.push(1, active_cell_renderable);
        flex.push(
            0,
            RenderableItem::Borrowed(&self.bottom_pane).inset(Insets::tlbr(1, 0, 0, 0)),
        );
        RenderableItem::Owned(Box::new(flex))
    }
}

impl Drop for ChatWidget {
    fn drop(&mut self) {
        self.stop_rate_limit_poller();
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
