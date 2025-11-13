use crossterm::event::KeyCode;
use ratatui::text::Span;
use tui_components::key_hint::{KeyBinding, alt, ctrl, plain, shift};

// Test-only imports
#[cfg(test)]
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

// Imports for the interactive example
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode as EventKeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::Paragraph,
};

// Imports for tests
#[cfg(test)]
use insta::assert_snapshot;

// Interactive example application
struct App {
    key_hints: Vec<(String, Span<'static>)>,
}

impl App {
    fn new() -> Self {
        let mut key_hints = Vec::new();

        // 1. Plain keys
        key_hints.push(("Plain Enter".to_string(), plain(KeyCode::Enter).into()));
        key_hints.push(("Plain Up Arrow".to_string(), plain(KeyCode::Up).into()));
        key_hints.push(("Plain Tab".to_string(), plain(KeyCode::Tab).into()));

        // 2. Control keys
        key_hints.push(("Ctrl+C".to_string(), ctrl(KeyCode::Char('c')).into()));
        key_hints.push(("Ctrl+S".to_string(), ctrl(KeyCode::Char('s')).into()));
        key_hints.push(("Ctrl+Z".to_string(), ctrl(KeyCode::Char('z')).into()));

        // 3. Alt keys
        key_hints.push(("Alt+F".to_string(), alt(KeyCode::Char('f')).into()));
        key_hints.push(("Alt+X".to_string(), alt(KeyCode::Char('x')).into()));

        // 4. Shift keys
        key_hints.push(("Shift+Tab".to_string(), shift(KeyCode::Tab).into()));
        key_hints.push(("Shift+Enter".to_string(), shift(KeyCode::Enter).into()));

        // 5. Arrow keys
        key_hints.push(("Up Arrow".to_string(), plain(KeyCode::Up).into()));
        key_hints.push(("Down Arrow".to_string(), plain(KeyCode::Down).into()));
        key_hints.push(("Left Arrow".to_string(), plain(KeyCode::Left).into()));
        key_hints.push(("Right Arrow".to_string(), plain(KeyCode::Right).into()));

        // 6. Page keys
        key_hints.push(("Page Up".to_string(), plain(KeyCode::PageUp).into()));
        key_hints.push(("Page Down".to_string(), plain(KeyCode::PageDown).into()));

        // 7. Multiple modifiers
        key_hints.push(("Ctrl+Shift+S".to_string(), KeyBinding::new(KeyCode::Char('s'), KeyModifiers::CONTROL | KeyModifiers::SHIFT).into()));
        key_hints.push(("Ctrl+Alt+Delete".to_string(), KeyBinding::new(KeyCode::Delete, KeyModifiers::CONTROL | KeyModifiers::ALT).into()));

        Self { key_hints }
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
            }
        }
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame) {
        let num_hints = self.key_hints.len();

        // Create vertical layout with equal heights
        let constraints = vec![Constraint::Length(1); num_hints]; // Each hint gets 1 line

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(frame.area());

        // Render each key hint with its label
        for (i, (label, hint_span)) in self.key_hints.iter().enumerate() {
            let area = chunks[i];

            // Split area: most for label, some for hint
            let inner_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(15), Constraint::Length(15)])
                .split(area);

            // Render label
            let label_paragraph = Paragraph::new(label.clone()).style(Style::default().fg(Color::Yellow));
            frame.render_widget(label_paragraph, inner_layout[0]);

            // Render hint span
            let hint_paragraph = Paragraph::new(hint_span.clone());
            frame.render_widget(hint_paragraph, inner_layout[1]);
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

#[test]
fn test_plain_key() {
    let binding = plain(KeyCode::Enter);
    let span: Span = binding.into();

    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    span.clone().render(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}

#[test]
fn test_ctrl_key() {
    let binding = ctrl(KeyCode::Char('c'));
    let span: Span = binding.into();

    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    span.clone().render(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}

#[test]
fn test_alt_key() {
    let binding = alt(KeyCode::Char('f'));
    let span: Span = binding.into();

    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    span.clone().render(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}

#[test]
fn test_shift_key() {
    let binding = shift(KeyCode::Tab);
    let span: Span = binding.into();

    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    span.clone().render(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}

#[test]
fn test_arrow_keys() {
    let bindings = vec![
        plain(KeyCode::Up),
        plain(KeyCode::Down),
        plain(KeyCode::Left),
        plain(KeyCode::Right),
    ];

    let mut outputs = Vec::new();
    for binding in bindings {
        let span: Span = binding.into();
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 1));
        span.clone().render(buf.area, &mut buf);
        outputs.push(format!("{buf:?}"));
    }

    insta::assert_snapshot!(outputs.join("\n---\n"));
}

#[test]
fn test_page_keys() {
    let pg_up = plain(KeyCode::PageUp);
    let pg_down = plain(KeyCode::PageDown);

    let span1: Span = pg_up.into();
    let span2: Span = pg_down.into();

    let mut buf1 = Buffer::empty(Rect::new(0, 0, 10, 1));
    let mut buf2 = Buffer::empty(Rect::new(0, 0, 10, 1));

    span1.clone().render(buf1.area, &mut buf1);
    span2.clone().render(buf2.area, &mut buf2);

    insta::assert_snapshot!(format!("{buf1:?}\n---\n{buf2:?}"));
}

#[test]
fn test_multiple_modifiers() {
    use crossterm::event::KeyModifiers;

    let binding = KeyBinding::new(
        KeyCode::Char('s'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    let span: Span = binding.into();

    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    span.clone().render(buf.area, &mut buf);

    insta::assert_snapshot!(format!("{buf:?}"));
}
