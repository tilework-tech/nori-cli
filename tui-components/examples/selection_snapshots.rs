//! Snapshot tests for selection components

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use tui_components::render::Renderable;
use tui_components::selection::selection_option_row;

// Imports for the interactive example
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode as EventKeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::Paragraph,
};
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

// Interactive example application
struct App {
    lists: Vec<(String, SelectionList<ExampleData>)>,
    last_event: String,
}

#[derive(Clone)]
struct ExampleData {
    name: String,
}

impl App {
    fn new() -> Self {
        let mut lists = Vec::new();

        // 1. Basic selection with title and footer
        let config = SelectionListConfig::new()
            .with_title("Basic Selection")
            .with_footer_hint(standard_popup_hint_line());
        let items = vec![
            SelectionItem {
                data: ExampleData {
                    name: "Option A".to_string(),
                },
                name: "Option A".to_string(),
                description: Some("First option".to_string()),
                selected_description: None,
                is_current: true,
                display_shortcut: None,
                search_value: Some("option a".to_string()),
            },
            SelectionItem {
                data: ExampleData {
                    name: "Option B".to_string(),
                },
                name: "Option B".to_string(),
                description: Some("Second option".to_string()),
                selected_description: None,
                is_current: false,
                display_shortcut: None,
                search_value: Some("option b".to_string()),
            },
        ];
        lists.push((
            "Basic".to_string(),
            SelectionList::new(config, items, Box::new(())),
        ));

        // 2. With search enabled
        let config = SelectionListConfig::new()
            .with_title("With Search")
            .with_search(Some("Type to filter...".to_string()))
            .with_footer_hint(standard_popup_hint_line());
        let items = vec![
            SelectionItem {
                data: ExampleData {
                    name: "Apple".to_string(),
                },
                name: "Apple".to_string(),
                description: Some("A fruit".to_string()),
                selected_description: None,
                is_current: false,
                display_shortcut: None,
                search_value: Some("apple fruit".to_string()),
            },
            SelectionItem {
                data: ExampleData {
                    name: "Banana".to_string(),
                },
                name: "Banana".to_string(),
                description: Some("Another fruit".to_string()),
                selected_description: None,
                is_current: false,
                display_shortcut: None,
                search_value: Some("banana fruit".to_string()),
            },
            SelectionItem {
                data: ExampleData {
                    name: "Carrot".to_string(),
                },
                name: "Carrot".to_string(),
                description: Some("A vegetable".to_string()),
                selected_description: None,
                is_current: false,
                display_shortcut: None,
                search_value: Some("carrot vegetable".to_string()),
            },
        ];
        lists.push((
            "Searchable".to_string(),
            SelectionList::new(config, items, Box::new(())),
        ));

        // 3. With subtitle
        let config = SelectionListConfig::new()
            .with_title("With Subtitle")
            .with_subtitle("This is a helpful subtitle")
            .with_footer_hint(standard_popup_hint_line());
        let items = vec![SelectionItem {
            data: ExampleData {
                name: "First".to_string(),
            },
            name: "First".to_string(),
            description: Some("First item".to_string()),
            selected_description: None,
            is_current: false,
            display_shortcut: None,
            search_value: Some("first".to_string()),
        }];
        lists.push((
            "Subtitle".to_string(),
            SelectionList::new(config, items, Box::new(())),
        ));

        // 4. Long list for scrolling
        let config = SelectionListConfig::new()
            .with_title("Scrolling List")
            .with_footer_hint(standard_popup_hint_line());
        let mut items = Vec::new();
        for i in 0..12 {
            items.push(SelectionItem {
                data: ExampleData {
                    name: format!("Item {}", i + 1),
                },
                name: format!("Item {}", i + 1),
                description: Some(format!("Description {}", i + 1)),
                selected_description: None,
                is_current: i == 0,
                display_shortcut: None,
                search_value: Some(format!("item {}", i + 1)),
            });
        }
        lists.push((
            "Long List".to_string(),
            SelectionList::new(config, items, Box::new(())),
        ));

        Self {
            lists,
            last_event: "Press keys to interact...".to_string(),
        }
    }

    fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;

            if let Event::Key(key) = event::read()? {
                // Exit on Esc or Ctrl+C
                if key.code == EventKeyCode::Esc
                    || (key.code == EventKeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    break;
                }

                // Distribute input to all SelectionLists
                for (label, list) in &mut self.lists {
                    let event = list.handle_key_event(key);
                    match event {
                        SelectionListEvent::Selected(idx) => {
                            self.last_event = format!("[{}] Selected item at index {}", label, idx);
                        }
                        SelectionListEvent::Cancelled => {
                            self.last_event = format!("[{}] Cancelled", label);
                        }
                        SelectionListEvent::None => {}
                    }
                }
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // Status line
                Constraint::Min(0),    // Lists
            ])
            .split(frame.area());

        // Render status line
        let status = Paragraph::new(format!("Last event: {}", self.last_event))
            .style(Style::default().fg(Color::Cyan));
        frame.render_widget(status, main_layout[0]);

        // Calculate heights for each list
        let num_lists = self.lists.len();
        let constraints = vec![Constraint::Min(0); num_lists];

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(main_layout[1]);

        // Render each SelectionList with its label
        for (i, (label, list)) in self.lists.iter_mut().enumerate() {
            if i < chunks.len() {
                let area = chunks[i];

                // Split area: 1 line for label, rest for list
                let inner_layout = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Min(0)])
                    .split(area);

                // Render label
                let label_text = Paragraph::new(format!("[ {} ]", label))
                    .style(Style::default().fg(Color::Yellow));
                frame.render_widget(label_text, inner_layout[0]);

                // Render list
                list.render(inner_layout[1], frame.buffer_mut());
            }
        }
    }
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let mut terminal = ratatui::init();
    let mut app = App::new();
    let result = app.run(&mut terminal);
    ratatui::restore();
    result
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
