use std::borrow::Cow;

use pulldown_cmark::CodeBlockKind;
use pulldown_cmark::Event;
use pulldown_cmark::HeadingLevel;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::Theme;

mod table;
mod text;

use table::TableBuilder;
use text::wrap_line;

/// Width-aware Markdown content. Rendering is deterministic and owns no
/// terminal or streaming lifecycle.
#[derive(Clone, Debug)]
pub struct Markdown<'a> {
    source: Cow<'a, str>,
    width: Option<u16>,
    theme: Theme,
}

impl<'a> Markdown<'a> {
    pub fn new(source: impl Into<Cow<'a, str>>) -> Self {
        Self {
            source: source.into(),
            width: None,
            theme: Theme::default(),
        }
    }

    pub fn width(mut self, width: u16) -> Self {
        self.width = Some(width);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn render_text(&self) -> Text<'static> {
        Writer::new(self.width, self.theme).render(&self.source)
    }
}

impl Widget for Markdown<'_> {
    fn render(mut self, area: Rect, buf: &mut Buffer) {
        if self.width.is_none() {
            self.width = Some(area.width);
        }
        Paragraph::new(self.render_text()).render(area, buf);
    }
}

/// Incremental source buffer for consumers receiving Markdown in chunks.
/// Each frame is rendered from the complete buffered source so incomplete
/// Markdown remains safe while subsequent chunks can refine its structure.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamingMarkdown {
    source: String,
}

impl StreamingMarkdown {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_str(&mut self, chunk: &str) {
        self.source.push_str(chunk);
    }

    pub fn clear(&mut self) {
        self.source.clear();
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn markdown(&self) -> Markdown<'_> {
        Markdown::new(&self.source)
    }
}

struct Writer {
    width: Option<u16>,
    theme: Theme,
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    styles: Vec<Style>,
    list_stack: Vec<Option<u64>>,
    prefix_stack: Vec<String>,
    pending_prefix: Option<String>,
    link: Option<String>,
    code_block: bool,
    table: Option<TableBuilder>,
}

impl Writer {
    fn new(width: Option<u16>, theme: Theme) -> Self {
        Self {
            width,
            theme,
            lines: Vec::new(),
            current: Vec::new(),
            styles: vec![theme.text],
            list_stack: Vec::new(),
            prefix_stack: Vec::new(),
            pending_prefix: None,
            link: None,
            code_block: false,
            table: None,
        }
    }

