use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::Theme;

/// Semantic severity for user-facing status and alert text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MessageLevel {
    #[default]
    Info,
    Success,
    Warning,
    Error,
}

impl MessageLevel {
    fn marker(self) -> &'static str {
        match self {
            Self::Info => "i",
            Self::Success => "✓",
            Self::Warning => "!",
            Self::Error => "×",
        }
    }

    fn style(self, theme: &Theme) -> Style {
        match self {
            Self::Info => theme.info,
            Self::Success => theme.success,
            Self::Warning => theme.warning,
            Self::Error => theme.error,
        }
    }
}

/// A compact semantic message with an optional supporting detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticMessage<'a> {
    pub level: MessageLevel,
    pub message: Cow<'a, str>,
    pub detail: Option<Cow<'a, str>>,
    pub theme: Theme,
}

impl<'a> SemanticMessage<'a> {
    pub fn new(level: MessageLevel, message: impl Into<Cow<'a, str>>) -> Self {
        Self {
            level,
            message: message.into(),
            detail: None,
            theme: Theme::default(),
        }
    }

    pub fn detail(mut self, detail: impl Into<Cow<'a, str>>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for SemanticMessage<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let marker_style = self.level.style(&self.theme);
        let mut lines = vec![Line::from(vec![
            Span::styled(format!("{} ", self.level.marker()), marker_style),
            Span::styled(self.message.into_owned(), self.theme.text),
        ])];
        if let Some(detail) = self.detail {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(detail.into_owned(), self.theme.muted),
            ]));
        }
        Paragraph::new(lines).render(area, buf);
    }
}

/// Empty, loading, no-match, and failed content state used by composites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyState<'a> {
    pub title: Cow<'a, str>,
    pub detail: Option<Cow<'a, str>>,
    pub marker: Cow<'a, str>,
    pub theme: Theme,
}

impl<'a> EmptyState<'a> {
    pub fn new(title: impl Into<Cow<'a, str>>) -> Self {
        Self {
            title: title.into(),
            detail: None,
            marker: "◇".into(),
            theme: Theme::default(),
        }
    }

    pub fn detail(mut self, detail: impl Into<Cow<'a, str>>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn marker(mut self, marker: impl Into<Cow<'a, str>>) -> Self {
        self.marker = marker.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for EmptyState<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines = vec![Line::from(vec![
            Span::styled(format!("{} ", self.marker), self.theme.info),
            Span::styled(self.title.into_owned(), self.theme.text),
        ])];
        if let Some(detail) = self.detail {
            lines.push(Line::from(Span::styled(
                format!("  {detail}"),
                self.theme.muted,
            )));
        }
        Paragraph::new(lines).render(area, buf);
    }
}

/// A key label and its human-readable action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyHint<'a> {
    pub key: Cow<'a, str>,
    pub action: Cow<'a, str>,
}

impl<'a> KeyHint<'a> {
    pub fn new(key: impl Into<Cow<'a, str>>, action: impl Into<Cow<'a, str>>) -> Self {
        Self {
            key: key.into(),
            action: action.into(),
        }
    }
}

/// A compact, wrapping footer of keyboard hints.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyHints<'a> {
    pub hints: Vec<KeyHint<'a>>,
    pub theme: Theme,
}

impl<'a> KeyHints<'a> {
    pub fn new(hints: impl IntoIterator<Item = KeyHint<'a>>) -> Self {
        Self {
            hints: hints.into_iter().collect(),
            theme: Theme::default(),
        }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }
}

impl Widget for KeyHints<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let spans = self
            .hints
            .into_iter()
            .enumerate()
            .flat_map(|(index, hint)| {
                let separator = (index > 0).then(|| Span::styled("  ", self.theme.muted));
                separator.into_iter().chain([
                    Span::styled(hint.key.into_owned(), self.theme.pointer),
                    Span::raw(" "),
                    Span::styled(hint.action.into_owned(), self.theme.muted),
                ])
            })
            .collect::<Vec<_>>();
        Paragraph::new(Line::from(spans))
            .wrap(ratatui::widgets::Wrap { trim: true })
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests;
