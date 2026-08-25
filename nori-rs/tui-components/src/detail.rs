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

/// Vertical spacing between adjacent detail entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailDensity {
    /// Render adjacent entries without an intervening blank row.
    #[default]
    Compact,
    /// Leave one blank row between adjacent key/value entries.
    Normal,
}

/// Horizontal organization for detail labels and values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailLayout {
    /// Render labels and values in two aligned columns.
    #[default]
    Columns,
    /// Render each label above a two-cell-inset value.
    Stacked,
    /// Stack below the caller-rectangle width and use columns otherwise.
    Responsive { stack_below: u16 },
}

/// Background treatment for logical key/value entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailRowPattern {
    /// Keep every entry on the pane surface.
    #[default]
    Plain,
    /// Alternate `Theme::row` and `Theme::row_alt` by logical entry.
    Zebra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResolvedDetailLayout {
    Columns,
    Stacked,
}

impl DetailLayout {
    fn resolve(self, width: u16) -> ResolvedDetailLayout {
        match self {
            Self::Columns => ResolvedDetailLayout::Columns,
            Self::Stacked => ResolvedDetailLayout::Stacked,
            Self::Responsive { stack_below } if width < stack_below => {
                ResolvedDetailLayout::Stacked
            }
            Self::Responsive { .. } => ResolvedDetailLayout::Columns,
        }
    }
}

/// A presentation-only definition-list widget.
pub struct DetailPane<'a> {
    entries: &'a [DetailEntry],
    heading: Option<Line<'static>>,
    theme: Theme,
    label_width: LabelWidth,
    density: DetailDensity,
    layout: DetailLayout,
    row_pattern: DetailRowPattern,
}

