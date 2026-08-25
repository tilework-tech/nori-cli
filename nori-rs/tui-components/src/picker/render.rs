use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use super::PickerColumn;
use super::PickerColumnWidth;
use super::PickerDensity;
use super::PickerLoadState;
use super::PickerMode;
use super::PickerState;
use super::SearchMode;
use crate::DetailEntry;
use crate::DetailPane;
use crate::EmptyState;
use crate::KeyHint;
use crate::KeyHints;
use crate::MessageLevel;
use crate::SemanticMessage;
use crate::Theme;

/// Stateless renderer for a caller-owned [`PickerState`].
pub struct Picker<'a, K> {
    state: &'a PickerState<K>,
    theme: Theme,
    density: PickerDensity,
    footer_hints: Option<Vec<KeyHint<'static>>>,
}

impl<'a, K> Picker<'a, K> {
    pub fn new(state: &'a PickerState<K>) -> Self {
        Self {
            state,
            theme: Theme::default(),
            density: PickerDensity::default(),
            footer_hints: None,
        }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn density(mut self, density: PickerDensity) -> Self {
        self.density = density;
        self
    }

    pub fn footer_hints(mut self, hints: impl IntoIterator<Item = KeyHint<'static>>) -> Self {
        self.footer_hints = Some(hints.into_iter().collect());
        self
    }
}

impl<K: Clone + Eq> Widget for Picker<'_, K> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width < 8 || area.height < 4 {
            return;
        }
        Block::default().style(self.theme.surface).render(area, buf);
        let inner = area.inner(Margin {
            horizontal: 2,
            vertical: 1,
        });
        if inner.width < 4 || inner.height < 3 {
            return;
        }
        let page = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
        Paragraph::new(Line::styled(self.state.title.clone(), self.theme.title))
            .render(page[0], buf);

        let detail_visible = area.width >= 110
            && self
                .state
                .selected_item()
                .is_some_and(|item| !item.details.is_empty() || !item.detail.is_empty());
        let panes = if detail_visible {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Ratio(2, 3),
                    Constraint::Length(2),
                    Constraint::Ratio(1, 3),
                ])
                .split(page[1])
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(100)])
                .split(page[1])
        };
        self.render_list(panes[0], buf);
        if let Some(detail_area) = panes.get(2).copied() {
            self.render_detail(detail_area, buf);
        }
        self.render_footer(page[2], buf);
    }
}

