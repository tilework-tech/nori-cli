//! Stateless definition-list rendering for caller-owned panes and overlays.
//!
//! `DetailPane` deliberately accepts a caller-provided rectangle. It does not
//! choose a side or bottom placement, manage focus, collect input, or retain
//! scrolling state.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::Theme;

/// Semantic emphasis for a detail value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailTone {
    #[default]
    Default,
    Muted,
    Info,
    Success,
    Warning,
    Error,
    Provider(ProviderKind),
}

/// Known agent/provider identities, with an extensible fallback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProviderKind {
    Claude,
    Codex,
    Pi,
    Nori,
    #[default]
    Other,
}

/// A semantic line in a detail pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetailEntry {
    KeyValue {
        label: String,
        value: Line<'static>,
        tone: DetailTone,
        wrap: bool,
    },
    Rule,
}

impl DetailEntry {
    pub fn key_value(label: impl Into<String>, value: impl Into<Line<'static>>) -> Self {
        Self::KeyValue {
            label: label.into().trim_end_matches(':').to_string(),
            value: value.into(),
            tone: DetailTone::Default,
            wrap: false,
        }
    }

    pub fn muted(label: impl Into<String>, value: impl Into<Line<'static>>) -> Self {
        Self::key_value(label, value)
            .tone(DetailTone::Muted)
            .wrap(true)
    }

    pub fn tone(mut self, tone: DetailTone) -> Self {
        if let Self::KeyValue {
            tone: entry_tone, ..
        } = &mut self
        {
            *entry_tone = tone;
        }
        self
    }

    pub fn wrap(mut self, wrap: bool) -> Self {
        if let Self::KeyValue {
            wrap: entry_wrap, ..
        } = &mut self
        {
            *entry_wrap = wrap;
        }
        self
    }
}

/// Label-gutter sizing policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelWidth {
    Auto { max: u16 },
    Fixed(u16),
}

/// Which neutral surface layer the pane paints inside its caller-owned area.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailBackground {
    /// Leave the caller's background untouched.
    Transparent,
    /// Shade the complete pane area.
    #[default]
    Pane,
    /// Shade only the optional heading row.
    Heading,
    /// Shade only the label gutter and separator column.
    LabelGutter,
    /// Shade each key-value row, including wrapped continuation lines.
    Rows,
}

impl Default for LabelWidth {
    fn default() -> Self {
        Self::Auto { max: 14 }
    }
}

/// A presentation-only definition-list widget.
pub struct DetailPane<'a> {
    entries: &'a [DetailEntry],
    heading: Option<Line<'static>>,
    theme: Theme,
    label_width: LabelWidth,
    background: DetailBackground,
}

impl<'a> DetailPane<'a> {
    pub fn new(entries: &'a [DetailEntry]) -> Self {
        Self {
            entries,
            heading: None,
            theme: Theme::default(),
            label_width: LabelWidth::default(),
            background: DetailBackground::default(),
        }
    }