impl<'a> DetailPane<'a> {
    pub fn new(entries: &'a [DetailEntry]) -> Self {
        Self {
            entries,
            heading: None,
            theme: Theme::default(),
            label_width: LabelWidth::default(),
            density: DetailDensity::default(),
            layout: DetailLayout::default(),
            row_pattern: DetailRowPattern::default(),
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

    pub fn density(mut self, density: DetailDensity) -> Self {
        self.density = density;
        self
    }

    pub fn layout(mut self, layout: DetailLayout) -> Self {
        self.layout = layout;
        self
    }

    pub fn row_pattern(mut self, row_pattern: DetailRowPattern) -> Self {
        self.row_pattern = row_pattern;
        self
    }

    /// Returns the rows required to render this pane at the caller rectangle width.
    pub fn required_height(&self, width: u16) -> u16 {
        let Some(content_width) = detail_content_width(width) else {
            return 0;
        };
        let layout = self.layout.resolve(width);
        let gutter = gutter_width(self.entries, self.label_width, content_width);
        let mut height = u16::from(self.heading.is_some()) * 2;
        for (index, entry) in self.entries.iter().enumerate() {
            height = height.saturating_add(entry_height(entry, layout, content_width, gutter));
            if self.density == DetailDensity::Normal
                && matches!(entry, DetailEntry::KeyValue { .. })
                && matches!(
                    self.entries.get(index + 1),
                    Some(DetailEntry::KeyValue { .. })
                )
            {
                height = height.saturating_add(1);
            }
        }
        height
    }
}

impl Widget for DetailPane<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if detail_content_width(area.width).is_none() || area.height == 0 {
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

        let layout = self.layout.resolve(area.width);
        let mut y = content.y;
        if let Some(heading) = self.heading {
            Paragraph::new(heading)
                .style(self.theme.title)
                .render(Rect::new(content.x, y, content.width, 1), buf);
            y = y.saturating_add(2);
        }
        let gutter = gutter_width(self.entries, self.label_width, content.width);
        let mut zebra_index = 0usize;
        for (index, entry) in self.entries.iter().enumerate() {
            if y >= content.bottom() {
                break;
            }
            match entry {
                DetailEntry::Rule => {
                    y = y.saturating_add(1);
                    zebra_index = 0;
                }
                DetailEntry::KeyValue {
                    label,
                    value,
                    tone,
                    wrap,
                } => {
                    let full_height = key_value_height(value, *wrap, layout, content.width, gutter);
                    let row_height = full_height.min(content.bottom().saturating_sub(y));
                    let row_style = match self.row_pattern {
                        DetailRowPattern::Plain => self.theme.detail_surface,
                        DetailRowPattern::Zebra if zebra_index.is_multiple_of(2) => {
                            self.theme.detail_surface.patch(self.theme.row)
                        }
                        DetailRowPattern::Zebra => {
                            self.theme.detail_surface.patch(self.theme.row_alt)
                        }
                    };
                    let zebra_background = (self.row_pattern == DetailRowPattern::Zebra)
                        .then(|| row_style.bg.map(|color| Style::new().bg(color)))
                        .flatten();
                    buf.set_style(
                        Rect::new(surface.x, y, surface.width, row_height),
                        row_style,
                    );

                    match layout {
                        ResolvedDetailLayout::Columns => {
                            let label =
                                pad_right(&truncate(label, gutter as usize), gutter as usize);
                            buf.set_string(
                                content.x,
                                y,
                                label,
                                content_style(row_style, self.theme.muted, zebra_background),
                            );
                            let value_x = content.x.saturating_add(gutter).saturating_add(2);
                            let value_width = content.right().saturating_sub(value_x);
                            render_value(
                                value,
                                *wrap,
                                Rect::new(value_x, y, value_width, row_height),
                                content_style(
                                    row_style,
                                    tone_style(*tone, self.theme),
                                    zebra_background,
                                ),
                                zebra_background,
                                buf,
                            );
                        }
                        ResolvedDetailLayout::Stacked => {
                            buf.set_string(
                                content.x,
                                y,
                                truncate(label, content.width as usize),
                                content_style(row_style, self.theme.muted, zebra_background),
                            );
                            let value_x = content.x.saturating_add(2);
                            let value_width = content.right().saturating_sub(value_x);
                            render_value(
                                value,
                                *wrap,
                                Rect::new(
                                    value_x,
                                    y.saturating_add(1),
                                    value_width,
                                    row_height.saturating_sub(1),
                                ),
                                content_style(
                                    row_style,
                                    tone_style(*tone, self.theme),
                                    zebra_background,
                                ),
                                zebra_background,
                                buf,
                            );
                        }
                    }

                    y = y.saturating_add(row_height);
                    zebra_index = zebra_index.saturating_add(1);
                    if row_height == full_height
                        && self.density == DetailDensity::Normal
                        && matches!(
                            self.entries.get(index + 1),
                            Some(DetailEntry::KeyValue { .. })
                        )
                    {
                        y = y.saturating_add(1);
                    }
                }
            }
        }
    }
}

fn detail_content_width(width: u16) -> Option<u16> {
    (width >= 7).then(|| width.saturating_sub(4))
}

fn entry_height(
    entry: &DetailEntry,
    layout: ResolvedDetailLayout,
    content_width: u16,
    gutter: u16,
) -> u16 {
    match entry {
        DetailEntry::Rule => 1,
        DetailEntry::KeyValue { value, wrap, .. } => {
            key_value_height(value, *wrap, layout, content_width, gutter)
        }
    }
}

fn key_value_height(
    value: &Line<'static>,
    wrap: bool,
    layout: ResolvedDetailLayout,
    content_width: u16,
    gutter: u16,
) -> u16 {
    match layout {
        ResolvedDetailLayout::Columns => {
            value_height(value, wrap, content_width.saturating_sub(gutter + 2)).max(1)
        }
        ResolvedDetailLayout::Stacked => {
            1u16.saturating_add(value_height(value, wrap, content_width.saturating_sub(2)))
        }
    }
}

fn value_height(value: &Line<'static>, wrap: bool, width: u16) -> u16 {
    if width == 0 {
        return 0;
    }
    if !wrap {
        return 1;
    }
    u16::try_from(
        Paragraph::new(value.clone())
            .wrap(ratatui::widgets::Wrap { trim: false })
            .line_count(width)
            .max(1),
    )
    .unwrap_or(u16::MAX)
}

fn render_value(
    value: &Line<'static>,
    wrap: bool,
    area: Rect,
    style: Style,
    background: Option<Style>,
    buf: &mut Buffer,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let mut rendered = if wrap {
        value.clone()
    } else {
        truncate_line(value, area.width as usize)
    };
    if let Some(background) = background {
        rendered.style = rendered.style.patch(background);
        for span in &mut rendered.spans {
            span.style = span.style.patch(background);
        }
    }
    Paragraph::new(rendered)
        .style(style)
        .wrap(ratatui::widgets::Wrap { trim: false })
        .render(area, buf);
}

fn content_style(row_style: Style, semantic_style: Style, background: Option<Style>) -> Style {
    let style = row_style.patch(semantic_style);
    background.map_or(style, |background| style.patch(background))
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
    if width == 0 {
        return String::new();
    }

    let content_width = width.saturating_sub(1);
    let end = value
        .char_indices()
        .map(|(index, character)| index + character.len_utf8())
        .take_while(|end| value[..*end].width() <= content_width)
        .last()
        .unwrap_or(0);
    format!("{}…", &value[..end])
}

fn truncate_line(line: &Line<'static>, width: usize) -> Line<'static> {
    Line::from(truncate(&line.to_string(), width))
}

fn pad_right(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}

#[cfg(test)]
mod tests;
