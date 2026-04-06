use super::*;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::effective_cwd_tracker::EffectiveCwdTracker;
use crate::test_backend::VT100Backend;
use crate::tui::FrameRequester;
use assert_matches::assert_matches;
use codex_common::approval_presets::builtin_approval_presets;

use codex_core::AuthManager;
use codex_core::CodexAuth;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::config::ConfigToml;
use codex_core::protocol::AgentMessageDeltaEvent;
use codex_core::protocol::AgentMessageEvent;
use codex_core::protocol::AgentReasoningDeltaEvent;
use codex_core::protocol::AgentReasoningEvent;
use codex_core::protocol::ApplyPatchApprovalRequestEvent;
use codex_core::protocol::BackgroundEventEvent;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::ExecApprovalRequestEvent;
use codex_core::protocol::ExecCommandBeginEvent;
use codex_core::protocol::ExecCommandEndEvent;
use codex_core::protocol::ExecCommandSource;
use codex_core::protocol::FileChange;
use codex_core::protocol::Op;
use codex_core::protocol::PatchApplyBeginEvent;
use codex_core::protocol::PatchApplyEndEvent;
use codex_core::protocol::StreamErrorEvent;
use codex_core::protocol::TaskCompleteEvent;
use codex_core::protocol::TaskStartedEvent;
use codex_core::protocol::TokenCountEvent;
use codex_core::protocol::TokenUsage;
use codex_core::protocol::TokenUsageInfo;
use codex_core::protocol::UndoCompletedEvent;
use codex_core::protocol::UndoStartedEvent;
use codex_core::protocol::ViewImageToolCallEvent;
use codex_core::protocol::WarningEvent;
use codex_protocol::ConversationId;
use codex_protocol::parse_command::ParsedCommand;
use codex_protocol::plan_tool::PlanItemArg;
use codex_protocol::plan_tool::StepStatus;
use codex_protocol::plan_tool::UpdatePlanArgs;
use codex_protocol::protocol::CodexErrorInfo;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use insta::assert_snapshot;
use std::collections::HashSet;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tempfile::tempdir;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::unbounded_channel;

#[cfg(target_os = "windows")]
fn set_windows_sandbox_enabled(enabled: bool) {
    codex_core::set_windows_sandbox_enabled(enabled);
}

fn test_config() -> Config {
    // Use base defaults to avoid depending on host state.
    Config::load_from_base_config_with_overrides(
        ConfigToml::default(),
        ConfigOverrides::default(),
        std::env::temp_dir(),
    )
    .expect("config")
}

fn drain_insert_history(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Vec<Vec<ratatui::text::Line<'static>>> {
    let mut out = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = ev {
            let mut lines = cell.display_lines(80);
            if !cell.is_stream_continuation() && !out.is_empty() && !lines.is_empty() {
                lines.insert(0, "".into());
            }
            out.push(lines)
        }
    }
    out
}

fn lines_to_single_string(lines: &[ratatui::text::Line<'static>]) -> String {
    let mut s = String::new();
    for line in lines {
        for span in &line.spans {
            s.push_str(&span.content);
        }
        s.push('\n');
    }
    s
}

fn make_token_info(total_tokens: i64, context_window: i64) -> TokenUsageInfo {
    fn usage(total_tokens: i64) -> TokenUsage {
        TokenUsage {
            total_tokens,
            ..TokenUsage::default()
        }
    }

    TokenUsageInfo {
        total_token_usage: usage(total_tokens),
        last_token_usage: usage(total_tokens),
        model_context_window: Some(context_window),
    }
}

fn begin_exec_with_source(
    chat: &mut ChatWidget,
    call_id: &str,
    raw_cmd: &str,
    source: ExecCommandSource,
) -> ExecCommandBeginEvent {
    // Build the full command vec and parse it using core's parser,
    // then convert to protocol variants for the event payload.
    let command = vec!["bash".to_string(), "-lc".to_string(), raw_cmd.to_string()];
    let parsed_cmd: Vec<ParsedCommand> = codex_core::parse_command::parse_command(&command);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let interaction_input = None;
    let event = ExecCommandBeginEvent {
        call_id: call_id.to_string(),
        process_id: None,
        turn_id: "turn-1".to_string(),
        command,
        cwd,
        parsed_cmd,
        source,
        interaction_input,
    };
    chat.handle_codex_event(Event {
        id: call_id.to_string(),
        msg: EventMsg::ExecCommandBegin(event.clone()),
    });
    event
}

fn begin_exec(chat: &mut ChatWidget, call_id: &str, raw_cmd: &str) -> ExecCommandBeginEvent {
    begin_exec_with_source(chat, call_id, raw_cmd, ExecCommandSource::Agent)
}

fn end_exec(
    chat: &mut ChatWidget,
    begin_event: ExecCommandBeginEvent,
    stdout: &str,
    stderr: &str,
    exit_code: i32,
) {
    let aggregated = if stderr.is_empty() {
        stdout.to_string()
    } else {
        format!("{stdout}{stderr}")
    };
    let ExecCommandBeginEvent {
        call_id,
        turn_id,
        command,
        cwd,
        parsed_cmd,
        source,
        interaction_input,
        process_id,
    } = begin_event;
    chat.handle_codex_event(Event {
        id: call_id.clone(),
        msg: EventMsg::ExecCommandEnd(ExecCommandEndEvent {
            call_id,
            process_id,
            turn_id,
            command,
            cwd,
            parsed_cmd,
            source,
            interaction_input,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            aggregated_output: aggregated.clone(),
            exit_code,
            duration: std::time::Duration::from_millis(5),
            formatted_output: aggregated,
        }),
    });
}

fn active_blob(chat: &ChatWidget) -> String {
    let lines = chat
        .active_cell
        .as_ref()
        .expect("active cell present")
        .display_lines(80);
    lines_to_single_string(&lines)
}

fn render_bottom_popup(chat: &ChatWidget, width: u16) -> String {
    let height = chat.desired_height(width);
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    chat.render(area, &mut buf);

    let mut lines: Vec<String> = (0..area.height)
        .map(|row| {
            let mut line = String::new();
            for col in 0..area.width {
                let symbol = buf[(area.x + col, area.y + row)].symbol();
                if symbol.is_empty() {
                    line.push(' ');
                } else {
                    line.push_str(symbol);
                }
            }
            line.trim_end().to_string()
        })
        .collect();

    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    lines.join("\n")
}

/// Helper to drain all RefreshSystemInfoForDirectory events from the channel.
fn drain_refresh_system_info_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let AppEvent::RefreshSystemInfoForDirectory { dir, agent: _ } = ev {
            dirs.push(dir);
        }
    }
    dirs
}

