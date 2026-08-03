use super::*;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::text::Line;

fn session_picker() -> PickerState<String> {
    let columns = [
        PickerColumn::flexible("title", "Session").width(PickerColumnWidth::Flexible {
            min: 14,
            max: 36,
            weight: 3,
        }),
        PickerColumn::flexible("project", "Project")
            .hide_below(52)
            .width(PickerColumnWidth::Flexible {
                min: 10,
                max: 24,
                weight: 2,
            }),
        PickerColumn::fixed("updated", "Updated", 10),
        PickerColumn::fixed("status", "Turn", 11).hide_below(72),
    ];
    let items = [
        PickerItem::new("new".to_string(), "title", "Start a new session")
            .cell("project", "—")
            .cell("updated", "now")
            .cell("status", "ready")
            .search_text("start a new session create")
            .pinned(true)
            .detail([
                Line::from("Create a fresh ACP session."),
                Line::from("No existing transcript will be loaded."),
            ]),
        PickerItem::new("parser".to_string(), "title", "Fix parser recovery")
            .cell("project", "nori-cli")
            .cell("updated", "2m ago")
            .cell("status", "working")
            .search_text("parser recovery nori-cli session-019f")
            .current(true)
            .detail([
                Line::from("Agent: Codex"),
                Line::from("Path: /workspace/nori/cli"),
                Line::from("Turn: implementing parser recovery"),
            ]),
        PickerItem::new("tables".to_string(), "title", "Improve Markdown tables")
            .cell("project", "codex")
            .cell("updated", "18m ago")
            .cell("status", "waiting")
            .search_text("markdown tables codex session-018a")
            .detail([
                Line::from("Agent: Codex"),
                Line::from("Waiting for user input."),
            ]),
        PickerItem::new("legacy".to_string(), "title", "Legacy unavailable session")
            .cell("project", "handroll")
            .cell("updated", "3d ago")
            .cell("status", "offline")
            .disabled(true),
    ];
    PickerState::new("Resume a session", columns, items)
        .subtitle("Search ACP sessions or start fresh")
        .categories(["Local", "Cloud"])
        .search_mode(SearchMode::Fuzzy)
        .search_placeholder("Title, project, or session id")
}

fn snapshot(state: &PickerState<String>, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(Picker::new(state), frame.area()))
        .expect("draw picker");
    terminal.backend().to_string()
}

#[test]
fn picker_wide_with_columns_and_detail_snapshot() {
    assert_snapshot!(snapshot(&session_picker(), 100, 16));
}

#[test]
fn picker_narrow_collapses_optional_columns_snapshot() {
    assert_snapshot!(snapshot(&session_picker(), 48, 13));
}

#[test]
fn picker_fuzzy_filter_snapshot() {
    let mut state = session_picker();
    for character in "mdtab".chars() {
        state.handle(PickerAction::AppendQuery(character));
    }
    assert_snapshot!(snapshot(&state, 86, 13));
}

#[test]
fn picker_loading_empty_and_error_snapshots() {
    let mut state = session_picker();
    state.load_state = PickerLoadState::Loading("Loading sessions…".to_string());
    assert_snapshot!("picker_loading", snapshot(&state, 62, 10));

    state.load_state = PickerLoadState::Ready;
    state.items.clear();
    state.selected_index = None;
    assert_snapshot!("picker_empty", snapshot(&state, 62, 10));

    state.load_state = PickerLoadState::Failed("ACP session/list timed out".to_string());
    assert_snapshot!("picker_error", snapshot(&state, 62, 10));
}

#[test]
fn multi_picker_selection_snapshot() {
    let mut state = session_picker().mode(PickerMode::Multi);
    state.handle(PickerAction::MoveDown);
    assert_eq!(
        state.handle(PickerAction::Toggle),
        PickerOutcome::Toggled {
            key: "parser".to_string(),
            selected: true,
        }
    );
    state.handle(PickerAction::MoveDown);
    state.handle(PickerAction::Toggle);
    assert_snapshot!(snapshot(&state, 86, 13));
}

#[test]
fn state_returns_typed_outcomes_without_application_events() {
    let mut state = session_picker();
    assert_eq!(
        state.handle(PickerAction::MoveDown),
        PickerOutcome::SelectionChanged(Some("parser".to_string()))
    );
    assert_eq!(
        state.handle(PickerAction::Submit),
        PickerOutcome::Selected("parser".to_string())
    );
    assert_eq!(state.handle(PickerAction::Cancel), PickerOutcome::Cancelled);
}

#[test]
fn caller_can_supply_a_custom_matcher() {
    fn prefix_score(query: &str, search_text: &str) -> Option<u32> {
        search_text
            .to_lowercase()
            .starts_with(&query.to_lowercase())
            .then_some(100)
    }

    let mut state = session_picker().search_mode(SearchMode::Custom(prefix_score));
    state.handle(PickerAction::AppendQuery('m'));

    assert_eq!(
        state
            .visible_indices()
            .into_iter()
            .map(|index| state.items[index].key.as_str())
            .collect::<Vec<_>>(),
        vec!["tables"]
    );
}
