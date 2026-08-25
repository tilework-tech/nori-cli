mod support;

use anyhow::Result;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use nori_tui_components::Picker;
use nori_tui_components::PickerAction;
use nori_tui_components::PickerColumn;
use nori_tui_components::PickerColumnWidth;
use nori_tui_components::PickerDetail;
use nori_tui_components::PickerItem;
use nori_tui_components::PickerMode;
use nori_tui_components::PickerOutcome;
use nori_tui_components::PickerState;
use nori_tui_components::SearchMode;
use ratatui::layout::Alignment;
use ratatui::text::Line;

use support::StorybookTerminal;

fn main() -> Result<()> {
    let mut terminal = StorybookTerminal::enter()?;
    let theme = terminal.theme;
    let mut state = story_state();
    let mut notice =
        "Resize the terminal to see columns collapse and the detail pane move.".to_string();

    loop {
        terminal.terminal.draw(|frame| {
            frame.render_widget(Picker::new(&state).theme(theme), frame.area());
            if frame.area().height > 2 {
                frame.render_widget(
                    Line::styled(notice.clone(), theme.muted).alignment(Alignment::Center),
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
        if should_quit(key, state.search_active) {
            break;
        }
        let Some(action) = picker_action(key, state.search_active) else {
            continue;
        };
        match state.handle(action) {
            PickerOutcome::Selected(key) => notice = format!("Selected: {key}"),
            PickerOutcome::Submitted(keys) => notice = format!("Submitted: {}", keys.join(", ")),
            PickerOutcome::Cancelled => break,
            PickerOutcome::Unchanged
            | PickerOutcome::SelectionChanged(_)
            | PickerOutcome::Toggled { .. }
            | PickerOutcome::SearchModeChanged(_)
            | PickerOutcome::QueryChanged(_)
            | PickerOutcome::CategoryChanged(_) => {}
        }
    }
    Ok(())
}

fn should_quit(key: KeyEvent, search_active: bool) -> bool {
    matches!(key.code, KeyCode::Char('q')) && !search_active
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_search_owns_the_q_shortcut() {
        let q = KeyEvent::new(KeyCode::Char('q'), crossterm::event::KeyModifiers::NONE);

        assert!(!should_quit(q, true));
        assert!(should_quit(q, false));
    }
}

fn picker_action(key: KeyEvent, search_active: bool) -> Option<PickerAction> {
    match key.code {
        KeyCode::Up => Some(PickerAction::MoveUp),
        KeyCode::Down => Some(PickerAction::MoveDown),
        KeyCode::PageUp => Some(PickerAction::PageUp),
        KeyCode::PageDown => Some(PickerAction::PageDown),
        KeyCode::Home => Some(PickerAction::First),
        KeyCode::End => Some(PickerAction::Last),
        KeyCode::Enter => Some(PickerAction::Submit),
        KeyCode::Char(' ') if !search_active => Some(PickerAction::Toggle),
        KeyCode::Backspace if search_active => Some(PickerAction::Backspace),
        KeyCode::Tab => Some(PickerAction::NextCategory),
        KeyCode::BackTab => Some(PickerAction::PreviousCategory),
        KeyCode::Char('f')
            if !search_active
                && key.modifiers == crossterm::event::KeyModifiers::CONTROL =>
        {
            Some(PickerAction::ActivateSearch)
        }
        KeyCode::Char('f' | '/') if !search_active && key.modifiers.is_empty() => {
            Some(PickerAction::ActivateSearch)
        }
        KeyCode::Char('k') if !search_active && key.modifiers.is_empty() => {
            Some(PickerAction::MoveUp)
        }
        KeyCode::Char('j') if !search_active && key.modifiers.is_empty() => {
            Some(PickerAction::MoveDown)
        }
        KeyCode::Char(character) if search_active => Some(PickerAction::AppendQuery(character)),
        KeyCode::Esc if search_active => Some(PickerAction::DeactivateSearch),
        KeyCode::Esc => Some(PickerAction::Cancel),
        KeyCode::Left
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
        | KeyCode::Modifier(_)
        | KeyCode::Backspace
        | KeyCode::Char(_) => None,
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
        .subtitle("/ searches · tab changes category · enter selects · q exits")
        .mode(PickerMode::Single)
        .search_mode(SearchMode::Fuzzy)
        .categories(["Local", "Cloud"])
        .search_placeholder("Title, project, or session id")
}