    pub fn heading(mut self, heading: impl Into<Line<'static>>) -> Self {
        self.heading = Some(heading.into());
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn label_width(mut self, label_width: LabelWidth) -> Self {
        self.label_width = label_width;
        self
    }

    pub fn background(mut self, background: DetailBackground) -> Self {
        self.background = background;
        self
    }
}

impl Widget for DetailPane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height == 0 {
            return;
        }
        if self.background == DetailBackground::Pane {
            buf.set_style(area, self.theme.detail_surface);
        }
        let mut y = area.y;
        if let Some(heading) = self.heading {
            if self.background == DetailBackground::Heading {
                buf.set_style(
                    Rect::new(area.x, y, area.width, 1),
                    self.theme.detail_surface,
                );
            }
            Paragraph::new(heading)
                .style(self.theme.title)
                .render(Rect::new(area.x, y, area.width, 1), buf);
            y = y.saturating_add(2);
        }
        let gutter = gutter_width(self.entries, self.label_width, area.width);
        for entry in self.entries {
            if y >= area.bottom() {
                break;
            }
            match entry {
                DetailEntry::Rule => {
                    buf.set_string(
                        area.x,
                        y,
                        "─".repeat(area.width as usize),
                        self.theme.separator,
                    );
                    y = y.saturating_add(1);
                }
                DetailEntry::KeyValue {
                    label,
                    value,
                    tone,
                    wrap,
                } => {
                    let value_width = area.width.saturating_sub(gutter).saturating_sub(3);
                    let row_height = if *wrap {
                        line_height(value, value_width as usize)
                    } else {
                        1
                    }
                    .min(area.bottom().saturating_sub(y));
                    match self.background {
                        DetailBackground::LabelGutter => buf.set_style(
                            Rect::new(area.x, y, gutter.saturating_add(2), row_height),
                            self.theme.detail_surface,
                        ),
                        DetailBackground::Rows => buf.set_style(
                            Rect::new(area.x, y, area.width, row_height),
                            self.theme.detail_surface,
                        ),
                        DetailBackground::Transparent
                        | DetailBackground::Pane
                        | DetailBackground::Heading => {}
                    }
                    let label = pad_left(&truncate(label, gutter as usize), gutter as usize);
                    buf.set_string(area.x, y, label, self.theme.muted);
                    let separator_x = area.x.saturating_add(gutter).saturating_add(1);
                    buf.set_string(separator_x, y, "│", self.theme.separator);
                    let value_x = separator_x.saturating_add(2);
                    let value_width = area.right().saturating_sub(value_x);
                    if value_width == 0 {
                        break;
                    }
                    let value_area =
                        Rect::new(value_x, y, value_width, area.bottom().saturating_sub(y));
                    let rendered = if *wrap {
                        value.clone()
                    } else {
                        truncate_line(value, value_width as usize)
                    };
                    Paragraph::new(rendered)
                        .style(tone_style(*tone, self.theme))
                        .wrap(ratatui::widgets::Wrap { trim: false })
                        .render(value_area, buf);
                    y = y.saturating_add(row_height);
                }
            }
        }
    }
}

fn gutter_width(entries: &[DetailEntry], policy: LabelWidth, width: u16) -> u16 {
    let available = width.saturating_sub(4) / 2;
    match policy {
        LabelWidth::Fixed(width) => width.min(available),
        LabelWidth::Auto { max } => entries
            .iter()
            .filter_map(|entry| match entry {
                DetailEntry::KeyValue { label, .. } => Some(label.width() as u16),
                DetailEntry::Rule => None,
            })
            .max()
            .unwrap_or(0)
            .min(max)
            .min(available),
    }
}

fn tone_style(tone: DetailTone, theme: Theme) -> Style {
    match tone {
        DetailTone::Default => theme.text,
        DetailTone::Muted => theme.muted,
        DetailTone::Info
        | DetailTone::Provider(ProviderKind::Codex)
        | DetailTone::Provider(ProviderKind::Nori) => theme.info,
        DetailTone::Success | DetailTone::Provider(ProviderKind::Claude) => theme.success,
        DetailTone::Warning | DetailTone::Provider(ProviderKind::Pi) => theme.warning,
        DetailTone::Error => theme.error,
        DetailTone::Provider(ProviderKind::Other) => theme.text,
    }
}

fn line_height(line: &Line<'_>, width: usize) -> u16 {
    if width == 0 {
        return 1;
    }
    let text = line.to_string();
    ((text.width().saturating_add(width - 1) / width).max(1)) as u16
}

fn truncate(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_string();
    }
    value
        .chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

fn truncate_line(line: &Line<'static>, width: usize) -> Line<'static> {
    Line::from(truncate(&line.to_string(), width))
}

fn pad_left(value: &str, width: usize) -> String {
    format!("{value:>width$}")
}

#[cfg(test)]
mod tests;
