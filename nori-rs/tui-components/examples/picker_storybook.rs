mod support;

use anyhow::Result;
use codex_tui_components::Picker;
use codex_tui_components::PickerAction;
use codex_tui_components::PickerColumn;
use codex_tui_components::PickerColumnWidth;
use codex_tui_components::PickerDetail;
use codex_tui_components::PickerItem;
use codex_tui_components::PickerMode;
use codex_tui_components::PickerOutcome;
use codex_tui_components::PickerState;
use codex_tui_components::SearchMode;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use ratatui::layout::Alignment;
use ratatui::text::Line;

use support::StorybookTerminal;

fn main() -> Result<()> {
    let mut terminal = StorybookTerminal::enter()?;
    let mut state = story_state();
    let mut notice =
        "Resize the terminal to see columns collapse and the detail pane move.".to_string();

    loop {
        terminal.terminal.draw(|frame| {
            frame.render_widget(Picker::new(&state), frame.area());
            if frame.area().height > 2 {
                frame.render_widget(
                    Line::styled(notice.clone(), codex_tui_components::Theme::default().muted)
                        .alignment(Alignment::Center),
                    ratatui::layout::Rect::new(
                        frame.area().x + 2,
                        frame.area().bottom() - 3,
                        frame.area().width.saturating_sub(4),
                        1,
                    ),
                );
            }
        })?;

        let Some(Event::Key(key)) = terminal.next_event()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            break;
        }
        let Some(action) = picker_action(key) else {
            continue;
        };
        match state.handle(action) {
            PickerOutcome::Selected(key) => notice = format!("Selected: {key}"),
            PickerOutcome::Submitted(keys) => notice = format!("Submitted: {}", keys.join(", ")),
            PickerOutcome::Cancelled => break,
            PickerOutcome::Unchanged
            | PickerOutcome::SelectionChanged(_)
            | PickerOutcome::Toggled { .. }
            | PickerOutcome::QueryChanged(_)
            | PickerOutcome::CategoryChanged(_) => {}
        }
    }
    Ok(())
}

fn picker_action(key: KeyEvent) -> Option<PickerAction> {
    match key.code {
        KeyCode::Up => Some(PickerAction::MoveUp),
        KeyCode::Down => Some(PickerAction::MoveDown),
        KeyCode::PageUp => Some(PickerAction::PageUp),
        KeyCode::PageDown => Some(PickerAction::PageDown),
        KeyCode::Home => Some(PickerAction::First),
        KeyCode::End => Some(PickerAction::Last),
        KeyCode::Enter => Some(PickerAction::Submit),
        KeyCode::Char(' ') => Some(PickerAction::Toggle),
        KeyCode::Backspace => Some(PickerAction::Backspace),
        KeyCode::Tab => Some(PickerAction::NextCategory),
        KeyCode::BackTab => Some(PickerAction::PreviousCategory),
        KeyCode::Char(character) => Some(PickerAction::AppendQuery(character)),
        KeyCode::Esc
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Delete
        | KeyCode::Insert
        | KeyCode::F(_)
        | KeyCode::Null
        | KeyCode::CapsLock
        | KeyCode::ScrollLock
        | KeyCode::NumLock
        | KeyCode::PrintScreen
        | KeyCode::Pause
        | KeyCode::Menu
        | KeyCode::KeypadBegin
        | KeyCode::Media(_)
        | KeyCode::Modifier(_) => None,
    }
}

fn story_state() -> PickerState<String> {
    let columns = [
        PickerColumn::flexible("title", "Session").width(PickerColumnWidth::Flexible {
            min: 18,
            max: 40,
            weight: 3,
        }),
        PickerColumn::flexible("project", "Project").hide_below(58),
        PickerColumn::fixed("updated", "Updated", 10),
        PickerColumn::fixed("status", "Turn status", 13).hide_below(82),
    ];
    let items = [
        PickerItem::new("new".to_string(), "title", "Start a new session")
            .cell("project", "Not reported")
            .cell("updated", "now")
            .cell("status", "ready")
            .search_text("start create new")
            .pinned(true)
            .category("Local")
            .description("Create a fresh ACP session")
            .details([
                PickerDetail::new("Action", "Create a fresh ACP session"),
                PickerDetail::new("Transcript", "No transcript will be loaded"),
            ]),
        PickerItem::new("parser".to_string(), "title", "Fix parser recovery")
            .cell("project", "nori-cli")
            .cell("updated", "2m ago")
            .cell("status", "working")
            .search_text("fix parser recovery nori cli session 019f")
            .current(true)
            .category("Local")
            .description("Codex is implementing parser recovery")
            .details([
                PickerDetail::new("Agent", "Codex"),
                PickerDetail::new("Path", "/workspace/nori/cli"),
                PickerDetail::new("Current turn", "Implementing parser recovery"),
            ]),
        PickerItem::new("markdown".to_string(), "title", "Improve Markdown tables")
            .cell("project", "external-codex")
            .cell("updated", "18m ago")
            .cell("status", "waiting")
            .search_text("markdown tables codex waiting")
            .category("Local")
            .description("Waiting for user input")
            .details([
                PickerDetail::new("Agent", "Codex"),
                PickerDetail::new("Turn", "Waiting for user input"),
            ]),
        PickerItem::new("cloud".to_string(), "title", "Slack · Claude")
            .cell("project", "Nori Sessions")
            .cell("updated", "1h ago")
            .cell("status", "ready")
            .search_text("slack claude cloud sessions")
            .category("Cloud")
            .description("The broker owns this remote session")
            .details([
                PickerDetail::new("Origin", "Nori cloud"),
                PickerDetail::new("Ownership", "The broker owns the remote session"),
            ]),
        PickerItem::new("offline".to_string(), "title", "Unavailable legacy session")
            .cell("project", "handroll")
            .cell("updated", "3d ago")
            .cell("status", "offline")
            .search_text("legacy handroll offline")
            .disabled(true)
            .category("Cloud"),
    ];
    PickerState::new("Picker storybook", columns, items)
        .subtitle("Type to fuzzy-find · tab changes category · enter selects · q exits")
        .mode(PickerMode::Single)
        .search_mode(SearchMode::Fuzzy)
        .categories(["Local", "Cloud"])
        .search_placeholder("Title, project, or session id")
}