impl<K: Clone + Eq> Picker<'_, K> {
    fn render_list(&self, area: Rect, buf: &mut Buffer) {
        let subtitle_height = u16::from(self.state.subtitle.is_some());
        let category_height = u16::from(!self.state.categories.is_empty());
        let search_height = u16::from(
            self.state.search_active && !matches!(self.state.search_mode, SearchMode::None),
        );
        let fixed_height = subtitle_height + category_height + search_height;
        let content_height = area.height.saturating_sub(fixed_height);
        let chunks = Layout::vertical([
            Constraint::Length(subtitle_height),
            Constraint::Length(category_height),
            Constraint::Length(search_height),
            Constraint::Length(content_height),
        ])
        .split(area);

        if let Some(subtitle) = &self.state.subtitle {
            Paragraph::new(Line::styled(subtitle.clone(), self.theme.muted)).render(chunks[0], buf);
        }
        self.render_categories(chunks[1], buf);
        self.render_search(chunks[2], buf);
        self.render_rows(chunks[3], buf);
    }

    fn render_categories(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let options = std::iter::once(("All", self.state.active_category.is_none())).chain(
            self.state.categories.iter().map(|category| {
                (
                    category.as_str(),
                    self.state.active_category.as_ref() == Some(category),
                )
            }),
        );
        let spans = options
            .enumerate()
            .flat_map(|(index, (label, active))| {
                let gap = (index > 0).then(|| Span::raw("  "));
                let style = if active {
                    self.theme.accent
                } else {
                    self.theme.muted
                };
                gap.into_iter()
                    .chain([Span::styled(label.to_string(), style)])
            })
            .collect::<Vec<_>>();
        Paragraph::new(Line::from(spans)).render(area, buf);
    }

    fn render_search(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let query = if self.state.query.is_empty() {
            Span::styled(self.state.search_placeholder.clone(), self.theme.muted)
        } else {
            Span::styled(self.state.query.clone(), self.theme.text)
        };
        buf.set_string(area.x, area.y, "⌕", self.theme.accent);
        let input = Rect::new(
            area.x.saturating_add(2),
            area.y,
            area.width.saturating_sub(2),
            1,
        );
        buf.set_style(input, self.theme.input);
        let inner = input.inner(Margin {
            horizontal: 1,
            vertical: 0,
        });
        Paragraph::new(Line::from(query)).render(inner, buf);
    }

    fn render_rows(&self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        match &self.state.load_state {
            PickerLoadState::Loading(message) => {
                EmptyState::new(message.clone())
                    .marker("◌")
                    .detail("Results will appear without rebuilding the picker.")
                    .theme(self.theme)
                    .render(area, buf);
                return;
            }
            PickerLoadState::Failed(message) => {
                SemanticMessage::new(MessageLevel::Error, "Could not load options")
                    .detail(message.clone())
                    .theme(self.theme)
                    .render(area, buf);
                return;
            }
            PickerLoadState::Ready => {}
        }

        let visible = self.state.visible_indices();
        if visible.is_empty() {
            let (title, detail) = if self.state.query.is_empty() {
                (
                    "No options available",
                    "The caller has not supplied any rows.",
                )
            } else {
                (
                    "No matching options",
                    "Try a different search term or category.",
                )
            };
            EmptyState::new(title)
                .detail(detail)
                .theme(self.theme)
                .render(area, buf);
            return;
        }

        let columns = visible_columns(&self.state.columns, area.width);
        let widths = column_widths(&columns, self.state, &visible, area.width);
        let row_height = match self.density {
            PickerDensity::Compact => 1,
            PickerDensity::Normal => 2,
        };
        let header_height = u16::from(area.height > row_height);
        if header_height > 0 {
            self.render_row(
                Rect::new(area.x, area.y, area.width, 1),
                buf,
                &columns,
                &widths,
                columns.iter().map(|column| column.header.as_str()),
                self.theme.table_header,
                " ",
            );
        }

        let rows_height = area.height.saturating_sub(header_height);
        if rows_height < row_height {
            return;
        }
        let visible_row_count = (rows_height / row_height).max(1) as usize;
        let selected_position = self
            .state
            .selected_index
            .and_then(|selected| visible.iter().position(|index| *index == selected))
            .unwrap_or(0);
        let start = selected_position.saturating_sub(visible_row_count.saturating_sub(1));
        for (row_offset, item_index) in visible
            .iter()
            .skip(start)
            .take(visible_row_count)
            .enumerate()
        {
            let item = &self.state.items[*item_index];
            let selected = self.state.selected_index == Some(*item_index);
            let checked = self.state.selected_keys.contains(&item.key);
            let marker = match self.state.mode {
                PickerMode::Single => {
                    if selected {
                        "›"
                    } else {
                        " "
                    }
                }
                PickerMode::Toggle | PickerMode::Multi => {
                    if checked {
                        "●"
                    } else {
                        "○"
                    }
                }
            };
            let surface = match self.density {
                PickerDensity::Compact if (start + row_offset).is_multiple_of(2) => self.theme.row,
                PickerDensity::Compact => self.theme.row_alt,
                PickerDensity::Normal => self.theme.surface,
            };
            let style = if selected {
                self.theme.selected
            } else if item.disabled {
                surface.patch(self.theme.disabled)
            } else {
                surface.patch(self.theme.text)
            };
            let badges = [
                item.current.then_some("current"),
                item.default.then_some("default"),
                item.read_only.then_some("read only"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(", ");
            let mut values = columns
                .iter()
                .map(|column| item.cells.get(&column.key).cloned().unwrap_or_default())
                .collect::<Vec<_>>();
            if !badges.is_empty()
                && let Some(primary) = values.first_mut()
            {
                primary.push_str(" (");
                primary.push_str(&badges);
                primary.push(')');
            }
            let row_area = Rect::new(
                area.x,
                area.y + header_height + row_offset as u16 * row_height,
                area.width,
                row_height,
            );
            buf.set_style(row_area, style);
            self.render_row(
                Rect::new(row_area.x, row_area.y, row_area.width, 1),
                buf,
                &columns,
                &widths,
                values.iter().map(String::as_str),
                style,
                marker,
            );
            if row_height > 1
                && let Some(description) = &item.description
            {
                let description_area = Rect::new(
                    row_area.x.saturating_add(2),
                    row_area.y.saturating_add(1),
                    row_area.width.saturating_sub(2),
                    1,
                );
                let description_style = if selected {
                    self.theme.selected
                } else {
                    surface.patch(self.theme.muted)
                };
                Paragraph::new(truncate(description, description_area.width as usize))
                    .style(description_style)
                    .render(description_area, buf);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row<'a>(
        &self,
        area: Rect,
        buf: &mut Buffer,
        columns: &[&PickerColumn],
        widths: &[u16],
        values: impl IntoIterator<Item = &'a str>,
        style: ratatui::style::Style,
        marker: &str,
    ) {
        let mut x = area.x;
        buf.set_string(x, area.y, marker, style);
        x = x.saturating_add(2);
        for ((column, width), value) in columns.iter().zip(widths).zip(values) {
            let value = truncate(value, *width as usize);
            buf.set_string(x, area.y, value, style);
            x = x.saturating_add(*width);
            if column.key
                != columns
                    .last()
                    .map(|column| column.key.as_str())
                    .unwrap_or_default()
            {
                x = x.saturating_add(2);
            }
        }
    }

    fn render_detail(&self, area: Rect, buf: &mut Buffer) {
        let Some(item) = self.state.selected_item() else {
            return;
        };
        let inner = area.inner(Margin {
            horizontal: 1,
            vertical: 1,
        });
        if inner.height == 0 {
            return;
        }
        let content = Rect::new(
            inner.x,
            inner.y.saturating_add(2),
            inner.width,
            inner.height.saturating_sub(2),
        );
        if item.details.is_empty() {
            Paragraph::new(item.detail.clone())
                .style(self.theme.text)
                .wrap(ratatui::widgets::Wrap { trim: false })
                .render(content, buf);
            return;
        }
        let entries = item
            .details
            .iter()
            .map(|detail| DetailEntry::key_value(detail.label.clone(), detail.value.clone()))
            .collect::<Vec<_>>();
        DetailPane::new(&entries)
            .heading("Details")
            .theme(self.theme)
            .render(Rect::new(inner.x, inner.y, inner.width, inner.height), buf);
    }

    fn render_footer(&self, area: Rect, buf: &mut Buffer) {
        let select_action = if self.state.mode == PickerMode::Single {
            "select"
        } else {
            "toggle"
        };
        let hints = self.footer_hints.clone().unwrap_or_else(|| {
            if self.state.search_active {
                vec![
                    KeyHint::new("↑↓", "move"),
                    KeyHint::new("type", "filter"),
                    KeyHint::new("enter", select_action),
                    KeyHint::new("esc", "stop search"),
                ]
            } else if !matches!(self.state.search_mode, SearchMode::None) {
                vec![
                    KeyHint::new("↑↓/j/k", "move"),
                    KeyHint::new("/", "search"),
                    KeyHint::new("enter", select_action),
                    KeyHint::new("esc", "close"),
                ]
            } else {
                vec![
                    KeyHint::new("↑↓/j/k", "move"),
                    KeyHint::new("enter", select_action),
                    KeyHint::new("esc", "close"),
                ]
            }
        });
        KeyHints::new(hints).theme(self.theme).render(area, buf);
    }
}

fn visible_columns(columns: &[PickerColumn], viewport_width: u16) -> Vec<&PickerColumn> {
    columns
        .iter()
        .filter(|column| {
            column
                .hide_below
                .is_none_or(|minimum| viewport_width >= minimum)
        })
        .collect()
}

fn column_widths<K>(
    columns: &[&PickerColumn],
    state: &PickerState<K>,
    visible: &[usize],
    viewport_width: u16,
) -> Vec<u16> {
    let gaps = columns.len().saturating_sub(1) as u16 * 2;
    let available = viewport_width.saturating_sub(2 + gaps);
    let mut widths = columns
        .iter()
        .map(|column| match column.width {
            PickerColumnWidth::Fixed(width) => width,
            PickerColumnWidth::Flexible { min, max, .. } => {
                let natural = visible
                    .iter()
                    .filter_map(|index| state.items[*index].cells.get(&column.key))
                    .map(|value| value.width() as u16)
                    .chain([column.header.width() as u16])
                    .max()
                    .unwrap_or(min);
                natural.clamp(min, max)
            }
        })
        .collect::<Vec<_>>();
    while widths.iter().sum::<u16>() > available {
        let Some((index, _)) = widths
            .iter()
            .enumerate()
            .filter(|(index, width)| match columns[*index].width {
                PickerColumnWidth::Fixed(_) => false,
                PickerColumnWidth::Flexible { min, .. } => **width > min,
            })
            .max_by_key(|(_, width)| **width)
        else {
            break;
        };
        widths[index] = widths[index].saturating_sub(1);
    }
    let mut spare = available.saturating_sub(widths.iter().sum());
    while spare > 0 {
        let mut grew = false;
        for (index, column) in columns.iter().enumerate() {
            if let PickerColumnWidth::Flexible { max, weight, .. } = column.width {
                for _ in 0..weight {
                    if widths[index] >= max || spare == 0 {
                        break;
                    }
                    widths[index] += 1;
                    spare -= 1;
                    grew = true;
                }
            }
            if spare == 0 {
                break;
            }
        }
        if !grew {
            break;
        }
    }
    widths
}

fn truncate(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_string();
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut result = String::new();
    let mut used = 0;
    for character in value.chars() {
        let character_width = character.width().unwrap_or(0);
        if used + character_width > width - 1 {
            break;
        }
        result.push(character);
        used += character_width;
    }
    result.push('…');
    result
}
