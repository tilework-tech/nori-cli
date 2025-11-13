use std::time::{Duration, Instant};

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Paragraph, WidgetRef},
};
use tui_components::shimmer::{ColorPalette, Shimmer};

#[cfg(test)]
use ratatui::{backend::TestBackend, Terminal};

// Interactive example application
struct App {
    examples: Vec<ShimmerExample>,
}

struct ShimmerExample {
    label: String,
    shimmer: Shimmer,
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

impl ShimmerExample {
    fn fixed(label: impl Into<String>, shimmer: Shimmer) -> Self {
        Self {
            label: label.into(),
            shimmer,
            mode: AnimationMode::FixedFps,
        }
    }

    fn on_keypress(label: impl Into<String>, shimmer: Shimmer) -> Self {
        Self {
            label: label.into(),
            shimmer,
            mode: AnimationMode::OnKeypress {
                elapsed: Duration::default(),
            },
        }
    }
}

impl App {
    fn new() -> Self {
        let mut examples = Vec::new();

        // Fixed 30 FPS shimmers
        examples.push(ShimmerExample::fixed("Basic Shimmer", Shimmer::new("Loading...")));

        let palette = ColorPalette::new((50, 100, 150), (150, 200, 255));
        let shimmer = Shimmer::with_palette("Processing data...", palette);
        examples.push(ShimmerExample::fixed("Blue Palette", shimmer));

        let palette = ColorPalette::new((50, 150, 50), (150, 255, 150));
        let shimmer = Shimmer::with_palette("Analyzing...", palette);
        examples.push(ShimmerExample::fixed("Green Palette", shimmer));

        let shimmer = Shimmer::new("Loading… 🚀 Processing data 📊");
        examples.push(ShimmerExample::fixed("Unicode Text", shimmer));

        // Keypress-driven shimmers
        let shimmer = Shimmer::new("Performing complex calculations that take some time...");
        examples.push(ShimmerExample::on_keypress("Long Text (Keypress)", shimmer));

        let shimmer = Shimmer::new("");
        examples.push(ShimmerExample::on_keypress("Empty Text (Keypress)", shimmer));

        let palette = ColorPalette::new((100, 50, 150), (200, 150, 255));
        let shimmer = Shimmer::with_palette("Syncing files...", palette);
        examples.push(ShimmerExample::on_keypress("Purple Palette (Keypress)", shimmer));

        let palette = ColorPalette::new((150, 50, 50), (255, 150, 150));
        let shimmer = Shimmer::with_palette("Error checking...", palette);
        examples.push(ShimmerExample::on_keypress("Red Palette (Keypress)", shimmer));

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
        let num_shimmers = self.examples.len();
        let constraints = vec![Constraint::Length(3); num_shimmers];

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(frame.area());

        for (i, example) in self.examples.iter().enumerate() {
            let area = chunks[i];
            let inner_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)])
                .split(area);

            let label_text = Paragraph::new(format!(
                "[ {} • {} ]",
                example.label,
                example.mode.badge()
            ))
            .style(Style::default().fg(Color::Yellow));
            frame.render_widget(label_text, inner_layout[0]);

            match &example.mode {
                AnimationMode::FixedFps => {
                    example.shimmer.render_ref(inner_layout[1], frame.buffer_mut());
                }
                AnimationMode::OnKeypress { elapsed } => {
                    example
                        .shimmer
                        .render_with_elapsed(*elapsed, inner_layout[1], frame.buffer_mut());
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
fn test_shimmer_basic() {
    let shimmer = Shimmer::new("Loading...");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(shimmer, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_shimmer_empty() {
    let shimmer = Shimmer::new("");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(shimmer, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_shimmer_long_text() {
    let shimmer = Shimmer::new("Processing a very long operation that takes time...");
    let mut terminal = Terminal::new(TestBackend::new(60, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(shimmer, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_shimmer_custom_palette() {
    let palette = ColorPalette::new((50, 100, 150), (200, 220, 255));
    let shimmer = Shimmer::with_palette("Custom colors", palette);
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(shimmer, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn test_shimmer_unicode() {
    let shimmer = Shimmer::new("Loading… 🚀");
    let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget_ref(shimmer, frame.area());
        })
        .unwrap();

    insta::assert_snapshot!(terminal.backend());
}
