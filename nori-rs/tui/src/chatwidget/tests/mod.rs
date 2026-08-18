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
    make_chatwidget_manual_for_mode(footer_segment_config, footer_layout_config, false)
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
    )
}

fn make_chatwidget_manual_for_mode(
    footer_segment_config: nori_config::FooterSegmentConfig,
    footer_layout_config: nori_config::FooterLayoutConfig,
    cloud_mode: bool,
) -> (
    ChatWidget,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    let (event_tx, event_rx) = unbounded_channel();
    let app_event_tx = AppEventSender::new(event_tx);
    let config = test_config();
    let widget = ChatWidget::new(ChatWidgetInit {
        config,
        frame_requester: FrameRequester::test_dummy(),
        app_event_tx,
        initial_prompt: None,
        initial_images: Vec::new(),
        enhanced_keys_supported: false,
        auth_manager: AuthManager::from_auth_for_testing(CodexAuth::from_api_key("test")),
        vertical_footer: false,
        footer_segment_config,
        footer_layout_config,
        deferred_spawn: true,
        fork_context: None,
        cloud_mode,
    });
    let (_unused_tx, unused_rx) = unbounded_channel();
    (widget, event_rx, unused_rx)
}

mod part10;
mod part8;
