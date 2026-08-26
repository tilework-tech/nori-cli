use super::*;
use crate::ProviderKind;
use crate::Theme;
use insta::assert_snapshot;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;
use ratatui::style::Modifier;

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
            .cell("project", "Not reported")
            .cell("updated", "now")
            .cell("status", "ready")
            .search_text("start a new session create")
            .pinned(true)
            .description("Create a fresh ACP session")
            .details([
                PickerDetail::new("Action", "Create a fresh ACP session"),
                PickerDetail::new("Transcript", "No existing transcript will be loaded"),
            ]),
        PickerItem::new("parser".to_string(), "title", "Fix parser recovery")
            .cell("project", "nori-cli")
            .cell("updated", "2m ago")
            .cell("status", "working")
            .search_text("parser recovery nori-cli session-019f")
            .current(true)
            .description("Codex is implementing parser recovery")
            .details([
                PickerDetail::new("Agent", "Codex"),
                PickerDetail::new("Path", "/workspace/nori/cli"),
                PickerDetail::new("Turn", "Implementing parser recovery"),
            ]),
        PickerItem::new("tables".to_string(), "title", "Improve Markdown tables")
            .cell("project", "codex")
            .cell("updated", "18m ago")
            .cell("status", "waiting")
            .search_text("markdown tables codex session-018a")
            .description("Waiting for user input")
            .details([
                PickerDetail::new("Agent", "Codex"),
                PickerDetail::new("Turn", "Waiting for user input"),
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
    snapshot_with_density(state, width, height, PickerDensity::Normal)
}

fn snapshot_with_density(
    state: &PickerState<String>,
    width: u16,
    height: u16,
    density: PickerDensity,
) -> String {
    snapshot_with_options(state, width, height, density, false)
}

fn snapshot_with_options(
    state: &PickerState<String>,
    width: u16,
    height: u16,
    density: PickerDensity,
    fullscreen_selection_rails: bool,
) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(
                Picker::new(state)
                    .density(density)
                    .fullscreen_selection_rails(fullscreen_selection_rails),
                frame.area(),
            )
        })
        .expect("draw picker");
    terminal.backend().to_string()
}

#[test]
fn picker_wide_with_columns_and_detail_snapshot() {
    assert_snapshot!(snapshot(&session_picker(), 124, 16));
}

#[test]
fn picker_narrow_collapses_optional_columns_snapshot() {
    assert_snapshot!(snapshot(&session_picker(), 48, 13));
}

#[test]
fn picker_compact_uses_single_height_rows_snapshot() {
    assert_snapshot!(snapshot_with_density(
        &session_picker(),
        86,
        13,
        PickerDensity::Compact,
    ));
}

#[test]
fn picker_section_headings_are_bold_and_not_selectable() {
    let items = [
        PickerItem::new("recommended", "name", "Recommended").section_heading(true),
        PickerItem::new("stable", "name", "Stable"),
        PickerItem::new("other", "name", "Other").section_heading(true),
        PickerItem::new("preview", "name", "Preview"),
    ];
    let mut state = PickerState::new("Models", [PickerColumn::flexible("name", "Model")], items)
        .search_mode(SearchMode::None);

    assert_eq!(state.selected_index, Some(1));
    assert_eq!(
        state.handle(PickerAction::MoveDown),
        PickerOutcome::SelectionChanged(Some("preview"))
    );

    let buffer = rendered_picker_buffer(&state, 48, 18);
    for label in ["Recommended", "Other"] {
        let heading = find_ascii_text_at_or_below(&buffer, label, 3).expect("section heading");
        assert!(buffer[heading].modifier.contains(Modifier::BOLD));
        assert_ne!(buffer[heading].fg, Color::DarkGray);
    }
}

#[test]
fn section_heading_remains_noninteractive_when_disabled_is_overridden() {
    let items = [
        PickerItem::new("heading", "name", "Heading")
            .section_heading(true)
            .disabled(false),
        PickerItem::new("choice", "name", "Choice"),
    ];
    let mut state = PickerState::new("Models", [PickerColumn::flexible("name", "Model")], items)
        .search_mode(SearchMode::None);

    assert_eq!(state.selected_index, Some(1));
    state.selected_index = Some(0);
    assert_eq!(state.handle(PickerAction::Submit), PickerOutcome::Unchanged);
}

