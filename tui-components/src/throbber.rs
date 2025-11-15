//! Animated throbber/spinner effect for text
//!
//! Creates a time-based spinner animation with text, useful for indicating
//! loading or processing states.
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::{Widget, WidgetRef};

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

fn elapsed_since_start() -> Duration {
    let start = PROCESS_START.get_or_init(Instant::now);
    start.elapsed()
}

/// Animated throbber/spinner widget
///
/// Renders text with a cycling spinner animation, synchronized to process start time.
/// The throbber displays a rotating spinner character followed by the provided text.
///
/// # Examples
///
/// Basic usage with default styling:
/// ```rust,no_run
/// use tui_components::throbber::Throbber;
/// use ratatui::widgets::WidgetRef;
///
/// let throbber = Throbber::new("Processing...");
/// // Render with WidgetRef::render_ref()
/// ```
///
/// Custom spinner frames:
/// ```rust,no_run
/// use tui_components::throbber::Throbber;
///
/// let frames = ["|", "/", "-", "\\"];
/// let throbber = Throbber::with_frames("Loading...", frames);
/// ```
pub struct Throbber {
    text: String,
    frames: Vec<String>,
}

impl Throbber {
    /// Creates a new throbber with the given text and default spinner frames
    ///
    /// Uses the standard spinner frames: ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
    ///
    /// # Example
    /// ```
    /// use tui_components::throbber::Throbber;
    ///
    /// let throbber = Throbber::new("Processing...");
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            frames: vec![
                "⠋".to_string(),
                "⠙".to_string(),
                "⠹".to_string(),
                "⠸".to_string(),
                "⠼".to_string(),
                "⠴".to_string(),
                "⠦".to_string(),
                "⠧".to_string(),
                "⠇".to_string(),
                "⠏".to_string(),
            ],
        }
    }

    /// Creates a new throbber with custom spinner frames
    ///
    /// # Example
    /// ```
    /// use tui_components::throbber::Throbber;
    ///
    /// let frames = ["|", "/", "-", "\\"];
    /// let throbber = Throbber::with_frames("Loading...", frames);
    /// ```
    pub fn with_frames(text: impl Into<String>, frames: &[&str]) -> Self {
        Self {
            text: text.into(),
            frames: frames.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Returns the current spinner frame and text at the provided elapsed time
    fn throbber_text_at(&self, elapsed: Duration) -> String {
        if self.frames.is_empty() {
            return self.text.clone();
        }

        // Calculate which frame to show based on elapsed time
        let frame_duration = Duration::from_millis(100); // 100ms per frame
        let frame_index =
            (elapsed.as_millis() / frame_duration.as_millis()) as usize % self.frames.len();
        let spinner = &self.frames[frame_index];

        if self.text.is_empty() {
            spinner.clone()
        } else {
            format!("{} {}", spinner, self.text)
        }
    }

    /// Renders the throbber at a manually supplied elapsed time
    pub fn render_with_elapsed(&self, elapsed: Duration, area: Rect, buf: &mut Buffer) {
        let text = self.throbber_text_at(elapsed);
        let span = Span::styled(text, Style::default().fg(Color::Cyan));
        let line = ratatui::text::Line::from(span);
        line.render(area, buf);
    }
}

impl WidgetRef for Throbber {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        self.render_with_elapsed(elapsed_since_start(), area, buf);
    }
}

impl Widget for Throbber {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.render_ref(area, buf);
    }
}
