//! Snapshot tests for selection components

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tui_components::render::Renderable;
use tui_components::selection::selection_option_row;

// Imports for the interactive example
use crossterm::event::KeyModifiers;
use tui_components::selection::{
    SelectionItem, SelectionList, SelectionListConfig, SelectionListEvent, standard_popup_hint_line,
};

// Test imports
#[cfg(test)]
use insta::assert_snapshot;

fn render_to_string(renderable: &dyn Renderable, width: u16) -> String {
    let height = renderable.desired_height(width);
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    renderable.render(area, &mut buf);

    let lines: Vec<String> = (0..area.height)
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
            line
        })
        .collect();
    lines.join("\n")
}

#[test]
fn selection_row_unselected() {
    let row = selection_option_row(0, "First Option".to_string(), false);
    let output = render_to_string(row.as_ref(), 40);
    assert_snapshot!("selection_row_unselected", output);
}

#[test]
fn selection_row_selected() {
    let row = selection_option_row(0, "First Option".to_string(), true);
    let output = render_to_string(row.as_ref(), 40);
    assert_snapshot!("selection_row_selected", output);
}

#[test]
fn selection_row_double_digit_index() {
    let row = selection_option_row(9, "Tenth Option".to_string(), false);
    let output = render_to_string(row.as_ref(), 40);
    assert_snapshot!("selection_row_double_digit_index", output);
}

#[test]
fn selection_row_triple_digit_index() {
    let row = selection_option_row(99, "Hundredth Option".to_string(), true);
    let output = render_to_string(row.as_ref(), 40);
    assert_snapshot!("selection_row_triple_digit_index", output);
}

#[test]
fn selection_row_long_label_wraps() {
    let row = selection_option_row(
        0,
        "This is a very long option label that should wrap to multiple lines when rendered"
            .to_string(),
        false,
    );
    let output = render_to_string(row.as_ref(), 40);
    assert_snapshot!("selection_row_long_label_wraps", output);
}

#[test]
fn selection_row_unicode_content() {
    let row = selection_option_row(0, "Option with emoji 🚀 and unicode ✓".to_string(), true);
    let output = render_to_string(row.as_ref(), 50);
    assert_snapshot!("selection_row_unicode_content", output);
}

// SelectionList widget tests

#[cfg(test)]
use crossterm::event::{KeyCode, KeyEvent};

// Helper function to render SelectionList to string for snapshot testing
#[cfg(test)]
fn render_list_to_string<T: Clone>(list: &SelectionList<T>, width: u16) -> String {
    let height = list.desired_height(width);
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    list.render(area, &mut buf);

    let lines: Vec<String> = (0..area.height)
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
            line
        })
        .collect();
    lines.join("\n")
}

#[cfg(test)]
#[derive(Clone)]
struct TestData {
    id: u32,
}

#[cfg(test)]
fn make_test_items() -> Vec<SelectionItem<TestData>> {
    vec![
        SelectionItem {
            data: TestData { id: 1 },
            name: "Read Only".to_string(),
            description: Some("Codex can read files".to_string()),
            selected_description: None,
            is_current: true,
            display_shortcut: None,
            search_value: Some("read only".to_string()),
        },
        SelectionItem {
            data: TestData { id: 2 },
            name: "Full Access".to_string(),
            description: Some("Codex can edit files".to_string()),
            selected_description: None,
            is_current: false,
            display_shortcut: None,
            search_value: Some("full access".to_string()),
        },
    ]
}

#[test]
fn selection_list_basic() {
    let config = SelectionListConfig::new()
        .with_title("Select Approval Mode")
        .with_footer_hint(standard_popup_hint_line());
    let items = make_test_items();
    let list = SelectionList::new(config, items, Box::new(()));
    let output = render_to_string(&list, 48);
    assert_snapshot!("selection_list_basic", output);
}

#[test]
fn selection_list_with_subtitle() {
    let config = SelectionListConfig::new()
        .with_title("Select Approval Mode")
        .with_subtitle("Switch between Codex approval presets")
        .with_footer_hint(standard_popup_hint_line());
    let items = make_test_items();
    let list = SelectionList::new(config, items, Box::new(()));
    let output = render_to_string(&list, 48);
    assert_snapshot!("selection_list_with_subtitle", output);
}

#[test]
fn selection_list_with_search() {
    let config = SelectionListConfig::new()
        .with_title("Select Approval Mode")
        .with_search(Some("Type to search...".to_string()))
        .with_footer_hint(standard_popup_hint_line());
    let items = make_test_items();
    let list = SelectionList::new(config, items, Box::new(()));
    let output = render_to_string(&list, 48);
    assert_snapshot!("selection_list_with_search", output);
}

#[test]
fn selection_list_empty() {
    let config = SelectionListConfig::new()
        .with_title("Empty List")
        .with_footer_hint(standard_popup_hint_line());
    let items: Vec<SelectionItem<TestData>> = vec![];
    let list = SelectionList::new(config, items, Box::new(()));
    let output = render_to_string(&list, 48);
    assert_snapshot!("selection_list_empty", output);
}