#[test]
fn picker_normal_selection_rails_snapshot() {
    assert_snapshot!(snapshot_with_options(
        &session_picker(),
        86,
        13,
        PickerDensity::Normal,
        true,
    ));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn picker_applies_density_surfaces_search_input_and_selection() {
    let theme = Theme::for_terminal_background(Some((20, 20, 20)));
    let mut normal_state = session_picker();
    normal_state.handle(PickerAction::ActivateSearch);
    let backend = TestBackend::new(100, 16);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(Picker::new(&normal_state).theme(theme), frame.area()))
        .expect("draw picker");
    let buffer = terminal.backend().buffer();

    for x in 2..56 {
        assert_eq!(buffer[(x, 6)].bg, Color::Rgb(43, 43, 43));
    }
    let selected = find_ascii_text_at_or_below(buffer, "Start a new session", 4)
        .expect("selected primary copy");
    let description = find_ascii_text_at_or_below(buffer, "Create a fresh ACP session", 4)
        .expect("selected supporting copy");
    let pointer = (2, selected.1);
    assert_eq!(buffer[pointer].symbol(), "›");
    assert_eq!(buffer[pointer].fg, Color::Green);
    assert_eq!(buffer[selected].fg, Color::Reset);
    assert_eq!(buffer[description].fg, Color::DarkGray);
    assert_eq!(buffer[(3, 8)].bg, Color::Reset);
    assert_eq!(buffer[(3, 10)].bg, Color::Reset);
    assert_eq!(buffer[(2, 4)].symbol(), "⌕");
    assert_eq!(buffer[(2, 4)].fg, Color::Green);
    assert_eq!(buffer[(2, 4)].bg, Color::Reset);
    assert_eq!(buffer[(4, 4)].bg, Color::Rgb(38, 38, 38));
    assert_eq!(buffer[(3, 5)].bg, Color::Reset);
    assert_eq!(buffer[(3, 14)].bg, Color::Reset);

    let backend = TestBackend::new(86, 13);
    let mut compact_state = session_picker();
    compact_state.handle(PickerAction::ActivateSearch);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(
                Picker::new(&compact_state)
                    .theme(theme)
                    .density(PickerDensity::Compact),
                frame.area(),
            )
        })
        .expect("draw compact picker");
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer[(3, 7)].bg, Color::Rgb(36, 36, 36));
    assert_eq!(buffer[(3, 8)].bg, Color::Rgb(29, 29, 29));
}

#[test]
fn picker_maps_agent_tones_to_category_tabs_and_type_cells() {
    let columns = [
        PickerColumn::fixed("title", "Agent", 16),
        PickerColumn::fixed("type", "Type", 16),
    ];
    let items = [
        PickerItem::new("selected", "title", "Selected row").cell("type", "Neutral"),
        PickerItem::new("claude", "title", "Agent one")
            .cell("type", "Claude")
            .cell_tone("type", ProviderKind::Claude),
        PickerItem::new("codex", "title", "Agent two")
            .cell("type", "Codex")
            .cell_tone("type", ProviderKind::Codex),
        PickerItem::new("gemini", "title", "Agent three")
            .cell("type", "Gemini")
            .cell_tone("type", ProviderKind::Gemini),
        PickerItem::new("antigravity", "title", "Agent four")
            .cell("type", "Antigravity")
            .cell_tone("type", ProviderKind::Antigravity),
        PickerItem::new("nori", "title", "Agent five")
            .cell("type", "Nori")
            .cell_tone("type", ProviderKind::Nori),
    ];
    let state = PickerState::new("Agent picker", columns, items)
        .categories(["Claude", "Codex", "Gemini", "Antigravity", "Nori"])
        .category_tone("Claude", ProviderKind::Claude)
        .category_tone("Codex", ProviderKind::Codex)
        .category_tone("Gemini", ProviderKind::Gemini)
        .category_tone("Antigravity", ProviderKind::Antigravity)
        .category_tone("Nori", ProviderKind::Nori);

    let buffer = rendered_picker_buffer(&state, 110, 22);
    let expected = [
        ("Claude", Color::Yellow),
        ("Codex", Color::White),
        ("Gemini", Color::Blue),
        ("Antigravity", Color::Blue),
        ("Nori", Color::Green),
    ];
    for (label, color) in expected {
        let category = find_ascii_text_at_or_below(&buffer, label, 2).expect("category tab");
        assert_eq!(buffer[category].fg, color, "{label} category tone");
        assert!(!buffer[category].modifier.contains(Modifier::BOLD));

        let cell = find_ascii_text_at_or_below(&buffer, label, 4).expect("type cell");
        assert_eq!(buffer[cell].fg, color, "{label} type tone");
    }
    let mut active_state = state;
    assert_eq!(
        active_state.handle(PickerAction::NextCategory),
        PickerOutcome::CategoryChanged(Some("Claude".to_string()))
    );
    let active_buffer = rendered_picker_buffer(&active_state, 110, 22);
    let active_claude =
        find_ascii_text_at_or_below(&active_buffer, "Claude", 2).expect("active Claude tab");
    assert_eq!(active_buffer[active_claude].fg, Color::Yellow);
    assert!(
        active_buffer[active_claude]
            .modifier
            .contains(Modifier::BOLD)
    );
}

