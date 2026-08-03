use pulldown_cmark::Alignment;
use pulldown_cmark::Event;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use std::ops::Range;
use unicode_width::UnicodeWidthStr;

use crate::Theme;

#[derive(Clone, Debug, Default)]
struct Cell {
    text: String,
    style: Style,
}

#[derive(Clone, Debug, Default)]
struct Row {
    cells: Vec<Cell>,
    has_boundary_pipe: bool,
}

pub(super) struct TableBuilder {
    alignments: Vec<Alignment>,
    header: Vec<Cell>,
    rows: Vec<Row>,
    current_row: Vec<Cell>,
    current_cell: Option<Cell>,
    in_header: bool,
    current_row_has_boundary_pipe: bool,
}

impl TableBuilder {
    pub(super) fn new(alignments: Vec<Alignment>) -> Self {
        Self {
            alignments,
            header: Vec::new(),
            rows: Vec::new(),
            current_row: Vec::new(),
            current_cell: None,
            in_header: false,
            current_row_has_boundary_pipe: false,
        }
    }

    pub(super) fn event(
        &mut self,
        event: Event<'_>,
        source_range: Range<usize>,
        source: &str,
        style: Style,
        theme: Theme,
    ) {
        match event {
            Event::Start(Tag::TableHead) => {
                self.in_header = true;
                self.current_row.clear();
            }
            Event::Start(Tag::TableRow) => {
                self.current_row.clear();
                self.current_row_has_boundary_pipe = source
                    .get(source_range)
                    .map(str::trim)
                    .is_some_and(|row| row.starts_with('|') || row.ends_with('|'));
            }
            Event::Start(Tag::TableCell) => {
                self.current_cell = Some(Cell {
                    text: String::new(),
                    style,
                })
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(cell) = self.current_cell.take() {
                    self.current_row.push(cell);
                }
            }
            Event::End(TagEnd::TableRow) => {
                let row = std::mem::take(&mut self.current_row);
                if self.in_header {
                    self.header = row;
                } else {
                    self.rows.push(Row {
                        cells: row,
                        has_boundary_pipe: self.current_row_has_boundary_pipe,
                    });
                }
                self.current_row_has_boundary_pipe = false;
            }
            Event::End(TagEnd::TableHead) => {
                if self.header.is_empty() && !self.current_row.is_empty() {
                    self.header = std::mem::take(&mut self.current_row);
                }
                self.in_header = false;
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(cell) = self.current_cell.as_mut() {
                    cell.text.push_str(&text);
                    cell.style = style;
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(cell) = self.current_cell.as_mut() {
                    cell.text.push(' ');
                }
            }
            Event::TaskListMarker(checked) => {
                if let Some(cell) = self.current_cell.as_mut() {
                    cell.text.push_str(if checked { "[x] " } else { "[ ] " });
                }
            }
            Event::Start(Tag::Strong) => {
                if let Some(cell) = self.current_cell.as_mut() {
                    cell.style = cell.style.patch(theme.table_header);
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                if let Some(cell) = self.current_cell.as_mut() {
                    cell.style = cell.style.patch(theme.link);
                    if !cell.text.is_empty() {
                        cell.text.push(' ');
                    }
                    cell.text.push_str(&dest_url);
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                if let Some(cell) = self.current_cell.as_mut() {
                    cell.text.push_str(&html);
                }
            }
            Event::Start(_) | Event::End(_) | Event::FootnoteReference(_) | Event::Rule => {}
        }
    }

    pub(super) fn render(mut self, width: u16, theme: Theme) -> Vec<Line<'static>> {
        let mut spillover = Vec::new();
        self.rows.retain(|row| {
            let is_spillover = !row.has_boundary_pipe
                && row
                    .cells
                    .iter()
                    .enumerate()
                    .all(|(index, cell)| index == 0 || cell.text.trim().is_empty());
            if is_spillover {
                if let Some(cell) = row.cells.first() {
                    spillover.push(cell.clone());
                }
                false
            } else {
                true
            }
        });
        let column_count = self.alignments.len().max(self.header.len()).max(
            self.rows
                .iter()
                .map(|row| row.cells.len())
                .max()
                .unwrap_or(0),
        );
        if column_count == 0 {
            return Vec::new();
        }
        self.header.resize(column_count, Cell::default());
        for row in &mut self.rows {
            row.cells.resize(column_count, Cell::default());
        }
        self.alignments.resize(column_count, Alignment::None);

        let natural = (0..column_count)
            .map(|column| {
                std::iter::once(&self.header[column])
                    .chain(self.rows.iter().map(|row| &row.cells[column]))
                    .map(|cell| cell.text.width())
                    .max()
                    .unwrap_or(0)
                    .clamp(3, 42)
            })
            .collect::<Vec<_>>();
        let minimum = (0..column_count)
            .map(|column| self.header[column].text.width().clamp(3, 12))
            .collect::<Vec<_>>();
        let gaps = column_count.saturating_sub(1) * 2;
        let available = width as usize;
        if minimum.iter().sum::<usize>() + gaps > available {
            let mut lines = self.render_stacked(width, theme);
            append_spillover(&mut lines, spillover, width, theme);
            return lines;
        }

        let mut widths = natural;
        while widths.iter().sum::<usize>() + gaps > available {
            let Some((index, _)) = widths
                .iter()
                .enumerate()
                .filter(|(index, value)| **value > minimum[*index])
                .max_by_key(|(_, value)| **value)
            else {
                let mut lines = self.render_stacked(width, theme);
                append_spillover(&mut lines, spillover, width, theme);
                return lines;
            };
            widths[index] -= 1;
        }
        let body_height = self
            .rows
            .iter()
            .map(|row| {
                row.cells
                    .iter()
                    .zip(&widths)
                    .map(|(cell, width)| wrapped_plain(&cell.text, *width).len())
                    .max()
                    .unwrap_or(1)
            })
            .sum::<usize>();
        if !self.rows.is_empty() && body_height > self.rows.len() * 2 {
            let mut lines = self.render_stacked(width, theme);
            append_spillover(&mut lines, spillover, width, theme);
            return lines;
        }
        let mut lines = self.render_grid(&widths, theme);
        append_spillover(&mut lines, spillover, width, theme);
        lines
    }

    fn render_grid(&self, widths: &[usize], theme: Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        lines.push(render_row(
            &self.header,
            widths,
            &self.alignments,
            theme.table_header,
        ));
        lines.push(Line::styled(
            widths
                .iter()
                .map(|width| "━".repeat(*width))
                .collect::<Vec<_>>()
                .join("  "),
            theme.table_rule,
        ));
        for (row_index, row) in self.rows.iter().enumerate() {
            let wrapped = row
                .cells
                .iter()
                .zip(widths)
                .map(|(cell, width)| wrapped_plain(&cell.text, *width))
                .collect::<Vec<_>>();
            let height = wrapped.iter().map(Vec::len).max().unwrap_or(1);
            for line_index in 0..height {
                let cells = row
                    .cells
                    .iter()
                    .enumerate()
                    .map(|(column, cell)| Cell {
                        text: wrapped[column].get(line_index).cloned().unwrap_or_default(),
                        style: cell.style,
                    })
                    .collect::<Vec<_>>();
                lines.push(render_row(&cells, widths, &self.alignments, theme.text));
            }
            if row_index + 1 < self.rows.len() {
                lines.push(Line::styled(
                    widths
                        .iter()
                        .map(|width| "─".repeat(*width))
                        .collect::<Vec<_>>()
                        .join("  "),
                    theme.table_rule,
                ));
            }
        }
        lines
    }

    fn render_stacked(&self, width: u16, theme: Theme) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        let label_width = self
            .header
            .iter()
            .map(|cell| cell.text.width())
            .max()
            .unwrap_or(0)
            .min((width as usize / 3).max(1));
        for (row_index, row) in self.rows.iter().enumerate() {
            for (column, cell) in row.cells.iter().enumerate() {
                let label = self.header[column].text.as_str();
                let prefix = format!("{label:label_width$}  ");
                let value_width = (width as usize).saturating_sub(prefix.width()).max(1);
                for (line_index, value) in wrapped_plain(&cell.text, value_width)
                    .into_iter()
                    .enumerate()
                {
                    let prefix = if line_index == 0 {
                        prefix.clone()
                    } else {
                        " ".repeat(prefix.width())
                    };
                    lines.push(Line::from(vec![
                        Span::styled(prefix, theme.table_header),
                        Span::styled(value, cell.style.patch(theme.text)),
                    ]));
                }
            }
            if row_index + 1 < self.rows.len() {
                lines.push(Line::styled("─".repeat(width as usize), theme.table_rule));
            }
        }
        lines
    }
}

fn render_row(
    row: &[Cell],
    widths: &[usize],
    alignments: &[Alignment],
    base_style: Style,
) -> Line<'static> {
    let spans = row
        .iter()
        .zip(widths)
        .zip(alignments)
        .enumerate()
        .flat_map(|(index, ((cell, width), alignment))| {
            let separator = (index > 0).then(|| Span::raw("  "));
            let value = aligned(&cell.text, *width, *alignment);
            separator
                .into_iter()
                .chain([Span::styled(value, base_style.patch(cell.style))])
        })
        .collect::<Vec<_>>();
    Line::from(spans)
}

fn aligned(value: &str, width: usize, alignment: Alignment) -> String {
    let padding = width.saturating_sub(value.width());
    match alignment {
        Alignment::Right => format!("{}{}", " ".repeat(padding), value),
        Alignment::Center => {
            let left = padding / 2;
            format!(
                "{}{}{}",
                " ".repeat(left),
                value,
                " ".repeat(padding - left)
            )
        }
        Alignment::Left | Alignment::None => format!("{}{}", value, " ".repeat(padding)),
    }
}

fn wrapped_plain(value: &str, width: usize) -> Vec<String> {
    if value.is_empty() {
        return vec![String::new()];
    }
    textwrap::wrap(value, width.max(1))
        .into_iter()
        .map(std::borrow::Cow::into_owned)
        .collect()
}

fn append_spillover(
    lines: &mut Vec<Line<'static>>,
    spillover: Vec<Cell>,
    width: u16,
    theme: Theme,
) {
    if spillover.is_empty() {
        return;
    }
    lines.push(Line::default());
    for cell in spillover {
        for line in wrapped_plain(&cell.text, width as usize) {
            lines.push(Line::styled(line, cell.style.patch(theme.text)));
        }
    }
}