#[test]
fn selection_list_navigation() {
    let config = SelectionListConfig::new().with_title("Select Option");
    let items = make_test_items();
    let mut list = SelectionList::new(config, items, Box::new(()));

    // Initial state - first item selected
    assert_eq!(list.selected_index(), Some(0));

    // Move down
    list.move_down();
    assert_eq!(list.selected_index(), Some(1));

    // Wrap to top
    list.move_down();
    assert_eq!(list.selected_index(), Some(0));

    // Move up wraps to bottom
    list.move_up();
    assert_eq!(list.selected_index(), Some(1));
}

#[test]
fn selection_list_search_filtering() {
    let config = SelectionListConfig::new()
        .with_title("Select Option")
        .with_search(Some("Search...".to_string()));
    let items = make_test_items();
    let mut list = SelectionList::new(config, items, Box::new(()));

    // Initially all items visible
    assert_eq!(list.selected_index(), Some(0));

    // Filter to "read"
    list.set_search_query("read".to_string());
    assert_eq!(list.selected_index(), Some(0)); // "Read Only" is first match

    // Filter to "full"
    list.set_search_query("full".to_string());
    assert_eq!(list.selected_index(), Some(1)); // "Full Access" is only match

    // Clear filter - selection should try to preserve but may reset to first item
    list.set_search_query("".to_string());
    // When clearing search, the first matching item with is_current might be selected
    assert!(list.selected_index().is_some()); // Just verify something is selected
}

#[test]
fn test_selection_list_basic_render() {
    let config = SelectionListConfig::new()
        .with_title("Select Approval Mode")
        .with_footer_hint(standard_popup_hint_line());
    let items = make_test_items();
    let list = SelectionList::new(config, items, Box::new(()));
    let output = render_list_to_string(&list, 48);
    assert_snapshot!(output);
}

#[test]
fn test_selection_list_with_search_render() {
    let config = SelectionListConfig::new()
        .with_title("Select Approval Mode")
        .with_search(Some("Type to search...".to_string()))
        .with_footer_hint(standard_popup_hint_line());
    let items = make_test_items();
    let list = SelectionList::new(config, items, Box::new(()));
    let output = render_list_to_string(&list, 48);
    assert_snapshot!(output);
}

#[test]
fn test_selection_list_with_subtitle_render() {
    let config = SelectionListConfig::new()
        .with_title("Select Approval Mode")
        .with_subtitle("Switch between Codex approval presets")
        .with_footer_hint(standard_popup_hint_line());
    let items = make_test_items();
    let list = SelectionList::new(config, items, Box::new(()));
    let output = render_list_to_string(&list, 48);
    assert_snapshot!(output);
}

#[test]
fn test_selection_list_long_list_render() {
    let config = SelectionListConfig::new()
        .with_title("Select Option")
        .with_footer_hint(standard_popup_hint_line());
    let mut items = Vec::new();
    for i in 0..12 {
        items.push(SelectionItem {
            data: TestData { id: i },
            name: format!("Option {}", i + 1),
            description: Some(format!("Description for option {}", i + 1)),
            selected_description: None,
            is_current: i == 0,
            display_shortcut: None,
            search_value: Some(format!("option {}", i + 1)),
        });
    }
    let list = SelectionList::new(config, items, Box::new(()));
    let output = render_list_to_string(&list, 48);
    assert_snapshot!(output);
}

#[test]
fn selection_list_keyboard_handling() {
    let config = SelectionListConfig::new().with_title("Select Option");
    let items = make_test_items();
    let mut list = SelectionList::new(config, items, Box::new(()));

    // Up/Down arrows
    let event = list.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(event, SelectionListEvent::None);
    assert_eq!(list.selected_index(), Some(1));

    // Enter selects
    let event = list.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(event, SelectionListEvent::Selected(1));

    // Esc cancels
    let event = list.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(event, SelectionListEvent::Cancelled);
}

#[test]
fn selection_list_number_key_selection() {
    let config = SelectionListConfig::new().with_title("Select Option");
    let items = make_test_items();
    let mut list = SelectionList::new(config, items, Box::new(()));

    // Press '2' to select second item
    let event = list.handle_key_event(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
    assert_eq!(event, SelectionListEvent::Selected(1));
}

#[test]
fn selection_list_search_keyboard() {
    let config = SelectionListConfig::new()
        .with_title("Select Option")
        .with_search(Some("Search...".to_string()));
    let items = make_test_items();
    let mut list = SelectionList::new(config, items, Box::new(()));

    // Type 'r'
    list.handle_key_event(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert_eq!(list.search_query(), "r");

    // Type 'e'
    list.handle_key_event(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    assert_eq!(list.search_query(), "re");

    // Backspace
    list.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(list.search_query(), "r");
}