#[test]
fn selection_and_disabled_styles_override_provider_and_checked_tones() {
    let columns = [
        PickerColumn::fixed("title", "Agent", 16),
        PickerColumn::fixed("type", "Type", 16),
    ];
    let items = [
        PickerItem::new("selected", "title", "Selected row")
            .cell("type", "Selected tone")
            .cell_tone("type", ProviderKind::Claude),
        PickerItem::new("disabled", "title", "Disabled row")
            .cell("type", "Disabled tone")
            .cell_tone("type", ProviderKind::Nori)
            .disabled(true),
    ];
    let mut state = PickerState::new("Agent picker", columns, items).mode(PickerMode::Multi);
    state.selected_keys.push("disabled");

    let buffer = rendered_picker_buffer(&state, 64, 12);
    let selected_tone =
        find_ascii_text_at_or_below(&buffer, "Selected tone", 2).expect("selected type cell");
    assert_eq!(buffer[selected_tone].fg, Color::Reset);
    let disabled_tone =
        find_ascii_text_at_or_below(&buffer, "Disabled tone", 2).expect("disabled type cell");
    assert_eq!(buffer[disabled_tone].fg, Color::DarkGray);
    assert_eq!(buffer[(2, disabled_tone.1)].symbol(), "●");
    assert_eq!(buffer[(2, disabled_tone.1)].fg, Color::DarkGray);
}