    fn render(mut self, source: &str) -> Text<'static> {
        let parser = Parser::new_ext(
            source,
            Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS,
        )
        .into_offset_iter();
        for (event, source_range) in parser {
            self.event(event, source_range, source);
        }
        self.flush();
        while self.lines.last().is_some_and(line_is_empty) {
            self.lines.pop();
        }
        Text::from(self.lines)
    }

    fn event(&mut self, event: Event<'_>, source_range: std::ops::Range<usize>, source: &str) {
        if self.table.is_some() {
            self.table_event(event, source_range, source);
            return;
        }
        match event {
            Event::Start(Tag::Table(alignments)) => {
                self.flush();
                self.table = Some(TableBuilder::new(alignments));
            }
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.push_text(&text),
            Event::Code(code) => self.push_span(Span::styled(code.into_string(), self.theme.code)),
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                self.lines
                    .push(Line::styled("─".repeat(24), self.theme.table_rule));
            }
            Event::Html(html) | Event::InlineHtml(html) => self.push_text(&html),
            Event::FootnoteReference(reference) => self.push_text(&format!("[{reference}]")),
            Event::TaskListMarker(checked) => {
                self.push_text(if checked { "[x] " } else { "[ ] " });
            }
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.ensure_separation(),
            Tag::Heading { level, .. } => {
                self.ensure_separation();
                self.pending_prefix = Some(format!("{} ", "#".repeat(level as usize)));
                self.push_style(heading_style(level));
            }
            Tag::BlockQuote => self.prefix_stack.push("> ".to_string()),
            Tag::CodeBlock(kind) => {
                self.ensure_separation();
                self.code_block = true;
                if let CodeBlockKind::Fenced(language) = kind {
                    let language = language.split_whitespace().next().unwrap_or_default();
                    if !language.is_empty() {
                        self.lines
                            .push(Line::styled(format!("  {language}"), self.theme.muted));
                    }
                }
                self.prefix_stack.push("  ".to_string());
                self.push_style(self.theme.code);
            }
            Tag::List(start) => {
                self.flush();
                self.list_stack.push(start);
            }
            Tag::Item => {
                let depth = self.list_stack.len().saturating_sub(1);
                let marker = match self.list_stack.last_mut() {
                    Some(Some(index)) => {
                        let marker = format!("{index}. ");
                        *index += 1;
                        marker
                    }
                    _ => "- ".to_string(),
                };
                self.pending_prefix = Some(format!("{}{}", "  ".repeat(depth), marker));
            }
            Tag::Emphasis => self.push_style(Style::new().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(Style::new().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => {
                self.push_style(Style::new().add_modifier(Modifier::CROSSED_OUT));
            }
            Tag::Link { dest_url, .. } => {
                self.link = Some(dest_url.to_string());
                self.push_style(self.theme.link);
            }
            Tag::Image { dest_url, .. } => {
                self.push_span(Span::styled("[image]", self.theme.muted));
                self.link = Some(dest_url.to_string());
            }
            Tag::FootnoteDefinition(_)
            | Tag::HtmlBlock
            | Tag::MetadataBlock(_)
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush_and_separate(),
            TagEnd::Heading(_) => {
                self.pop_style();
                self.flush_and_separate();
            }
            TagEnd::BlockQuote => {
                self.flush();
                self.prefix_stack.pop();
            }
            TagEnd::CodeBlock => {
                self.flush_and_separate();
                self.prefix_stack.pop();
                self.pop_style();
                self.code_block = false;
            }
            TagEnd::List(_) => {
                self.flush();
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.ensure_separation();
                }
            }
            TagEnd::Item => self.flush(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link | TagEnd::Image => {
                self.pop_style();
                if let Some(destination) = self.link.take() {
                    self.push_span(Span::styled(format!(" ({destination})"), self.theme.link));
                }
            }
            TagEnd::FootnoteDefinition
            | TagEnd::HtmlBlock
            | TagEnd::MetadataBlock(_)
            | TagEnd::Table
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell => {}
        }
    }

    fn table_event(
        &mut self,
        event: Event<'_>,
        source_range: std::ops::Range<usize>,
        source: &str,
    ) {
        if matches!(event, Event::End(TagEnd::Table)) {
            if let Some(table) = self.table.take() {
                let width = self.width.unwrap_or(100).max(8);
                self.lines.extend(table.render(width, self.theme));
                self.lines.push(Line::default());
            }
            return;
        }
        let style = self.styles.last().copied().unwrap_or(self.theme.text);
        if let Some(table) = self.table.as_mut() {
            table.event(event, source_range, source, style, self.theme);
        }
    }

    fn push_text(&mut self, text: &str) {
        for (index, segment) in text.split('\n').enumerate() {
            if index > 0 {
                self.flush();
            }
            if !segment.is_empty() {
                let style = self.styles.last().copied().unwrap_or(self.theme.text);
                self.push_span(Span::styled(segment.to_string(), style));
            }
        }
    }

    fn push_span(&mut self, span: Span<'static>) {
        if self.current.is_empty()
            && let Some(prefix) = self.pending_prefix.take()
        {
            self.current.push(Span::styled(prefix, self.theme.muted));
        }
        self.current.push(span);
    }

    fn flush(&mut self) {
        if self.current.is_empty() {
            return;
        }
        let prefix = self.prefix_stack.concat();
        let line = Line::from(std::mem::take(&mut self.current));
        if self.code_block {
            let mut spans = vec![Span::styled(prefix, self.theme.muted)];
            spans.extend(line.spans);
            self.lines.push(Line::from(spans));
        } else if let Some(width) = self.width {
            self.lines.extend(wrap_line(line, width, &prefix));
        } else {
            let mut spans = vec![Span::styled(prefix, self.theme.muted)];
            spans.extend(line.spans);
            self.lines.push(Line::from(spans));
        }
    }

    fn flush_and_separate(&mut self) {
        self.flush();
        self.ensure_separation();
    }

    fn ensure_separation(&mut self) {
        if !self.lines.is_empty() && self.lines.last().is_some_and(|line| !line_is_empty(line)) {
            self.lines.push(Line::default());
        }
    }

    fn push_style(&mut self, style: Style) {
        let current = self.styles.last().copied().unwrap_or_default();
        self.styles.push(current.patch(style));
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }
}

fn heading_style(level: HeadingLevel) -> Style {
    match level {
        HeadingLevel::H1 => Style::new()
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::UNDERLINED),
        HeadingLevel::H2 => Style::new().add_modifier(Modifier::BOLD),
        HeadingLevel::H3 => Style::new()
            .add_modifier(Modifier::BOLD)
            .add_modifier(Modifier::ITALIC),
        HeadingLevel::H4 | HeadingLevel::H5 | HeadingLevel::H6 => {
            Style::new().add_modifier(Modifier::ITALIC)
        }
    }
}

fn line_is_empty(line: &Line<'_>) -> bool {
    line.spans.iter().all(|span| span.content.is_empty())
}

#[cfg(test)]
mod tests;
