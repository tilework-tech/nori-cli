use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Margin;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
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
use crate::detail::provider_tone_style;
use crate::detail::tone_style;
use crate::focus;
use crate::focus::POINTER_WIDTH;

/// Stateless renderer for a caller-owned [`PickerState`].
pub struct Picker<'a, K> {
    state: &'a PickerState<K>,
    theme: Theme,
    density: PickerDensity,
    show_title: bool,
    show_details: bool,
    selection_rails: bool,
    compact_descriptions: bool,
    selection_keeps_tones: bool,
    horizontal_margin: u16,
    footer_hints: Option<Vec<KeyHint<'static>>>,
    empty_state: Option<(&'a str, &'a str)>,
}

impl<'a, K> Picker<'a, K> {
    pub fn new(state: &'a PickerState<K>) -> Self {
        Self {
            state,
            theme: Theme::default(),
            density: PickerDensity::default(),
            show_title: true,
            show_details: true,
            selection_rails: true,
            compact_descriptions: false,
            selection_keeps_tones: false,
            horizontal_margin: 2,
            footer_hints: None,
            empty_state: None,
        }
    }

    /// Supply caller-specific copy when the current view contains no rows.
    pub fn empty_state(mut self, title: &'a str, detail: &'a str) -> Self {
        self.empty_state = Some((title, detail));
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn density(mut self, density: PickerDensity) -> Self {
        self.density = density;
        self
    }

    /// Controls whether the picker reserves and renders its title row.
    ///
    /// Bottom-pane flows whose surrounding context already names the surface
    /// can hide the title to avoid redundant chrome.
    pub fn show_title(mut self, show_title: bool) -> Self {
        self.show_title = show_title;
        self
    }

    /// Controls whether wide layouts may open a detail pane beside the list.
    pub fn show_details(mut self, show_details: bool) -> Self {
        self.show_details = show_details;
        self
    }

    /// Draws green rails at both edges of the focused row. On by default;
    /// copyable content can turn them off.
    pub fn selection_rails(mut self, enabled: bool) -> Self {
        self.selection_rails = enabled;
        self
    }

    /// Show each row's description on a second line in compact density too,
    /// for lists whose rows carry a live preview.
    pub fn compact_descriptions(mut self, enabled: bool) -> Self {
        self.compact_descriptions = enabled;
        self
    }

    /// Keep agent and status colours on the selected row, over its
    /// highlight.
    pub fn selection_keeps_tones(mut self, enabled: bool) -> Self {
        self.selection_keeps_tones = enabled;
        self
    }

    /// Cells left empty at each side of the picker. Lists whose column
    /// widths follow the terminal width pass 0.
    pub fn horizontal_margin(mut self, cells: u16) -> Self {
        self.horizontal_margin = cells;
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
            horizontal: self.horizontal_margin,
            vertical: 1,
        });
        if inner.width < 4 || inner.height < 3 {
            return;
        }
        let title_height = u16::from(self.show_title && !self.state.title.is_empty());
        let page = Layout::vertical([
            Constraint::Length(title_height),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
        if title_height > 0 {
            Paragraph::new(Line::styled(self.state.title.clone(), self.theme.title))
                .render(page[0], buf);
        }

        let detail_visible = self.show_details
            && area.width >= 110
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
        let header_gap = u16::from(area.height > subtitle_height + category_height + search_height);
        let fixed_height = subtitle_height + category_height + search_height + header_gap;
        let content_height = area.height.saturating_sub(fixed_height);
        let chunks = Layout::vertical([
            Constraint::Length(subtitle_height),
            Constraint::Length(category_height),
            Constraint::Length(search_height),
            Constraint::Length(header_gap),
            Constraint::Length(content_height),
        ])
        .split(area);

        if let Some(subtitle) = &self.state.subtitle {
            Paragraph::new(Line::styled(subtitle.clone(), self.theme.muted)).render(chunks[0], buf);
        }
        self.render_categories(chunks[1], buf);
        self.render_search(chunks[2], buf);
        self.render_rows(chunks[4], buf);
    }

    fn render_categories(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let options = std::iter::once(("All", None, self.state.active_category.is_none())).chain(
            self.state.categories.iter().map(|category| {
                (
                    category.as_str(),
                    Some(category.as_str()),
                    self.state.active_category.as_ref() == Some(category),
                )
            }),
        );
        let spans = options
            .enumerate()
            .flat_map(|(index, (label, category, active))| {
                let gap = (index > 0).then(|| Span::raw("  "));
                let style = category
                    .and_then(|category| self.state.category_tones.get(category))
                    .map(|provider| provider_tone_style(*provider, self.theme))
                    .map(|style| {
                        if active {
                            style.add_modifier(Modifier::BOLD)
                        } else {
                            style
                        }
                    })
                    .unwrap_or_else(|| {
                        if active {
                            self.theme.text.add_modifier(Modifier::BOLD)
                        } else {
                            self.theme.muted
                        }
                    });
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
        buf.set_string(area.x, area.y, "⌕", self.theme.pointer);
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
            let (title, detail) = if let Some(message) = self.empty_state {
                message
            } else if self.state.query.is_empty() {
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

        let row_content_area = if self.selection_rails {
            area.inner(Margin {
                horizontal: 1,
                vertical: 0,
            })
        } else {
            area
        };
        if row_content_area.width == 0 {
            return;
        }
        let columns = visible_columns(&self.state.columns, row_content_area.width);
        let widths = column_widths(&columns, self.state, &visible, row_content_area.width);
        // A session table stripes its lines: each session's first line is
        // shaded and its second keeps the terminal's own background.
        let striped = self.density == PickerDensity::Compact
            && visible
                .iter()
                .any(|index| !self.state.items[*index].second_row.is_empty());
        let row_height = match self.density {
            PickerDensity::Compact
                if visible.iter().any(|index| {
                    let item = &self.state.items[*index];
                    !item.second_row.is_empty()
                        || (self.compact_descriptions && item.description.is_some())
                }) =>
            {
                2
            }
            PickerDensity::Compact => 1,
            PickerDensity::Normal => 2,
        };
        let guides = super::tree::tree_guides(
            &visible
                .iter()
                .map(|index| self.state.items[*index].depth)
                .collect::<Vec<_>>(),
        );
        let simple_list = columns.len() == 1 && self.state.mode == PickerMode::Single;
        let number_width = if simple_list
            && visible.iter().any(|index| {
                let item = &self.state.items[*index];
                !item.disabled && !item.section_heading
            }) {
            3
        } else {
            0
        };
        let dot_width = match self.state.mode {
            PickerMode::Single => 0,
            PickerMode::Toggle | PickerMode::Multi => 2,
        };
        let marker_width = POINTER_WIDTH + number_width + dot_width;
        let header_height = u16::from(!simple_list && area.height > row_height);
        if header_height > 0 {
            self.render_row(
                Rect::new(row_content_area.x, area.y, row_content_area.width, 1),
                buf,
                &columns,
                &widths,
                columns
                    .iter()
                    .map(|column| (column.header.as_str(), self.theme.table_header)),
                &[(" ", self.theme.table_header, marker_width)],
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
            let numbered_position = (simple_list && !item.disabled && !item.section_heading)
                .then(|| {
                    visible[..=start + row_offset]
                        .iter()
                        .filter(|index| {
                            let item = &self.state.items[**index];
                            !item.disabled && !item.section_heading
                        })
                        .count()
                })
                .filter(|position| (1..=9).contains(position));
            let surface = match self.density {
                PickerDensity::Compact if striped => self.theme.row_stripe,
                PickerDensity::Compact if (start + row_offset).is_multiple_of(2) => self.theme.row,
                PickerDensity::Compact => self.theme.row_alt,
                PickerDensity::Normal => self.theme.surface,
            };
            let style = if selected {
                surface.patch(self.theme.selected)
            } else if item.section_heading {
                surface.patch(self.theme.text).add_modifier(Modifier::BOLD)
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
            let (guide, description_guide) = &guides[start + row_offset];
            let tree_index = self
                .state
                .tree_column
                .as_ref()
                .and_then(|key| columns.iter().position(|column| &column.key == key))
                .unwrap_or(0);
            if let Some(primary) = values.get_mut(tree_index) {
                // Cut the title, not the badge, when the column is narrow.
                let badge = if badges.is_empty() {
                    String::new()
                } else {
                    format!(" ({badges})")
                };
                let room = usize::from(widths.get(tree_index).copied().unwrap_or(0))
                    .saturating_sub(guide.width() + badge.width());
                let cut_middle = columns
                    .get(tree_index)
                    .is_some_and(|column| column.cut_middle);
                let title = if cut_middle {
                    crate::text::truncate_middle(primary, room)
                } else if badge.is_empty() {
                    std::mem::take(primary)
                } else {
                    truncate(primary, room)
                };
                *primary = format!("{guide}{title}{badge}");
            }
            let row_area = Rect::new(
                area.x,
                area.y + header_height + row_offset as u16 * row_height,
                area.width,
                row_height,
            );
            let second_surface = if striped {
                Style::new().bg(Color::Reset)
            } else {
                surface
            };
            if striped && !selected {
                buf.set_style(
                    Rect {
                        height: 1,
                        ..row_area
                    },
                    style,
                );
                buf.set_style(
                    Rect {
                        y: row_area.y.saturating_add(1),
                        height: row_area.height.saturating_sub(1),
                        ..row_area
                    },
                    second_surface.patch(self.theme.text),
                );
            } else {
                buf.set_style(row_area, style);
            }
            let focus_style = focus::mark_style(style, self.theme);
            if selected && self.selection_rails {
                focus::render_rails(row_area, buf, focus_style);
            }
            let number =
                numbered_position.map_or_else(String::new, |position| format!("{position}."));
            let dot = match (self.state.mode, checked) {
                (PickerMode::Single, _) => "",
                (PickerMode::Toggle | PickerMode::Multi, true) => "●",
                (PickerMode::Toggle | PickerMode::Multi, false) => "○",
            };
            let dot_style = if checked && !item.disabled {
                style.patch(self.theme.pointer)
            } else {
                style
            };
            self.render_row(
                Rect::new(row_content_area.x, row_area.y, row_content_area.width, 1),
                buf,
                &columns,
                &widths,
                columns.iter().zip(&values).map(|(column, value)| {
                    let cell_style = if item.disabled || (selected && !self.selection_keeps_tones) {
                        style
                    } else if selected {
                        let style = item.cell_tones.get(&column.key).map_or(style, |provider| {
                            style.patch(provider_tone_style(*provider, self.theme))
                        });
                        item.cell_detail_tones
                            .get(&column.key)
                            .map_or(style, |tone| style.patch(tone_style(*tone, self.theme)))
                    } else {
                        let style = item.cell_tones.get(&column.key).map_or(style, |provider| {
                            surface.patch(provider_tone_style(*provider, self.theme))
                        });
                        item.cell_detail_tones
                            .get(&column.key)
                            .map_or(style, |tone| surface.patch(tone_style(*tone, self.theme)))
                    };
                    (value.as_str(), cell_style)
                }),
                &[
                    (
                        if selected { focus::POINTER } else { "" },
                        focus_style,
                        POINTER_WIDTH,
                    ),
                    (number.as_str(), style, number_width),
                    (dot, dot_style, dot_width),
                ],
            );
            let second_row_style = if selected {
                style.patch(self.theme.muted)
            } else {
                second_surface.patch(self.theme.muted)
            };
            if row_height > 1 && !item.second_row.is_empty() {
                let values = columns
                    .iter()
                    .enumerate()
                    .map(|(index, column)| {
                        let value = item
                            .second_row
                            .get(&column.key)
                            .cloned()
                            .unwrap_or_default();
                        if index != tree_index {
                            return value;
                        }
                        // Under a tree title, the value lines up with the
                        // title text after its guide and icon.
                        let room = usize::from(widths.get(index).copied().unwrap_or(0))
                            .saturating_sub(description_guide.width());
                        let value = if column.second_row_cut_middle {
                            crate::text::truncate_middle(&value, room)
                        } else {
                            truncate(&value, room)
                        };
                        format!("{description_guide}{value}")
                    })
                    .collect::<Vec<_>>();
                let mut x = row_content_area.x.saturating_add(marker_width);
                for (index, ((column, width), value)) in
                    columns.iter().zip(&widths).zip(&values).enumerate()
                {
                    let value = if index == tree_index {
                        truncate(value, usize::from(*width))
                    } else if column.second_row_cut_middle {
                        crate::text::truncate_middle(value, usize::from(*width))
                    } else {
                        truncate(value, usize::from(*width))
                    };
                    buf.set_string(x, row_area.y.saturating_add(1), value, second_row_style);
                    x = x.saturating_add(*width).saturating_add(2);
                }
            } else if row_height > 1
                && let Some(description) = &item.description
            {
                let description_area = Rect::new(
                    row_content_area.x.saturating_add(marker_width),
                    row_area.y.saturating_add(1),
                    row_content_area.width.saturating_sub(marker_width),
                    1,
                );
                let description = format!("{description_guide}{description}");
                let description_style = if selected {
                    style.patch(self.theme.muted)
                } else {
                    second_surface.patch(self.theme.muted)
                };
                Paragraph::new(truncate(&description, description_area.width as usize))
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
        values: impl IntoIterator<Item = (&'a str, Style)>,
        markers: &[(&str, Style, u16)],
    ) {
        let mut x = area.x;
        for (marker, marker_style, width) in markers {
            buf.set_string(x, area.y, marker, *marker_style);
            x = x.saturating_add(*width);
        }
        for ((column, width), (value, value_style)) in columns.iter().zip(widths).zip(values) {
            let value = if column.cut_middle {
                crate::text::truncate_middle(value, *width as usize)
            } else {
                truncate(value, *width as usize)
            };
            buf.set_string(x, area.y, value, value_style);
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
