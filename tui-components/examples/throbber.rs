use std::time::{Duration, Instant};

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Paragraph, WidgetRef},
};
use tui_components::throbber::Throbber;

#[cfg(test)]
use ratatui::{Terminal, backend::TestBackend};

// Interactive example application
struct App {
    examples: Vec<ThrobberExample>,
}

struct ThrobberExample {
    label: String,
    throbber: Throbber,
    mode: AnimationMode,
}

enum AnimationMode {
    FixedFps,
    OnKeypress { elapsed: Duration },
}

impl AnimationMode {
    fn badge(&self) -> &'static str {
        match self {
            AnimationMode::FixedFps => "30 FPS",
            AnimationMode::OnKeypress { .. } => "On Key",
        }
    }
}

impl ThrobberExample {
    fn fixed(label: impl Into<String>, throbber: Throbber) -> Self {
        Self {
            label: label.into(),
            throbber,
            mode: AnimationMode::FixedFps,
        }
    }

    fn on_keypress(label: impl Into<String>, throbber: Throbber) -> Self {
        Self {
            label: label.into(),
            throbber,
            mode: AnimationMode::OnKeypress {
                elapsed: Duration::default(),
            },
        }
    }
}

impl App {
    fn new() -> Self {
        let mut examples = Vec::new();

        // Fixed 30 FPS throbbers
        examples.push(ThrobberExample::fixed(
            "Basic Throbber",
            Throbber::new("Loading..."),
        ));

        let frames = ["|", "/", "-", "\\"];
        let throbber = Throbber::with_frames("Processing data...", &frames);
        examples.push(ThrobberExample::fixed("ASCII Frames", throbber));

        let frames = ["◐", "◓", "◑", "◒"];
        let throbber = Throbber::with_frames("Analyzing...", &frames);
        examples.push(ThrobberExample::fixed("Circle Frames", throbber));

        let throbber = Throbber::new("Loading… 🚀 Processing data 📊");
        examples.push(ThrobberExample::fixed("Unicode Text", throbber));

        // Keypress-driven throbbers
        let throbber = Throbber::new("Performing complex calculations that take some time...");
        examples.push(ThrobberExample::on_keypress(
            "Long Text (Keypress)",
            throbber,
        ));

        let throbber = Throbber::new("");
        examples.push(ThrobberExample::on_keypress(
            "Empty Text (Keypress)",
            throbber,
        ));

        let frames = ["▏", "▎", "▍", "▌", "▋", "▊", "▉", "█"];
        let throbber = Throbber::with_frames("Syncing files...", &frames);
        examples.push(ThrobberExample::on_keypress(
            "Progress Bar (Keypress)",
            throbber,
        ));

        let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let throbber = Throbber::with_frames("Error checking...", &frames);
        examples.push(ThrobberExample::on_keypress(
            "Braille Frames (Keypress)",
            throbber,
        ));

        Self { examples }
    }

    fn run(&mut self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        let tick_rate = Duration::from_millis(33);
        let mut last_tick = Instant::now();

        loop {
            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_millis(0));
            let mut should_redraw = false;

            if event::poll(timeout)? {
                match event::read()? {
                    Event::Key(key) => {
                        if key.code == KeyCode::Esc
                            || (key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL))
                        {
                            break;
                        }

                        self.advance_manual_examples(tick_rate);
                        should_redraw = true;
                    }
                    Event::Resize(_, _) => {
                        should_redraw = true;
                    }
                    _ => {}
                }
            }

            if last_tick.elapsed() >= tick_rate {
                last_tick = Instant::now();
                should_redraw = true;
            }

            if should_redraw {
                terminal.draw(|frame| self.draw(frame))?;
            }
        }
        Ok(())
    }

    fn advance_manual_examples(&mut self, delta: Duration) {
        for example in &mut self.examples {
            if let AnimationMode::OnKeypress { elapsed } = &mut example.mode {
                *elapsed += delta;
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let num_throbbers = self.examples.len();
        let constraints = vec![Constraint::Length(3); num_throbbers];

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(frame.area());

        for (i, example) in self.examples.iter().enumerate() {
            let area = chunks[i];
            let inner_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(area);

            let label_text =
                Paragraph::new(format!("[ {} • {} ]", example.label, example.mode.badge()))
                    .style(Style::default().fg(Color::Yellow));
            frame.render_widget(label_text, inner_layout[0]);

            match &example.mode {
                AnimationMode::FixedFps => {
                    example
                        .throbber
                        .render_ref(inner_layout[1], frame.buffer_mut());
                }
                AnimationMode::OnKeypress { elapsed } => {
                    example.throbber.render_with_elapsed(
                        *elapsed,
                        inner_layout[1],
                        frame.buffer_mut(),
                    );
                }
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

#[test]
fn test_throbber_basic() {
    let throbber = Throbber::new("Loading...");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(throbber, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_throbber_empty() {
    let throbber = Throbber::new("");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(throbber, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_throbber_long_text() {
    let throbber = Throbber::new("Processing a very long operation that takes time...");
    let mut terminal = Terminal::new(TestBackend::new(60, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(throbber, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_throbber_custom_frames() {
    let frames = ["|", "/", "-", "\\"];
    let throbber = Throbber::with_frames("Custom frames", &frames);
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(throbber, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_throbber_unicode() {
    let throbber = Throbber::new("Loading… 🚀");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(throbber, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}
