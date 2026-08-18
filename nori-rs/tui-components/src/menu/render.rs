use std::borrow::Cow;
use std::marker::PhantomData;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::StatefulWidget;
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use super::MenuItem;
use super::MenuItemTone;
use super::MenuState;
use super::layout::truncate;
use super::layout::wrap_lines;
use crate::KeyHint;
use crate::KeyHints;
use crate::Theme;

/// Centered presentation for a bounded action menu.
///
/// The caller supplies the rectangle and owns terminal setup, input polling,
/// raw-mode and alternate-screen lifecycle, render cadence, focus or modal
/// orchestration, and application routing. Rendering may update only the
/// menu-local viewport fields in [`MenuState`]. It never invokes callbacks or
/// performs application actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OverlayMenu<'a, K> {
    title: Cow<'a, str>,
    subtitle: Option<Cow<'a, str>>,
    theme: Theme,
    max_width: u16,
    backdrop: bool,
    key_hints: Vec<KeyHint<'a>>,
    key: PhantomData<fn() -> K>,
}

impl<'a, K> OverlayMenu<'a, K> {
    /// Creates a menu presentation with a centered maximum width of 58 cells.
    pub fn new(title: impl Into<Cow<'a, str>>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            theme: Theme::default(),
            max_width: 58,
            backdrop: true,
            key_hints: Vec::new(),
            key: PhantomData,
        }
    }

    /// Adds supporting copy that is hidden before the primary labels on narrow
    /// or short caller rectangles.
    pub fn subtitle(mut self, subtitle: impl Into<Cow<'a, str>>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Applies semantic component styles.
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Sets the centered surface's maximum width in terminal cells.
    pub fn max_width(mut self, max_width: u16) -> Self {
        self.max_width = max_width.max(1);
        self
    }

    /// Enables or disables styling the caller-provided area as a backdrop.
    pub fn backdrop(mut self, backdrop: bool) -> Self {
        self.backdrop = backdrop;
        self
    }

    /// Adds centered hints to the bottom of the menu surface.
    pub fn key_hints(mut self, hints: impl IntoIterator<Item = KeyHint<'a>>) -> Self {
        self.key_hints = hints.into_iter().collect();
        self
    }
}

impl<K> StatefulWidget for OverlayMenu<'_, K> {
    type State = MenuState<K>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        if self.backdrop {
            buf.set_style(area, self.theme.backdrop);
        }

        let outer_margin = u16::from(area.width >= 34) * 2;
        let surface_width = area
            .width
            .saturating_sub(outer_margin.saturating_mul(2))
            .min(self.max_width);
        if surface_width == 0 {
            return;
        }
        let horizontal_padding = if surface_width >= 8 {
            2
        } else if surface_width >= 4 {
            1
        } else {
            0
        };
        let content_width = surface_width.saturating_sub(horizontal_padding * 2);
        let show_subtitle = content_width >= 40 && area.height >= 14;
        let subtitle_rows = self
            .subtitle
            .as_deref()
            .filter(|_| show_subtitle)
            .map(|subtitle| wrap_lines(subtitle, content_width, 2).len() as u16)
            .unwrap_or(0);
        let footer_rows = hint_rows(&self.key_hints, content_width);
        let vertical_padding = u16::from(area.height >= 16);
        let gap = u16::from(area.height >= 14);
        let has_numbers = state
            .items
            .iter()
            .any(|item| item.number_shortcut.is_some());
        let item_heights = item_heights(&state.items, content_width, has_numbers);
        let item_gaps = state.items.len().saturating_sub(1) as u16;
        let desired_height = vertical_padding * 2
            + 1
            + subtitle_rows
            + gap
            + item_heights.iter().sum::<u16>()
            + item_gaps
            + gap
            + footer_rows;
        let vertical_margin = u16::from(area.height >= 18);
        let surface_height = desired_height
            .min(area.height.saturating_sub(vertical_margin * 2))
            .max(1);
        let surface = Rect::new(
            area.x
                .saturating_add(area.width.saturating_sub(surface_width) / 2),
            area.y
                .saturating_add(area.height.saturating_sub(surface_height) / 2),
            surface_width.min(area.width),
            surface_height.min(area.height),
        );

        Clear.render(surface, buf);
        Block::default()
            .style(self.theme.menu_surface)
            .render(surface, buf);
        let content = Rect::new(
            surface.x.saturating_add(horizontal_padding),
            surface.y.saturating_add(vertical_padding),
            surface.width.saturating_sub(horizontal_padding * 2),
            surface.height.saturating_sub(vertical_padding * 2),
        );
        if content.width == 0 || content.height == 0 {
            return;
        }

        Paragraph::new(truncate(&self.title, content.width))
            .style(self.theme.title)
            .render(Rect::new(content.x, content.y, content.width, 1), buf);
        let mut header_height = 1;
        if subtitle_rows > 0
            && let Some(subtitle) = self.subtitle.as_deref()
        {
            let lines = wrap_lines(subtitle, content.width, 2)
                .into_iter()
                .map(|line| Line::styled(line, self.theme.muted))
                .collect::<Vec<_>>();
            Paragraph::new(lines).render(
                Rect::new(
                    content.x,
                    content.y.saturating_add(1),
                    content.width,
                    subtitle_rows,
                ),
                buf,
            );
            header_height += subtitle_rows;
        }

        let footer_height = footer_rows.min(content.height.saturating_sub(header_height));
        let footer_y = content.bottom().saturating_sub(footer_height);
        let list_y = content.y.saturating_add(header_height).saturating_add(gap);
        let list_bottom = footer_y.saturating_sub(gap).max(list_y);
        render_items(
            Rect::new(content.x, list_y, content.width, list_bottom - list_y),
            buf,
            self.theme,
            state,
            &item_heights,
            has_numbers,
        );
        if footer_height > 0 {
            KeyHints::new(self.key_hints).theme(self.theme).render(
                Rect::new(content.x, footer_y, content.width, footer_height),
                buf,
            );
        }
    }
}

