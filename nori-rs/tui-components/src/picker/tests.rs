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
    snapshot_with_options(state, width, height, density, true)
}

fn snapshot_with_options(
    state: &PickerState<String>,
    width: u16,
    height: u16,
    density: PickerDensity,
    selection_rails: bool,
) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(
                Picker::new(state)
                    .density(density)
                    .selection_rails(selection_rails),
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
fn simple_picker_uses_description_rows_without_table_headers_and_numbers_choices() {
    let state = PickerState::new(
        "Session Config",
        [PickerColumn::flexible("option", "Option")],
        [
            PickerItem::new("mode", "option", "Mode (Agent)")
                .description("Approval and sandboxing preset for the session"),
            PickerItem::new("model", "option", "Model (GPT-5.6-Sol)")
                .description("Model Codex uses for the session"),
        ],
    )
    .subtitle("Select an ACP session setting to change")
    .search_mode(SearchMode::None);

    let buffer = rendered_picker_buffer(&state, 80, 14);

    assert!(find_ascii_text_at_or_below(&buffer, "Option", 0).is_none());
    assert_eq!(
        find_ascii_text_at_or_below(&buffer, "Session Config", 0),
        Some((2, 1))
    );
    assert_eq!(
        find_ascii_text_at_or_below(&buffer, "Select an ACP session setting to change", 0),
        Some((2, 2))
    );
    assert_eq!(
        find_ascii_text_at_or_below(&buffer, "Mode (Agent)", 0),
        Some((8, 4))
    );
    assert_eq!(buffer[(3, 4)].symbol(), "›");
    assert_eq!(buffer[(5, 4)].symbol(), "1");
    assert_eq!(buffer[(6, 4)].symbol(), ".");
    assert_eq!(
        find_ascii_text_at_or_below(&buffer, "Approval and sandboxing preset for the session", 0,),
        Some((8, 5))
    );
}

#[test]
fn picker_section_headings_are_bold_and_not_selectable() {
    let items = [
        PickerItem::new("recommended", "name", "Recommended").section_heading(true),
        PickerItem::new("stable", "name", "Stable"),
        PickerItem::new("unavailable", "name", "Unavailable").disabled(true),
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
        assert_eq!(buffer[(5, heading.1)].symbol(), " ");
        assert_eq!(buffer[(6, heading.1)].symbol(), " ");
    }
    let stable = find_ascii_text_at_or_below(&buffer, "Stable", 3).expect("stable choice");
    assert_eq!(buffer[(5, stable.1)].symbol(), "1");
    let unavailable =
        find_ascii_text_at_or_below(&buffer, "Unavailable", 3).expect("disabled choice");
    assert_eq!(buffer[(5, unavailable.1)].symbol(), " ");
    assert_eq!(buffer[(6, unavailable.1)].symbol(), " ");
    let preview = find_ascii_text_at_or_below(&buffer, "Preview", 3).expect("preview choice");
    assert_eq!(buffer[(3, preview.1)].symbol(), "›");
    assert_eq!(buffer[(5, preview.1)].symbol(), "2");
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

    let selected = find_ascii_text_at_or_below(buffer, "Start a new session", 4)
        .expect("selected primary copy");
    let description = find_ascii_text_at_or_below(buffer, "Create a fresh ACP session", 4)
        .expect("selected supporting copy");
    let pointer = (3, selected.1);
    for x in 2..56 {
        assert_eq!(buffer[(x, selected.1)].bg, Color::Rgb(43, 43, 43));
    }
    assert_eq!(buffer[pointer].symbol(), "›");
    assert_eq!(buffer[pointer].fg, Color::Green);
    assert_eq!(buffer[selected].fg, Color::Reset);
    assert_eq!(buffer[description].fg, Color::DarkGray);
    assert_eq!(buffer[(3, selected.1 + 2)].bg, Color::Reset);
    assert_eq!(buffer[(3, selected.1 + 4)].bg, Color::Reset);
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
    assert_eq!(buffer[(3, 7)].bg, Color::Rgb(43, 43, 43));
    assert_eq!(buffer[(3, 8)].bg, Color::Rgb(36, 36, 36));
    assert_eq!(buffer[(3, 9)].bg, Color::Rgb(29, 29, 29));
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
    assert_eq!(buffer[(5, disabled_tone.1)].symbol(), "●");
    assert_eq!(buffer[(5, disabled_tone.1)].fg, Color::DarkGray);
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
fn focused_and_unfocused_checked_rows_share_the_dot() {
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
    assert_eq!(buffer[(3, first.1)].symbol(), " ");
    assert_eq!(buffer[(5, first.1)].symbol(), "●");
    assert_eq!(buffer[(5, first.1)].fg, Color::Green);
    assert_eq!(buffer[(3, second.1)].symbol(), "›");
    assert_eq!(buffer[(5, second.1)].symbol(), "●");
    assert_eq!(buffer[(5, second.1)].fg, Color::Green);
}

#[test]
fn turning_rails_off_keeps_the_pointer_and_dots() {
    let columns = [PickerColumn::flexible("title", "Action")];
    let items = [
        PickerItem::new("first", "title", "First action").description("First description"),
        PickerItem::new("second", "title", "Second action").description("Second description"),
    ];
    let mut state = PickerState::new("Choose actions", columns, items).mode(PickerMode::Multi);
    state.handle(PickerAction::Toggle);

    let backend = TestBackend::new(48, 10);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| frame.render_widget(Picker::new(&state).selection_rails(false), frame.area()))
        .expect("draw picker");
    let buffer = terminal.backend().buffer();
    let first = find_ascii_text_at_or_below(buffer, "First action", 2).expect("first row");
    let description =
        find_ascii_text_at_or_below(buffer, "First description", 2).expect("description");
    for y in first.1..=description.1 {
        assert_eq!(find_symbols_on_row(buffer, y, "▏"), Vec::new());
        assert_eq!(find_symbols_on_row(buffer, y, "▕"), Vec::new());
    }
    assert_eq!(buffer[(2, first.1)].symbol(), "›");
    assert_eq!(buffer[(4, first.1)].symbol(), "●");
}

#[test]
fn numbered_simple_picker_description_preserves_right_selection_rail() {
    let state = PickerState::new(
        "Choose action",
        [PickerColumn::flexible("title", "Action")],
        [
            PickerItem::new("first", "title", "First action").description(
                "This supporting description is intentionally much wider than the picker",
            ),
        ],
    )
    .search_mode(SearchMode::None);

    let buffer = rendered_picker_buffer(&state, 24, 8);
    let title = find_ascii_text_at_or_below(&buffer, "First action", 2).expect("title row");
    let description_y = title.1 + 1;

    let title_left = find_symbol_on_row(&buffer, title.1, "▏").expect("title left rail");
    let title_right = find_symbol_on_row(&buffer, title.1, "▕").expect("title right rail");
    let description_left =
        find_symbol_on_row(&buffer, description_y, "▏").expect("description left rail");
    let description_right =
        find_symbol_on_row(&buffer, description_y, "▕").expect("description right rail");
    assert_eq!(description_left.0, title_left.0);
    assert_eq!(description_right.0, title_right.0);
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

fn two_row_sessions() -> PickerState<String> {
    let items = [
        PickerItem::new("parser".to_string(), crate::session_table::SESSION, "● Fix the parser recovery path")
            .cell(crate::session_table::AGENT, "claude-code")
            .cell_tone(crate::session_table::AGENT, ProviderKind::Claude)
            .cell(crate::session_table::STATUS, "working")
            .cell_detail_tone(crate::session_table::STATUS, crate::DetailTone::Info)
            .cell(crate::session_table::CONTEXT, "8% / 475k")
            .second_row_cell(
                crate::session_table::SESSION,
                "I traced the failure to the recovery branch, which drops the token after a missing brace",
            )
            .second_row_cell(crate::session_table::AGENT, "claude-opus-5-5-with-a-long-model-name")
            .second_row_cell(crate::session_table::STATUS, "2m ago"),
        PickerItem::new("tables".to_string(), crate::session_table::SESSION, "○ Improve Markdown tables")
            .cell(crate::session_table::AGENT, "codex")
            .cell(crate::session_table::STATUS, "ready")
            .second_row_cell(crate::session_table::SESSION, "no reply yet")
            .second_row_cell(crate::session_table::AGENT, "gpt-5.5")
            .second_row_cell(crate::session_table::STATUS, "14s ago")
            .depth(1),
        PickerItem::new("docs".to_string(), crate::session_table::SESSION, "○ Update the docs")
            .cell(crate::session_table::AGENT, "gemini")
            .cell(crate::session_table::STATUS, "ready")
            .second_row_cell(crate::session_table::SESSION, "Done.")
            .second_row_cell(crate::session_table::STATUS, "1h ago"),
    ];
    PickerState::new("Sessions", crate::session_table::session_columns(), items)
}

fn render_sessions(state: &PickerState<String>, theme: Theme, width: u16) -> Buffer {
    let backend = TestBackend::new(width, 12);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            frame.render_widget(
                Picker::new(state)
                    .theme(theme)
                    .density(PickerDensity::Compact)
                    .show_title(false)
                    .show_details(false)
                    .horizontal_margin(0)
                    .selection_keeps_tones(true),
                frame.area(),
            )
        })
        .expect("draw picker");
    terminal.backend().buffer().clone()
}

