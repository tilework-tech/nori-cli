use super::*;
use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::tui::FrameRequester;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use nori_config::NoriConfig as Config;
use std::path::PathBuf;
use tokio::sync::mpsc::unbounded_channel;

fn test_config() -> Config {
    Config {
        cwd: std::env::current_dir().expect("current directory"),
        ..Config::default()
    }
}

fn drain_insert_history(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Vec<Vec<ratatui::text::Line<'static>>> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            let mut lines = cell.display_lines(80);
            if !cell.is_stream_continuation() && !out.is_empty() && !lines.is_empty() {
                lines.insert(0, "".into());
            }
            out.push(lines);
        }
    }
    out
}

fn lines_to_single_string(lines: &[ratatui::text::Line<'static>]) -> String {
    let mut text = String::new();
    for line in lines {
        for span in &line.spans {
            text.push_str(&span.content);
        }
        text.push('\n');
    }
    text
}

pub(crate) fn make_chatwidget_manual() -> (
    ChatWidget,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    make_chatwidget_manual_with_footer_config(
        nori_config::FooterSegmentConfig::default(),
        nori_config::FooterLayoutConfig::default(),
    )
}

pub(crate) fn make_chatwidget_manual_with_footer_layout(
    footer_layout_config: nori_config::FooterLayoutConfig,
) -> (
    ChatWidget,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    make_chatwidget_manual_with_footer_config(
        nori_config::FooterSegmentConfig::default(),
        footer_layout_config,
    )
}

pub(crate) fn make_chatwidget_manual_with_footer_config(
    footer_segment_config: nori_config::FooterSegmentConfig,
    footer_layout_config: nori_config::FooterLayoutConfig,
) -> (
    ChatWidget,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    make_chatwidget_manual_for_mode(
        footer_segment_config,
        footer_layout_config,
        false,
        crate::answer_guard::Clock::system(),
    )
}

/// A chat widget whose request prompts read `clock`.
pub(crate) fn make_chatwidget_with_clock(
    clock: crate::answer_guard::Clock,
) -> (
    ChatWidget,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    make_chatwidget_manual_for_mode(
        nori_config::FooterSegmentConfig::default(),
        nori_config::FooterLayoutConfig::default(),
        false,
        clock,
    )
}

pub(crate) fn make_cloud_chatwidget_manual() -> (
    ChatWidget,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    make_chatwidget_manual_for_mode(
        nori_config::FooterSegmentConfig::default(),
        nori_config::FooterLayoutConfig::default(),
        true,
        crate::answer_guard::Clock::system(),
    )
}

fn make_chatwidget_manual_for_mode(
    footer_segment_config: nori_config::FooterSegmentConfig,
    footer_layout_config: nori_config::FooterLayoutConfig,
    cloud_mode: bool,
    clock: crate::answer_guard::Clock,
) -> (
    ChatWidget,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    let (event_tx, event_rx) = unbounded_channel();
    let app_event_tx = AppEventSender::new(event_tx);
    let config = test_config();
    let mut widget = ChatWidget::new(ChatWidgetInit {
        config,
        frame_requester: FrameRequester::test_dummy(),
        app_event_tx: app_event_tx.clone(),
        initial_prompt: None,
        initial_images: Vec::new(),
        enhanced_keys_supported: false,
        auth_manager: AuthManager::from_auth_for_testing(CodexAuth::from_api_key("test")),
        vertical_footer: false,
        footer_segment_config,
        footer_layout_config,
        deferred_spawn: true,
        fork_context: None,
        prepared_agent: None,
        cloud_mode,
        analytics: None,
        session_manager: Some(nori_harness::session_manager::SessionManager::new(
            nori_harness::session_manager::SessionManagerConfig::default(),
        )),
        clock,
    });
    // Every widget tags its history with its session; these tests read the
    // untagged events, as the active session's are once the app routes them.
    widget.app_event_tx = app_event_tx;
    let (_unused_tx, unused_rx) = unbounded_channel();
    (widget, event_rx, unused_rx)
}

mod action_failures;
mod agent_status;
mod api_key;
mod approvals;
mod close_refusal;
mod context;
mod delegation;
mod explore_details;
mod history_host;
mod host_feed;
mod host_keys;
mod host_role;
mod new_session_folder;
mod part10;
mod part8;
mod program_status;
mod queue_host;
mod queue_list;
mod queue_recall;
mod session_title_envelope;
mod skillsets_v2;
mod terminal_title;
mod thought_history;
mod thought_status;

/// Type `text` as key presses and let the composer settle them.
pub(crate) fn type_text(chat: &mut ChatWidget, text: &str) {
    for c in text.chars() {
        chat.handle_key_event(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    std::thread::sleep(
        crate::bottom_pane::ChatComposer::recommended_paste_flush_delay()
            + std::time::Duration::from_millis(5),
    );
    chat.bottom_pane.flush_paste_burst_if_due();
}

pub(crate) fn make_scoped_chatwidget_manual() -> (
    ChatWidget,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    let (mut widget, rx, op_rx) = make_chatwidget_manual();
    widget.app_event_tx = widget.app_event_tx.for_session(widget.session_generation());
    (widget, rx, op_rx)
}
