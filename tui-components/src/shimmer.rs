///! Animated shimmer effect for text
///!
///! Creates a time-based color sweep effect across text, useful for indicating
///! loading or processing states.
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Widget, WidgetRef};

static PROCESS_START: OnceLock<Instant> = OnceLock::new();

fn elapsed_since_start() -> Duration {
    let start = PROCESS_START.get_or_init(Instant::now);
    start.elapsed()
}

/// Color palette for shimmer effect
///
/// Defines the base and highlight colors used for the shimmer animation.
#[derive(Debug, Clone, Copy)]
pub struct ColorPalette {
    /// Base color (RGB) - the normal text color
    pub base: (u8, u8, u8),
    /// Highlight color (RGB) - the bright sweep color
    pub highlight: (u8, u8, u8),
}

impl Default for ColorPalette {
    /// Returns a default grayscale palette suitable for most terminals
    fn default() -> Self {
        Self {
            base: (128, 128, 128),
            highlight: (255, 255, 255),
        }
    }
}

impl ColorPalette {
    /// Creates a new color palette
    ///
    /// # Example
    /// ```
    /// use tui_components::shimmer::ColorPalette;
    ///
    /// let palette = ColorPalette::new((100, 100, 150), (200, 200, 255));
    /// ```
    pub fn new(base: (u8, u8, u8), highlight: (u8, u8, u8)) -> Self {
        Self { base, highlight }
    }
}

/// Animated shimmer effect widget
///
/// Renders text with a sweeping highlight effect, synchronized to process start time.
/// The shimmer creates a band of brightness that moves across the text, making it
/// appear to shimmer or pulse.
///
/// # Examples
///
/// Basic usage with default colors:
/// ```rust,no_run
/// use tui_components::shimmer::Shimmer;
/// use ratatui::widgets::WidgetRef;
///
/// let shimmer = Shimmer::new("Processing...");
/// // Render with WidgetRef::render_ref()
/// ```
///
/// Custom color palette:
/// ```rust,no_run
/// use tui_components::shimmer::{Shimmer, ColorPalette};
///
/// let palette = ColorPalette::new((50, 100, 150), (150, 200, 255));
/// let shimmer = Shimmer::with_palette("Loading...", palette);
/// ```
pub struct Shimmer {
    text: String,
    palette: ColorPalette,
}

impl Shimmer {
    /// Creates a new shimmer with the given text and default color palette
    ///
    /// # Example
    /// ```
    /// use tui_components::shimmer::Shimmer;
    ///
    /// let shimmer = Shimmer::new("Processing...");
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            palette: ColorPalette::default(),
        }
    }

    /// Creates a new shimmer with custom color palette
    ///
    /// # Example
    /// ```
    /// use tui_components::shimmer::{Shimmer, ColorPalette};
    ///
    /// let palette = ColorPalette::new((100, 100, 100), (255, 255, 200));
    /// let shimmer = Shimmer::with_palette("Loading...", palette);
    /// ```
    pub fn with_palette(text: impl Into<String>, palette: ColorPalette) -> Self {
        Self {
            text: text.into(),
            palette,
        }
    }

    /// Returns the spans for the shimmer effect
    fn shimmer_spans(&self) -> Vec<Span<'static>> {
        let chars: Vec<char> = self.text.chars().collect();
        if chars.is_empty() {
            return Vec::new();
        }

        // Use time-based sweep synchronized to process start.
        let padding = 10usize;
        let period = chars.len() + padding * 2;
        let sweep_seconds = 2.0f32;
        let pos_f =
            (elapsed_since_start().as_secs_f32() % sweep_seconds) / sweep_seconds * (period as f32);
        let pos = pos_f as usize;

        let has_true_color = supports_color::on_cached(supports_color::Stream::Stdout)
            .map(|level| level.has_16m)
            .unwrap_or(false);
        let band_half_width = 5.0;

        let mut spans: Vec<Span<'static>> = Vec::with_capacity(chars.len());

        for (i, ch) in chars.iter().enumerate() {
            let i_pos = i as isize + padding as isize;
            let pos = pos as isize;
            let dist = (i_pos - pos).abs() as f32;

            let t = if dist <= band_half_width {
                let x = std::f32::consts::PI * (dist / band_half_width);
                0.5 * (1.0 + x.cos())
            } else {
                0.0
            };

            let style = if has_true_color {
                let highlight = t.clamp(0.0, 1.0);
                let (r, g, b) = blend(self.palette.highlight, self.palette.base, highlight * 0.9);
                Style::default()
                    .fg(Color::Rgb(r, g, b))
                    .add_modifier(Modifier::BOLD)
            } else {
                color_for_level(t)
            };
            spans.push(Span::styled(ch.to_string(), style));
        }
        spans
    }
}

impl WidgetRef for &Shimmer {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let spans = self.shimmer_spans();
        let line = ratatui::text::Line::from(spans);
        line.render(area, buf);
    }
}

impl Widget for Shimmer {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render_ref(area, buf);
    }
}

/// Blends two RGB colors with the given alpha value
fn blend(fg: (u8, u8, u8), bg: (u8, u8, u8), alpha: f32) -> (u8, u8, u8) {
    let r = (fg.0 as f32 * alpha + bg.0 as f32 * (1.0 - alpha)) as u8;
    let g = (fg.1 as f32 * alpha + bg.1 as f32 * (1.0 - alpha)) as u8;
    let b = (fg.2 as f32 * alpha + bg.2 as f32 * (1.0 - alpha)) as u8;
    (r, g, b)
}

/// Returns a style based on intensity for terminals without true color support
fn color_for_level(intensity: f32) -> Style {
    // Tune fallback styling so the shimmer band reads even without RGB support.
    if intensity < 0.2 {
        Style::default().add_modifier(Modifier::DIM)
    } else if intensity < 0.6 {
        Style::default()
    } else {
        Style::default().add_modifier(Modifier::BOLD)
    }
}
