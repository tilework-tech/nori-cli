use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{StatefulWidgetRef, WidgetRef};
use tui_components::textarea::{TextArea, TextAreaConfig, TextAreaState};

// Imports for the interactive example
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    widgets::Paragraph,
};

// Imports for tests
#[cfg(test)]
use insta::assert_snapshot;

// Interactive example application
struct App {
    textareas: Vec<(String, TextArea)>,
}

impl App {
    fn new() -> Self {
        let mut textareas = Vec::new();

        // 1. Default with placeholder
        let config = TextAreaConfig::default().with_placeholder("Type here...");
        let textarea = TextArea::new(config);
        textareas.push(("Default with Placeholder".to_string(), textarea));

        // 2. With custom styling (colored)
        let config = TextAreaConfig::default()
            .with_text_style(Style::default().fg(Color::Cyan))
            .with_placeholder("Styled text (cyan)...");
        let textarea = TextArea::new(config);
        textareas.push(("Custom Style (Colored)".to_string(), textarea));

        // 3. Pre-filled with multiline text
        let config = TextAreaConfig::default();
        let mut textarea = TextArea::new(config);
        textarea.set_text("Hello, world!\nThis is pre-filled text.\nTry editing it!");
        textareas.push(("Pre-filled Content".to_string(), textarea));

        // 4. Narrow width for wrapping demonstration
        let config =
            TextAreaConfig::default().with_placeholder("Type a long line to see wrapping...");
        let textarea = TextArea::new(config);
        textareas.push(("Wrapping Demo (20 cols)".to_string(), textarea));

        Self { textareas }
    }

    fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;

            if let Event::Key(key) = event::read()? {
                // Exit on Esc or Ctrl+C
                if key.code == KeyCode::Esc
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    break;
                }

                // Distribute input to all TextAreas
                for (_, textarea) in &mut self.textareas {
                    textarea.handle_key(key);
                }
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let num_areas = self.textareas.len();

        // Create vertical layout with equal heights
        let constraints = vec![Constraint::Percentage(100 / num_areas as u16); num_areas];

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(frame.area());

        // Render each TextArea with its label
        for (i, (label, textarea)) in self.textareas.iter_mut().enumerate() {
            let area = chunks[i];

            // Split area: 1 line for label, rest for textarea
            let inner_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(area);

            // Render label
            let label_text =
                Paragraph::new(format!("[ {label} ]")).style(Style::default().fg(Color::Yellow));
            frame.render_widget(label_text, inner_layout[0]);

            // Render textarea - handle narrow width for wrapping demo
            let textarea_area = if i == 3 {
                // Narrow width for the last TextArea
                let narrow_layout = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(20), Constraint::Min(0)])
                    .split(inner_layout[1]);
                narrow_layout[0]
            } else {
                inner_layout[1]
            };

            WidgetRef::render_ref(textarea, textarea_area, frame.buffer_mut());
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

fn render_to_string(textarea: &TextArea, width: u16, height: u16) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    WidgetRef::render_ref(textarea, area, &mut buf);

    let mut output = String::new();
    for y in 0..height {
        for x in 0..width {
            let cell = &buf[(x, y)];
            output.push_str(cell.symbol());
        }
        // Trim trailing spaces from each line
        if let Some(last_line_start) = output.rfind('\n').map(|i| i + 1).or(Some(0)) {
            let current_line = &output[last_line_start..];
            let trimmed_len = current_line.trim_end().len();
            output.truncate(last_line_start + trimmed_len);
        }
        if y < height - 1 {
            output.push('\n');
        }
    }
    output
}

fn render_with_state(
    textarea: &TextArea,
    width: u16,
    height: u16,
    state: &mut TextAreaState,
) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buf = Buffer::empty(area);
    StatefulWidgetRef::render_ref(textarea, area, &mut buf, state);

    let mut output = String::new();
    for y in 0..height {
        for x in 0..width {
            let cell = &buf[(x, y)];
            output.push_str(cell.symbol());
        }
        // Trim trailing spaces from each line
        if let Some(last_line_start) = output.rfind('\n').map(|i| i + 1).or(Some(0)) {
            let current_line = &output[last_line_start..];
            let trimmed_len = current_line.trim_end().len();
            output.truncate(last_line_start + trimmed_len);
        }
        if y < height - 1 {
            output.push('\n');
        }
    }
    output
}

