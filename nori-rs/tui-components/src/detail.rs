//! Stateless definition-list rendering for caller-owned panes and overlays.
//!
//! `DetailPane` deliberately accepts a caller-provided rectangle. It does not
//! choose a side or bottom placement, manage focus, collect input, or retain
//! scrolling state.

use ratatui::buffer::Buffer;
use ratatui::layout::Margin;
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
    Gemini,
    Antigravity,
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
}

impl<'a> DetailPane<'a> {
    pub fn new(entries: &'a [DetailEntry]) -> Self {
        Self {
            entries,
            heading: None,
            theme: Theme::default(),
            label_width: LabelWidth::default(),
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
}

impl Widget for DetailPane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 4 || area.height == 0 {
            return;
        }
        let surface = Rect::new(
            area.x.saturating_add(1),
            area.y,
            area.width.saturating_sub(2),
            area.height,
        );
        buf.set_style(surface, self.theme.detail_surface);
        let content = surface.inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
        if content.width == 0 {
            return;
        }

        let mut y = content.y;
        if let Some(heading) = self.heading {
            Paragraph::new(heading)
                .style(self.theme.title)
                .render(Rect::new(content.x, y, content.width, 1), buf);
            y = y.saturating_add(2);
        }
        let gutter = gutter_width(self.entries, self.label_width, content.width);
        for entry in self.entries {
            if y >= content.bottom() {
                break;
            }
            match entry {
                DetailEntry::Rule => {
                    y = y.saturating_add(1);
                }
                DetailEntry::KeyValue {
                    label,
                    value,
                    tone,
                    wrap,
                } => {
                    let label = pad_right(&truncate(label, gutter as usize), gutter as usize);
                    buf.set_string(content.x, y, label, self.theme.muted);
                    let value_x = content.x.saturating_add(gutter).saturating_add(2);
                    let value_width = content.right().saturating_sub(value_x);
                    if value_width == 0 {
                        break;
                    }
                    let rendered = if *wrap {
                        value.clone()
                    } else {
                        truncate_line(value, value_width as usize)
                    };
                    let paragraph = Paragraph::new(rendered)
                        .style(tone_style(*tone, self.theme))
                        .wrap(ratatui::widgets::Wrap { trim: false });
                    let row_height = if *wrap {
                        paragraph.line_count(value_width).max(1) as u16
                    } else {
                        1
                    }
                    .min(content.bottom().saturating_sub(y));
                    let value_area = Rect::new(value_x, y, value_width, row_height);
                    paragraph.render(value_area, buf);
                    y = y.saturating_add(row_height);
                }
            }
        }
    }
}

fn gutter_width(entries: &[DetailEntry], policy: LabelWidth, width: u16) -> u16 {
    let available = width.saturating_sub(2) / 2;
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
        DetailTone::Info => theme.info,
        DetailTone::Success => theme.success,
        DetailTone::Warning | DetailTone::Provider(ProviderKind::Pi) => theme.warning,
        DetailTone::Error => theme.error,
        DetailTone::Provider(ProviderKind::Other) => theme.text,
        DetailTone::Provider(provider) => provider_tone_style(provider, theme),
    }
}

pub(crate) fn provider_tone_style(provider: ProviderKind, theme: Theme) -> Style {
    match provider {
        ProviderKind::Claude => theme.provider_claude,
        ProviderKind::Codex => theme.provider_codex,
        ProviderKind::Gemini => theme.provider_gemini,
        ProviderKind::Antigravity => theme.provider_antigravity,
        ProviderKind::Nori => theme.provider_nori,
        ProviderKind::Pi => theme.warning,
        ProviderKind::Other => theme.text,
    }
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

fn pad_right(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}

#[cfg(test)]
mod tests;