fn render_items<K>(
    area: Rect,
    buf: &mut Buffer,
    theme: Theme,
    state: &mut MenuState<K>,
    item_heights: &[u16],
    has_numbers: bool,
) {
    if area.height == 0 || state.items.is_empty() {
        state.viewport_offset = 0;
        state.viewport_capacity = 1;
        return;
    }
    let selected = state.selected_index.unwrap_or(0).min(state.items.len() - 1);
    let mut start = state.viewport_offset.min(selected);
    let mut end = selected.saturating_add(1).min(state.items.len());
    while start < selected
        && window_height(start, end, item_heights, state.items.len()) > area.height
    {
        start += 1;
    }
    while end < state.items.len()
        && window_height(start, end + 1, item_heights, state.items.len()) <= area.height
    {
        end += 1;
    }
    while start > 0 && window_height(start - 1, end, item_heights, state.items.len()) <= area.height
    {
        start -= 1;
    }

    let show_top_marker = start > 0 && area.height > 2;
    let show_bottom_marker =
        end < state.items.len() && area.height.saturating_sub(u16::from(show_top_marker)) > 1;
    let mut y = area.y;
    if show_top_marker {
        Paragraph::new(format!("↑ {start} more"))
            .style(theme.muted)
            .render(Rect::new(area.x, y, area.width, 1), buf);
        y = y.saturating_add(1);
    }
    let items_bottom = area.bottom().saturating_sub(u16::from(show_bottom_marker));
    for (offset, item_index) in (start..end).enumerate() {
        if offset > 0 {
            y = y.saturating_add(1).min(items_bottom);
        }
        let height = item_heights[item_index].min(items_bottom.saturating_sub(y));
        render_item(
            Rect::new(area.x, y, area.width, height),
            buf,
            theme,
            &state.items[item_index],
            state.selected_index == Some(item_index),
            has_numbers,
        );
        y = y.saturating_add(height);
    }
    if show_bottom_marker {
        Paragraph::new(format!("↓ {} more", state.items.len() - end))
            .style(theme.muted)
            .render(
                Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1),
                buf,
            );
    }
    state.viewport_offset = start;
    state.viewport_capacity = end.saturating_sub(start).max(1);
}

fn window_height(start: usize, end: usize, item_heights: &[u16], item_count: usize) -> u16 {
    item_heights[start..end].iter().sum::<u16>()
        + end.saturating_sub(start + 1) as u16
        + u16::from(start > 0)
        + u16::from(end < item_count)
}

