//! Agent configuration state: what reaches history, the status views, and the
//! `/config` picker as the agent advertises and changes its configuration.

use super::*;

use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;

use crate::history_cell::HistoryCell;
use crate::nori::session_header::DisplayMode;
use crate::nori::session_header::NoriSessionHeaderCell;

fn select(
    id: &str,
    name: &str,
    current: &str,
    values: &[(&str, &str)],
) -> acp::SessionConfigOption {
    acp::SessionConfigOption::select(
        id.to_string(),
        name.to_string(),
        current.to_string(),
        values
            .iter()
            .map(|(value, label)| {
                acp::SessionConfigSelectOption::new(value.to_string(), label.to_string())
            })
            .collect::<Vec<_>>(),
    )
}

fn claude_options(model: &str) -> Vec<acp::SessionConfigOption> {
    vec![
        select(
            "mode",
            "Mode",
            "plan",
            &[("plan", "Plan"), ("build", "Build")],
        )
        .category(acp::SessionConfigOptionCategory::Mode),
        select(
            "model",
            "Model",
            model,
            &[("opus-5", "Opus 5"), ("sonnet-5", "Sonnet 5")],
        )
        .category(acp::SessionConfigOptionCategory::Model),
        select("effort", "Effort", "xhigh", &[("xhigh", "xhigh")])
            .category(acp::SessionConfigOptionCategory::ThoughtLevel),
    ]
}

/// The most recently inserted history cell, kept as a cell so a test can
/// re-render it after later state changes.
fn last_history_cell(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Option<Box<dyn HistoryCell>> {
    let mut last = None;
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            last = Some(cell);
        }
    }
    last
}

fn history_text(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    drain_insert_history(rx)
        .iter()
        .map(|cell| lines_to_single_string(cell))
        .collect()
}

fn agent_row(chat: &ChatWidget, display_mode: DisplayMode) -> String {
    let cell =
        NoriSessionHeaderCell::new(chat.live_status_view_model()).with_display_mode(display_mode);
    lines_to_single_string(&cell.display_lines(80))
        .lines()
        .find(|line| line.trim_start().starts_with("Agent"))
        .unwrap_or_default()
        .to_string()
}

#[test]
fn advertised_configuration_reaches_the_status_views() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    // The welcome card is written before the agent advertises anything, so it
    // holds the live handle and must not guess at that point.
    let welcome = NoriSessionHeaderCell::new(chat.live_status_view_model())
        .with_display_mode(DisplayMode::Compact);
    assert!(
        lines_to_single_string(&welcome.display_lines(80)).contains("Agent        Claude\n"),
        "before configuration the row is the provider name alone"
    );

    chat.handle_acp_session_config_update(&claude_options("opus-5"));

    assert!(
        lines_to_single_string(&welcome.display_lines(80))
            .contains("Agent        Claude · Opus 5 · xhigh · Plan"),
        "the welcome card fills in once the agent advertises its configuration"
    );
}

#[test]
fn status_output_is_detached_from_later_configuration_changes() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_acp_session_config_update(&claude_options("opus-5"));
    while rx.try_recv().is_ok() {}

    chat.add_status_output();
    let printed = last_history_cell(&mut rx).expect("status output");
    assert!(
        lines_to_single_string(&printed.display_lines(80)).contains("Model   Opus 5"),
        "status prints the configuration as it stood"
    );

    chat.handle_acp_session_config_update(&claude_options("sonnet-5"));

    assert!(
        lines_to_single_string(&printed.display_lines(80)).contains("Model   Opus 5"),
        "printed /status output must not change after the fact"
    );
    assert!(
        agent_row(&chat, DisplayMode::Compact).contains("Sonnet 5"),
        "later cards do show the new value"
    );
}

#[test]
fn configuration_history_announces_the_initial_set_then_only_changes() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_acp_session_config_update(&claude_options("opus-5"));
    let announced = history_text(&mut rx);
    assert!(
        announced.contains("Claude Code options: Mode=Plan, Model=Opus 5, Effort=xhigh"),
        "the first announcement lists every advertised option, got:\n{announced}"
    );
    assert!(announced.contains("(/config to change)"));

    chat.handle_acp_session_config_update(&claude_options("sonnet-5"));
    let changed = history_text(&mut rx);
    assert_eq!(
        changed.trim_end(),
        "• Claude Code option updated: Model=Sonnet 5",
        "later announcements report only what changed"
    );
}

#[test]
fn successful_mutations_and_snapshots_update_the_tracked_configuration() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    // The options a successful `set_session_config_option` echoes back.
    chat.sync_acp_session_config_snapshot(&claude_options("sonnet-5"));
    assert!(agent_row(&chat, DisplayMode::Compact).contains("Sonnet 5"));

    // A stale generation must not clobber the tracked configuration.
    chat.handle_acp_session_config_snapshot(
        chat.acp_mode_config_generation - 1,
        &claude_options("opus-5"),
    );
    assert!(
        agent_row(&chat, DisplayMode::Compact).contains("Sonnet 5"),
        "a snapshot from a previous session generation is ignored"
    );

    chat.handle_acp_session_config_snapshot(
        chat.acp_mode_config_generation,
        &claude_options("opus-5"),
    );
    assert!(agent_row(&chat, DisplayMode::Compact).contains("Opus 5"));
}

#[test]
fn the_mode_cycle_is_derived_from_the_tracked_configuration() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.handle_acp_session_config_update(&claude_options("opus-5"));

    let mode = chat.acp_mode_config.as_ref().expect("mode config");
    assert_eq!(mode.current_label, "Plan");
    assert_eq!(mode.next_label, "Build");
}

#[test]
fn the_config_picker_opens_from_the_tracked_configuration() {
    use crate::render::renderable::Renderable;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.handle_acp_session_config_update(&claude_options("opus-5"));

    // No harness handle is available here: opening the panel must still work,
    // because the advertised configuration is already tracked.
    chat.open_session_config_popup();

    let area = Rect::new(0, 0, 84, chat.bottom_pane.desired_height(84));
    let mut buffer = Buffer::empty(area);
    chat.bottom_pane.render(area, &mut buffer);
    let rendered = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    for expected in ["Mode (Plan)", "Model (Opus 5)", "Effort (xhigh)"] {
        assert!(
            rendered.contains(expected),
            "the config panel must list {expected:?}, got:\n{rendered}"
        );
    }
}

#[test]
fn a_session_start_carrying_configuration_renders_it_on_the_welcome_card() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;

    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionStarted(
            nori_protocol::SessionStarted {
                transcript_id: None,
                acp_session_id: nori_protocol::acp::v1::SessionId::new("0"),
                cwd: std::path::PathBuf::from("/workspace"),
                transcript_path: None,
                history_log_id: 0,
                history_entry_count: 0,
                config_options: claude_options("opus-5"),
            },
        )),
    );

    let welcome = last_history_cell(&mut rx).expect("welcome card");
    assert!(
        lines_to_single_string(&welcome.display_lines(80))
            .contains("Agent        Claude · Opus 5 · xhigh · Plan"),
        "the agent's session/new configuration must reach the very first card"
    );
}