#[test]
fn test_empty_textarea_with_placeholder() {
    let config = TextAreaConfig::default().with_placeholder("Type a message...");
    let textarea = TextArea::new(config);

    assert_snapshot!(render_to_string(&textarea, 30, 3), @r###"
    Type a message...
    "###);
}

#[test]
fn test_empty_textarea_without_placeholder() {
    let config = TextAreaConfig::default();
    let textarea = TextArea::new(config);

    assert_snapshot!(render_to_string(&textarea, 30, 3), @r###"




    "###);
}

#[test]
fn test_single_line_text() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("Hello world!");

    assert_snapshot!(render_to_string(&textarea, 30, 3), @r###"
    Hello world!
    "###);
}

#[test]
fn test_multiline_text() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("Line 1\nLine 2\nLine 3");

    assert_snapshot!(render_to_string(&textarea, 30, 5), @r###"
    Line 1
    Line 2
    Line 3
    "###);
}

#[test]
fn test_text_wrapping() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("This is a long line that should wrap");

    assert_snapshot!(render_to_string(&textarea, 15, 5), @r"
    This is a
    long line that
    should wrap
    ");
}

#[test]
fn test_insert_text() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.insert_str("Hello");
    textarea.insert_str(" world");

    assert_eq!(textarea.text(), "Hello world");
    assert_snapshot!(render_to_string(&textarea, 20, 3), @r###"
    Hello world
    "###);
}

#[test]
fn test_cursor_position() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("Hello");

    assert_eq!(textarea.cursor(), 5);

    textarea.set_cursor(2);
    assert_eq!(textarea.cursor(), 2);
}

#[test]
fn test_text_replacement() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("Hello world");
    textarea.replace_range(6..11, "Rust");

    assert_eq!(textarea.text(), "Hello Rust");
}

#[test]
fn test_desired_height_single_line() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("Short");

    assert_eq!(textarea.desired_height(20), 1);
}

#[test]
fn test_desired_height_wrapped() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("This is a very long line that will definitely wrap");

    let height = textarea.desired_height(15);
    assert!(height > 1);
}

#[test]
fn test_desired_height_multiline() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("Line 1\nLine 2\nLine 3");

    let height = textarea.desired_height(20);
    assert_eq!(height, 3);
}

#[test]
fn test_is_empty() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);

    assert!(textarea.is_empty());

    textarea.insert_str("text");
    assert!(!textarea.is_empty());
}

#[test]
fn test_scrolling_viewport() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("Line 1\nLine 2\nLine 3\nLine 4\nLine 5");

    let mut state = TextAreaState { scroll: 2 };
    assert_snapshot!(render_with_state(&textarea, 20, 3, &mut state), @r###"
    Line 3
    Line 4
    Line 5
    "###);
}

#[test]
fn test_config_text_style() {
    let config = TextAreaConfig::default().with_text_style(Style::default().fg(Color::Blue));
    let mut textarea = TextArea::new(config);
    textarea.set_text("Styled text");

    // Just test it renders without error - style verification would need more complex assertions
    let _output = render_to_string(&textarea, 20, 3);
}

#[test]
fn test_unicode_content() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("Hello 世界 🌍");

    assert_snapshot!(render_to_string(&textarea, 20, 3), @"Hello 世 界  🌍");
}

#[test]
fn test_empty_lines() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("Line 1\n\nLine 3");

    assert_snapshot!(render_to_string(&textarea, 20, 5), @r###"
    Line 1

    Line 3
    "###);
}

#[test]
fn test_long_single_word() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("Supercalifragilisticexpialidocious");

    assert_snapshot!(render_to_string(&textarea, 15, 5), @r###"
    Supercalifragil
    isticexpialidoc
    ious
    "###);
}

#[test]
fn test_trailing_newline() {
    let config = TextAreaConfig::default();
    let mut textarea = TextArea::new(config);
    textarea.set_text("Line 1\n");

    assert_snapshot!(render_to_string(&textarea, 20, 3), @r###"
    Line 1

    "###);
}