fn render_item<K>(
    area: Rect,
    buf: &mut Buffer,
    theme: Theme,
    item: &MenuItem<K>,
    selected: bool,
    has_numbers: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let selected_style = theme.selected.remove_modifier(Modifier::BOLD);
    if selected {
        buf.set_style(area, selected_style);
        for y in area.y..area.bottom() {
            Paragraph::new("▏")
                .style(theme.accent)
                .render(Rect::new(area.x, y, 1, 1), buf);
            if area.width > 1 {
                Paragraph::new("▕")
                    .style(theme.accent)
                    .render(Rect::new(area.right().saturating_sub(1), y, 1, 1), buf);
            }
        }
    }

    let inner_x = area.x.saturating_add(u16::from(area.width >= 3) * 2);
    let number_width = u16::from(has_numbers) * 3;
    let label_x = inner_x.saturating_add(number_width);
    let right_padding = if area.width >= 3 { 2 } else { 0 };
    let label_width = area
        .right()
        .saturating_sub(label_x)
        .saturating_sub(right_padding);
    if label_width == 0 {
        return;
    }
    let label_style = item_style(item, selected, theme);
    if has_numbers && inner_x < area.right() {
        let shortcut_style = if item.disabled {
            theme.disabled
        } else if selected {
            selected_style
        } else {
            theme.accent
        };
        let number = item
            .number_shortcut
            .map_or_else(String::new, |number| number.to_string());
        Paragraph::new(number)
            .style(shortcut_style)
            .render(Rect::new(inner_x, area.y, number_width, 1), buf);
    }

    let current_suffix = if item.current && label_width >= 18 {
        "  current"
    } else {
        ""
    };
    let suffix_width = UnicodeWidthStr::width(current_suffix) as u16;
    let label = truncate(&item.label, label_width.saturating_sub(suffix_width));
    let label_line = mnemonic_line(&label, item.mnemonic, label_style);
    Paragraph::new(label_line).render(Rect::new(label_x, area.y, label_width, 1), buf);
    if !current_suffix.is_empty() {
        let suffix_x = label_x.saturating_add(UnicodeWidthStr::width(label.as_str()) as u16);
        let suffix_style = if selected {
            selected_style
        } else {
            theme.muted
        };
        Paragraph::new(current_suffix).style(suffix_style).render(
            Rect::new(suffix_x, area.y, area.right().saturating_sub(suffix_x), 1),
            buf,
        );
    }

    if area.height > 1
        && let Some(description) = item.description.as_deref()
    {
        let description_style = if item.disabled {
            theme.disabled
        } else if selected {
            selected_style
        } else {
            theme.muted
        };
        let lines = wrap_lines(
            description,
            label_width,
            area.height.saturating_sub(1).min(2),
        )
        .into_iter()
        .map(|line| Line::styled(line, description_style))
        .collect::<Vec<_>>();
        Paragraph::new(lines).render(
            Rect::new(
                label_x,
                area.y.saturating_add(1),
                label_width,
                area.height.saturating_sub(1),
            ),
            buf,
        );
    }
}

fn item_style<K>(item: &MenuItem<K>, selected: bool, theme: Theme) -> Style {
    if item.disabled {
        theme.disabled
    } else if selected {
        theme.selected.remove_modifier(Modifier::BOLD)
    } else {
        match item.tone {
            MenuItemTone::Default => theme.text,
            MenuItemTone::Warning => theme.warning,
            MenuItemTone::Destructive => theme.error,
        }
    }
}

fn mnemonic_line<'a>(label: &'a str, mnemonic: Option<char>, style: Style) -> Line<'a> {
    let Some(mnemonic) = mnemonic else {
        return Line::styled(label, style);
    };
    let Some((byte_index, character)) = label
        .char_indices()
        .find(|(_, character)| character.eq_ignore_ascii_case(&mnemonic))
    else {
        return Line::styled(label, style);
    };
    let end = byte_index + character.len_utf8();
    Line::from(vec![
        Span::styled(&label[..byte_index], style),
        Span::styled(&label[byte_index..end], style.add_modifier(Modifier::BOLD)),
        Span::styled(&label[end..], style),
    ])
}

fn item_heights<K>(items: &[MenuItem<K>], content_width: u16, has_numbers: bool) -> Vec<u16> {
    let label_width = content_width
        .saturating_sub(4)
        .saturating_sub(u16::from(has_numbers) * 3);
    items
        .iter()
        .map(|item| {
            item.description.as_deref().map_or(2, |description| {
                1 + wrap_lines(description, label_width, 2).len().max(1) as u16
            })
        })
        .collect()
}

fn hint_rows(hints: &[KeyHint<'_>], width: u16) -> u16 {
    if hints.is_empty() || width == 0 {
        return 0;
    }
    let rendered_width = hints
        .iter()
        .enumerate()
        .map(|(index, hint)| {
            usize::from(index > 0) * 2
                + UnicodeWidthStr::width(hint.key.as_ref())
                + 1
                + UnicodeWidthStr::width(hint.action.as_ref())
        })
        .sum::<usize>();
    rendered_width.div_ceil(usize::from(width)).clamp(1, 2) as u16
}