fn buffer_lines(buffer: &Buffer) -> Vec<String> {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn two_row_sessions_snapshots_at_each_layout_width() {
    let state = two_row_sessions();
    for width in [60, 80, 89, 120] {
        let lines = buffer_lines(&render_sessions(&state, Theme::default(), width));
        insta::assert_snapshot!(
            format!("two_row_sessions_{width}"),
            lines.join("\n").trim_end().to_string()
        );
    }
}

/// Display column where `needle` starts in `line`.
fn cell_of(line: &str, needle: &str) -> Option<usize> {
    let at = line.find(needle)?;
    Some(unicode_width::UnicodeWidthStr::width(&line[..at]))
}

/// Terminal width, then where Agent, Status and the context value start.
/// The focus rails take a cell at each side, so a picker shows each column
/// two cells later than `session_widths` alone would.
const SESSION_LAYOUTS: [(u16, Option<usize>, usize, Option<usize>); 6] = [
    (59, None, 44, None),
    (60, Some(31), 45, None),
    (80, Some(51), 65, None),
    (88, Some(59), 73, None),
    (89, Some(47), 61, Some(77)),
    (120, Some(78), 92, Some(108)),
];

#[test]
fn two_row_session_columns_sit_at_the_layout_offsets() {
    let state = two_row_sessions();
    for (width, agent, status, context) in SESSION_LAYOUTS {
        let lines = buffer_lines(&render_sessions(&state, Theme::default(), width));
        let header = lines
            .iter()
            .find(|line| line.contains("Status"))
            .expect("header");
        assert_eq!(
            cell_of(header, "Session"),
            Some(3),
            "width {width}: {header}"
        );
        assert_eq!(cell_of(header, "Agent"), agent, "width {width}: {header}");
        assert_eq!(
            cell_of(header, "Status"),
            Some(status),
            "width {width}: {header}"
        );
        // The context column has no heading; its values sit after Status.
        let row = lines
            .iter()
            .find(|line| line.contains("Fix the par"))
            .expect("first row");
        assert_eq!(cell_of(row, "8% / 475k"), context, "width {width}: {row}");
    }
}

#[test]
fn a_second_row_value_is_cut_to_its_own_column() {
    let state = two_row_sessions();
    let lines = buffer_lines(&render_sessions(&state, Theme::default(), 80));
    let preview_row = lines
        .iter()
        .find(|line| line.contains("I traced"))
        .expect("preview row");
    let header = lines
        .iter()
        .find(|line| line.contains("Agent"))
        .expect("header");
    let agent_start = cell_of(header, "Agent").expect("Agent column");
    // The reply starts at its beginning and stops before the Agent column.
    let session_cells: String = preview_row.chars().take(agent_start).collect();
    assert!(session_cells.trim_end().ends_with('…'), "{preview_row}");
    assert!(
        session_cells.contains("I traced the failure"),
        "{preview_row}"
    );
    let agent_cells: String = preview_row.chars().skip(agent_start).take(12).collect();
    assert!(
        agent_cells.contains('…'),
        "the model is cut in the middle: {agent_cells}"
    );
    assert!(agent_cells.starts_with("claude"), "{agent_cells}");
    assert!(agent_cells.trim_end().ends_with("name"), "{agent_cells}");
    assert!(preview_row.contains("2m ago"), "{preview_row}");
}

#[test]
fn a_selected_session_keeps_its_status_and_agent_colours_over_its_highlight() {
    let state = two_row_sessions();
    for background in [(0, 0, 0), (250, 250, 250)] {
        let theme = Theme::for_terminal_background(Some(background));
        let buffer = render_sessions(&state, theme, 87);
        let status = find_ascii_text_at_or_below(&buffer, "working", 0).expect("status cell");
        let agent = find_ascii_text_at_or_below(&buffer, "claude-code", 0).expect("agent cell");
        assert_eq!(buffer[status].fg, Color::Cyan);
        assert_eq!(buffer[agent].fg, Color::Yellow);
        assert_eq!(
            buffer[status].bg,
            theme.selected.bg.expect("selected background")
        );
    }
}

#[test]
fn session_table_lines_alternate_between_a_subtle_stripe_and_the_terminal_background() {
    let mut state = two_row_sessions();
    state.selected_index = Some(2);
    for background in [(0, 0, 0), (250, 250, 250)] {
        let theme = Theme::for_terminal_background(Some(background));
        let buffer = render_sessions(&state, theme, 87);
        let line_backgrounds = |y: u16| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].bg)
                .collect::<Vec<_>>()
        };
        let first = find_ascii_text_at_or_below(&buffer, "Fix the par", 0).expect("row 1");
        let selected =
            find_ascii_text_at_or_below(&buffer, "Update the docs", 0).expect("selected");
        let body = first.1..selected.1;
        assert_eq!(
            body.len(),
            4,
            "two sessions of two lines above the selection"
        );
        for y in (0..first.1).chain(selected.1 + 2..buffer.area.height) {
            assert!(
                line_backgrounds(y).iter().all(|bg| *bg == Color::Reset),
                "line {y} outside the body is not shaded"
            );
        }
        let mut stripe = None;
        for (line, y) in body.enumerate() {
            let backgrounds = line_backgrounds(y);
            if line % 2 == 1 {
                assert!(
                    backgrounds.iter().all(|bg| *bg == Color::Reset),
                    "line {line} keeps the terminal background: {backgrounds:?}"
                );
                continue;
            }
            assert!(
                backgrounds.iter().all(|bg| *bg == backgrounds[0]),
                "line {line} is shaded edge to edge: {backgrounds:?}"
            );
            assert_ne!(backgrounds[0], Color::Reset, "line {line} is shaded");
            assert_eq!(*stripe.get_or_insert(backgrounds[0]), backgrounds[0]);
        }
        // The stripe sits closer to the terminal background than the
        // `row_alt` band of other compact pickers.
        let distance = |colour: Option<Color>| match colour {
            Some(Color::Rgb(red, green, blue)) => [
                (red, background.0),
                (green, background.1),
                (blue, background.2),
            ]
            .into_iter()
            .map(|(channel, base)| (i32::from(channel) - i32::from(base)).abs())
            .max()
            .unwrap_or(0),
            other => panic!("expected an RGB colour: {other:?}"),
        };
        let step = distance(stripe);
        assert!(
            step > 0 && step < distance(theme.row_alt.bg),
            "stripe step {step} for {background:?}"
        );
        let selected_bg = theme.selected.bg.expect("selected background");
        for y in [selected.1, selected.1 + 1] {
            assert!(
                line_backgrounds(y).iter().all(|bg| *bg == selected_bg),
                "selected line {y} keeps its highlight"
            );
        }
    }
}

#[test]
fn session_table_stripes_need_a_known_terminal_background() {
    let buffer = render_sessions(
        &two_row_sessions(),
        Theme::for_terminal_background(None),
        87,
    );
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            assert_eq!(buffer[(x, y)].bg, Color::Reset, "({x}, {y})");
        }
    }
}