#[test]
fn picker_fuzzy_filter_snapshot() {
    let mut state = session_picker();
    state.handle(PickerAction::ActivateSearch);
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
fn fallback_theme_distinguishes_focused_and_unfocused_checked_rows() {
    let columns = [PickerColumn::flexible("title", "Action")];
    let items = [
        PickerItem::new("first", "title", "First action"),
        PickerItem::new("second", "title", "Second action"),
    ];
    let mut state = PickerState::new("Choose actions", columns, items).mode(PickerMode::Multi);
    assert!(matches!(
        state.handle(PickerAction::Toggle),
        PickerOutcome::Toggled { selected: true, .. }
    ));
    state.handle(PickerAction::MoveDown);
    assert!(matches!(
        state.handle(PickerAction::Toggle),
        PickerOutcome::Toggled { selected: true, .. }
    ));

    let buffer = rendered_picker_buffer(&state, 48, 10);
    let first = find_ascii_text_at_or_below(&buffer, "First action", 2).expect("first row");
    let second = find_ascii_text_at_or_below(&buffer, "Second action", 2).expect("second row");
    assert_eq!(buffer[(2, first.1)].symbol(), "●");
    assert_eq!(buffer[(2, first.1)].fg, Color::Green);
    assert_eq!(buffer[(2, second.1)].symbol(), "◉");
    assert_eq!(buffer[(2, second.1)].fg, Color::Green);
}

#[test]
fn picker_selection_rails_are_explicit_and_preserve_checked_state() {
    let columns = [PickerColumn::flexible("title", "Action")];
    let items = [
        PickerItem::new("first", "title", "First action").description("First description"),
        PickerItem::new("second", "title", "Second action").description("Second description"),
    ];
    let mut state = PickerState::new("Choose actions", columns, items).mode(PickerMode::Multi);
    assert!(matches!(
        state.handle(PickerAction::Toggle),
        PickerOutcome::Toggled { selected: true, .. }
    ));

    let default_buffer = rendered_picker_buffer(&state, 48, 10);
    let default_first =
        find_ascii_text_at_or_below(&default_buffer, "First action", 2).expect("default row");
    let default_description =
        find_ascii_text_at_or_below(&default_buffer, "First description", 2).expect("description");
    assert_eq!(default_buffer[(2, default_first.1)].symbol(), "◉");
    for y in default_first.1..=default_description.1 {
        assert_eq!(find_symbols_on_row(&default_buffer, y, "▏"), Vec::new());
        assert_eq!(find_symbols_on_row(&default_buffer, y, "▕"), Vec::new());
    }

    let rails_buffer = rendered_picker_buffer_with_rails(&state, 48, 10);
    let rails_first =
        find_ascii_text_at_or_below(&rails_buffer, "First action", 2).expect("railed row");
    let description = find_ascii_text_at_or_below(&rails_buffer, "First description", 2)
        .expect("railed description");
    for y in rails_first.1..=description.1 {
        let left = find_symbol_on_row(&rails_buffer, y, "▏").expect("left rail");
        let right = find_symbol_on_row(&rails_buffer, y, "▕").expect("right rail");
        assert!(left.0 < rails_first.0);
        assert!(right.0 > rails_first.0);
        assert_eq!(rails_buffer[left].fg, Color::Green);
        assert_eq!(rails_buffer[right].fg, Color::Green);
    }
    let selected_checked =
        find_symbol_on_row(&rails_buffer, rails_first.1, "●").expect("selected checked marker");
    assert!(selected_checked.0 < rails_first.0);
    assert_eq!(find_symbol_on_row(&rails_buffer, rails_first.1, "◉"), None);
    assert_eq!(find_symbol_on_row(&rails_buffer, rails_first.1, "›"), None);

    state.handle(PickerAction::MoveDown);
    let mixed_buffer = rendered_picker_buffer_with_rails(&state, 48, 10);
    let first = find_ascii_text_at_or_below(&mixed_buffer, "First action", 2).expect("first row");
    let second =
        find_ascii_text_at_or_below(&mixed_buffer, "Second action", 2).expect("second row");
    assert!(find_symbol_on_row(&mixed_buffer, first.1, "●").is_some());
    assert!(find_symbols_on_row(&mixed_buffer, first.1, "▏").is_empty());
    assert!(find_symbol_on_row(&mixed_buffer, second.1, "○").is_some());
    assert!(find_symbol_on_row(&mixed_buffer, second.1, "▏").is_some());
    assert!(find_symbol_on_row(&mixed_buffer, second.1, "▕").is_some());
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
    state.handle(PickerAction::ActivateSearch);
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

fn rendered_picker_buffer<K: Clone + Eq>(
    state: &PickerState<K>,
    width: u16,
    height: u16,
) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(Picker::new(state), frame.area()))
        .expect("draw picker");
    terminal.backend().buffer().clone()
}

fn rendered_picker_buffer_with_rails<K: Clone + Eq>(
    state: &PickerState<K>,
    width: u16,
    height: u16,
) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(
                Picker::new(state).fullscreen_selection_rails(true),
                frame.area(),
            )
        })
        .expect("draw picker");
    terminal.backend().buffer().clone()
}

fn find_symbol_on_row(buffer: &Buffer, y: u16, symbol: &str) -> Option<(u16, u16)> {
    for x in buffer.area.x..buffer.area.right() {
        if buffer[(x, y)].symbol() == symbol {
            return Some((x, y));
        }
    }
    None
}

fn find_symbols_on_row(buffer: &Buffer, y: u16, symbol: &str) -> Vec<(u16, u16)> {
    (buffer.area.x..buffer.area.right())
        .filter_map(|x| (buffer[(x, y)].symbol() == symbol).then_some((x, y)))
        .collect()
}

fn find_ascii_text_at_or_below(buffer: &Buffer, text: &str, minimum_y: u16) -> Option<(u16, u16)> {
    assert!(text.is_ascii());
    let characters = text.chars().collect::<Vec<_>>();
    for y in minimum_y.max(buffer.area.y)..buffer.area.bottom() {
        for x in buffer.area.x..buffer.area.right() {
            if x.saturating_add(characters.len() as u16) > buffer.area.right() {
                break;
            }
            if characters.iter().enumerate().all(|(offset, character)| {
                buffer[(x + offset as u16, y)].symbol() == character.to_string()
            }) {
                return Some((x, y));
            }
        }
    }
    None
}