pub(crate) fn make_chatwidget_manual() -> (
    ChatWidget,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<Op>,
) {
    let (tx_raw, rx) = unbounded_channel::<AppEvent>();
    let app_event_tx = AppEventSender::new(tx_raw);
    let (op_tx, op_rx) = unbounded_channel::<Op>();
    let cfg = test_config();
    let bottom = BottomPane::new(BottomPaneParams {
        app_event_tx: app_event_tx.clone(),
        frame_requester: FrameRequester::test_dummy(),
        has_input_focus: true,
        enhanced_keys_supported: false,
        placeholder_text: "Ask Nori to do anything".to_string(),
        disable_paste_burst: false,
        animations_enabled: cfg.animations,
        vertical_footer: false,
        footer_segment_config: codex_acp::config::FooterSegmentConfig::default(),
        agent_display_name: String::new(),
        agent_slug: String::new(),
    });
    let auth_manager = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("test"));
    let widget = ChatWidget {
        app_event_tx,
        codex_op_tx: op_tx,
        bottom_pane: bottom,
        active_cell: None,
        config: cfg.clone(),
        auth_manager,
        session_header: SessionHeader::new(cfg.model),
        initial_user_message: None,
        token_info: None,
        rate_limit_snapshot: None,
        rate_limit_warnings: RateLimitWarningState::default(),
        rate_limit_poller: None,
        stream_controller: None,
        running_commands: HashMap::new(),
        suppressed_exec_calls: HashSet::new(),
        completed_client_tool_calls: HashSet::new(),
        last_unified_wait: None,
        task_complete_pending: false,
        mcp_startup_status: None,
        interrupts: InterruptManager::new(),
        reasoning_buffer: String::new(),
        full_reasoning_buffer: String::new(),
        current_status_header: String::from("Thinking really hard"),
        retry_status_header: None,
        conversation_id: None,
        frame_requester: FrameRequester::test_dummy(),
        show_welcome_banner: true,
        queued_user_messages: VecDeque::new(),
        suppress_session_configured_redraw: false,
        pending_notification: None,
        needs_final_message_separator: false,
        last_rendered_width: std::cell::Cell::new(None),
        current_rollout_path: None,
        pending_exec_cells: PendingExecCellTracker::new(),
        pending_client_tool_cells: HashMap::new(),
        effective_cwd_tracker: EffectiveCwdTracker::with_initial_cwd(cfg.cwd),
        pending_agent: None,
        expected_agent: None,
        session_configured_received: false,
        #[cfg(feature = "unstable")]
        acp_handle: None,
        session_stats: crate::session_stats::SessionStats::new(),
        login_handler: None,
        first_prompt_text: None,
        loop_remaining: None,
        loop_total: None,
        #[cfg(feature = "nori-config")]
        loop_count_override: None,
        turn_finished: false,
        plan_drawer_mode: PlanDrawerMode::Off,
        pinned_plan: None,
        terminal_title_animation_origin: std::time::Instant::now(),
        last_terminal_title: None,
    };
    (widget, rx, op_rx)
}

pub(crate) fn make_chatwidget_manual_with_sender() -> (
    ChatWidget,
    AppEventSender,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<Op>,
) {
    let (widget, rx, op_rx) = make_chatwidget_manual();
    let app_event_tx = widget.app_event_tx.clone();
    (widget, app_event_tx, rx, op_rx)
}

mod mod_tests;
mod part1;
mod part2;
mod part3;
mod part4;
mod part5;
mod part6;
mod part7;
