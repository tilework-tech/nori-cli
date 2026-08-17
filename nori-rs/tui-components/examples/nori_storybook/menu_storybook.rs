//! Temporary, storybook-only overlay menu used to settle the visual contract.
//!
//! The production component will replace this module after visual approval.

mod fixtures;
#[path = "menu_storybook/state.rs"]
mod state;

pub(super) use state::MenuAction;
pub(super) use state::MenuStoryState;

use codex_tui_components::KeyHints;
use codex_tui_components::Theme;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use textwrap::Options;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy)]
pub(super) enum MenuStory {
    Action,
    Shortcuts,
    Narrow,
    Destructive,
}

enum MenuTone {
    Default,
    Warning,
    Destructive,
}

struct PrototypeItem {
    label: &'static str,
    description: &'static str,
    mnemonic: Option<char>,
    number: Option<u8>,
    disabled: bool,
    tone: MenuTone,
}

impl PrototypeItem {
    fn new(label: &'static str, description: &'static str) -> Self {
        Self {
            label,
            description,
            mnemonic: None,
            number: None,
            disabled: false,
            tone: MenuTone::Default,
        }
    }

    fn mnemonic(mut self, mnemonic: char) -> Self {
        self.mnemonic = Some(mnemonic);
        self
    }

    fn number(mut self, number: u8) -> Self {
        self.number = Some(number);
        self
    }

    fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    fn tone(mut self, tone: MenuTone) -> Self {
        self.tone = tone;
        self
    }
}

struct PrototypeMenu {
    title: &'static str,
    subtitle: Option<&'static str>,
    items: Vec<PrototypeItem>,
    selected: usize,
}

pub(super) fn render(area: Rect, buf: &mut Buffer, theme: Theme, state: &MenuStoryState) {
    render_host(area, buf, theme, state);
    let caller_area = match state.story() {
        MenuStory::Narrow => centered(area, area.width.min(30), area.height.min(12)),
        MenuStory::Action | MenuStory::Shortcuts | MenuStory::Destructive => area,
    };
    let mut menu = fixtures::menu(state.story());
    menu.selected = state.selected_index();
    let subtitle = state.notice().or(menu.subtitle);
    render_overlay(caller_area, buf, theme, state.story(), subtitle, menu);
}

fn render_host(area: Rect, buf: &mut Buffer, theme: Theme, state: &MenuStoryState) {
    Block::default().style(theme.surface).render(area, buf);
    if area.width < 60 {
        return;
    }
    let story_name = match state.story() {
        MenuStory::Action => "centered action",
        MenuStory::Shortcuts => "shortcut-heavy",
        MenuStory::Narrow => "30x12 caller area",
        MenuStory::Destructive => "destructive action",
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "Overlay menu prototype",
                theme.text.add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {story_name}"), theme.muted),
        ]),
        Line::styled("tab next example   shift-tab previous example", theme.muted),
        Line::styled(state.notice().unwrap_or_default(), theme.accent),
        Line::styled("Session transcript", theme.text),
        Line::styled("[host] transcript remains beneath the overlay", theme.muted),
        Line::styled("[host] status stays visible outside the menu", theme.muted),
    ];
    Paragraph::new(lines).render(area, buf);
}

fn render_overlay(
    area: Rect,
    buf: &mut Buffer,
    theme: Theme,
    story: MenuStory,
    subtitle: Option<&str>,
    menu: PrototypeMenu,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    buf.set_style(area, theme.row);
    if area.width < 8 || area.height < 4 {
        return;
    }

    let outer_margin = if area.width < 40 { 1 } else { 2 };
    let surface_width = area.width.saturating_sub(outer_margin * 2).min(58);
    let content_width = surface_width.saturating_sub(4);
    let show_subtitle = content_width >= 40 && area.height >= 14;
    let subtitle_rows = subtitle
        .filter(|_| show_subtitle)
        .map(|subtitle| wrap_lines(subtitle, content_width, 2).len() as u16)
        .unwrap_or(0);
    let vertical_padding = u16::from(area.height >= 16);
    let gap = u16::from(area.height >= 14);
    let footer_rows = if content_width < 40 || matches!(story, MenuStory::Shortcuts) {
        2
    } else {
        1
    };
    let item_heights = item_heights(&menu, content_width);
    let item_gaps = menu.items.len().saturating_sub(1) as u16;
    let desired_height = vertical_padding * 2
        + 1
        + subtitle_rows
        + gap
        + item_heights.iter().sum::<u16>()
        + item_gaps
        + gap
        + footer_rows;
    let vertical_margin = u16::from(area.height >= 18);
    let surface_height = desired_height.min(area.height.saturating_sub(vertical_margin * 2));
    let surface = centered(area, surface_width, surface_height);

    Clear.render(surface, buf);
    Block::default()
        .style(theme.detail_surface)
        .render(surface, buf);
    let content = Rect::new(
        surface.x.saturating_add(2),
        surface.y.saturating_add(vertical_padding),
        surface.width.saturating_sub(4),
        surface.height.saturating_sub(vertical_padding * 2),
    );
    if content.width == 0 || content.height == 0 {
        return;
    }

    let mut header_height = 1;
    Paragraph::new(truncate(menu.title, content.width))
        .style(theme.text.add_modifier(Modifier::BOLD))
        .render(Rect::new(content.x, content.y, content.width, 1), buf);
    if subtitle_rows > 0
        && let Some(subtitle) = subtitle
    {
        let lines = wrap_lines(subtitle, content.width, 2)
            .into_iter()
            .map(|line| Line::styled(line, theme.muted))
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

    let footer_y = content.bottom().saturating_sub(footer_rows);
    let list_y = content.y.saturating_add(header_height).saturating_add(gap);
    let list_bottom = footer_y.saturating_sub(gap).max(list_y);
    render_items(
        Rect::new(content.x, list_y, content.width, list_bottom - list_y),
        buf,
        theme,
        &menu,
        &item_heights,
    );
    KeyHints::new(fixtures::footer_hints(story))
        .theme(theme)
        .render(
            Rect::new(content.x, footer_y, content.width, footer_rows),
            buf,
        );
}

fn render_items(
    area: Rect,
    buf: &mut Buffer,
    theme: Theme,
    menu: &PrototypeMenu,
    item_heights: &[u16],
) {
    if area.height == 0 || menu.items.is_empty() {
        return;
    }
    let mut start = 0;
    let mut end = menu.selected.saturating_add(1).min(menu.items.len());
    while start < menu.selected
        && window_height(start, end, item_heights, menu.items.len()) > area.height
    {
        start += 1;
    }
    while end < menu.items.len()
        && window_height(start, end + 1, item_heights, menu.items.len()) <= area.height
    {
        end += 1;
    }
    while start > 0 && window_height(start - 1, end, item_heights, menu.items.len()) <= area.height
    {
        start -= 1;
    }

    let show_top_marker = start > 0 && area.height > 2;
    let show_bottom_marker =
        end < menu.items.len() && area.height.saturating_sub(u16::from(show_top_marker)) > 1;
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
        let item = &menu.items[item_index];
        let height = item_heights[item_index].min(items_bottom.saturating_sub(y));
        render_item(
            Rect::new(area.x, y, area.width, height),
            buf,
            theme,
            item,
            item_index == menu.selected,
            menu.items.iter().any(|item| item.number.is_some()),
            menu.items
                .iter()
                .any(|item| !matches!(item.tone, MenuTone::Default)),
        );
        y = y.saturating_add(height);
    }
    if show_bottom_marker {
        Paragraph::new(format!("↓ {} more", menu.items.len() - end))
            .style(theme.muted)
            .render(Rect::new(area.x, items_bottom, area.width, 1), buf);
    }
}

fn window_height(start: usize, end: usize, item_heights: &[u16], item_count: usize) -> u16 {
    item_heights[start..end].iter().sum::<u16>()
        + end.saturating_sub(start + 1) as u16
        + u16::from(start > 0)
        + u16::from(end < item_count)
}

fn render_item(
    area: Rect,
    buf: &mut Buffer,
    theme: Theme,
    item: &PrototypeItem,
    selected: bool,
    show_numbers: bool,
    show_tones: bool,
) {
    if area.width < 4 || area.height == 0 {
        return;
    }
    if selected {
        let selection_surface = theme
            .selected
            .bg
            .map_or_else(Style::new, |background| Style::new().bg(background));
        buf.set_style(area, selection_surface);
        for y in area.y..area.bottom() {
            Paragraph::new("▏")
                .style(theme.accent)
                .render(Rect::new(area.x, y, 1, 1), buf);
            Paragraph::new("▕")
                .style(theme.accent)
                .render(Rect::new(area.right().saturating_sub(1), y, 1, 1), buf);
        }
    }

    let number_width = usize::from(show_numbers);
    let tone_width = usize::from(show_tones) * 2;
    let prefix_width = 2 + number_width + usize::from(show_numbers) + tone_width;
    let label_width = usize::from(area.width).saturating_sub(prefix_width + 2);
    if label_width == 0 {
        return;
    }
    let label_x = area.x.saturating_add(prefix_width as u16);
    if show_numbers {
        let number = item
            .number
            .map_or_else(|| " ".to_string(), |value| value.to_string());
        Paragraph::new(number)
            .style(item_style(item, selected, theme, /*supporting*/ true))
            .render(Rect::new(area.x.saturating_add(2), area.y, 1, 1), buf);
    }
    if show_tones {
        let (marker, style) = match item.tone {
            MenuTone::Default => (" ", theme.muted),
            MenuTone::Warning => ("!", theme.warning),
            MenuTone::Destructive => ("!", theme.error),
        };
        Paragraph::new(marker)
            .style(style)
            .render(Rect::new(label_x.saturating_sub(2), area.y, 1, 1), buf);
    }

    let label = truncate(item.label, label_width as u16);
    let mut chars = label.chars();
    let first = chars.next().map(|character| character.to_string());
    let rest = chars.collect::<String>();
    let base_style = item_style(item, selected, theme, /*supporting*/ false);
    let first_style = if item.mnemonic.is_some() {
        base_style.add_modifier(Modifier::BOLD)
    } else {
        base_style.remove_modifier(Modifier::BOLD)
    };
    Paragraph::new(Line::from(vec![
        Span::styled(first.unwrap_or_default(), first_style),
        Span::styled(rest, base_style.remove_modifier(Modifier::BOLD)),
    ]))
    .render(Rect::new(label_x, area.y, label_width as u16, 1), buf);

    let descriptions = wrap_lines(
        item.description,
        label_width as u16,
        area.height.saturating_sub(1),
    );
    for (index, description) in descriptions.into_iter().enumerate() {
        Paragraph::new(description)
            .style(item_style(item, selected, theme, /*supporting*/ true))
            .render(
                Rect::new(
                    label_x,
                    area.y.saturating_add(1 + index as u16),
                    label_width as u16,
                    1,
                ),
                buf,
            );
    }
}

fn item_style(item: &PrototypeItem, selected: bool, theme: Theme, supporting: bool) -> Style {
    if item.disabled {
        theme.disabled
    } else if selected {
        theme.accent
    } else if supporting {
        theme.muted
    } else {
        match item.tone {
            MenuTone::Default => theme.text,
            MenuTone::Warning => theme.warning,
            MenuTone::Destructive => theme.error,
        }
    }
}

fn item_heights(menu: &PrototypeMenu, content_width: u16) -> Vec<u16> {
    let show_numbers = menu.items.iter().any(|item| item.number.is_some());
    let show_tones = menu
        .items
        .iter()
        .any(|item| !matches!(item.tone, MenuTone::Default));
    let prefix = 4 + u16::from(show_numbers) * 2 + u16::from(show_tones) * 2;
    let label_width = content_width.saturating_sub(prefix).max(1);
    menu.items
        .iter()
        .map(|item| {
            if content_width < 40 {
                1 + wrap_lines(item.description, label_width, 2).len().max(1) as u16
            } else {
                2
            }
        })
        .collect()
}

fn wrap_lines(text: &str, width: u16, maximum: u16) -> Vec<String> {
    if width == 0 || maximum == 0 {
        return Vec::new();
    }
    let wrapped = textwrap::wrap(
        text,
        Options::new(usize::from(width))
            .break_words(true)
            .word_splitter(textwrap::WordSplitter::NoHyphenation),
    );
    let truncated = wrapped.len() > usize::from(maximum);
    let mut lines = wrapped
        .into_iter()
        .take(usize::from(maximum))
        .map(std::borrow::Cow::into_owned)
        .collect::<Vec<_>>();
    if truncated && let Some(last) = lines.last_mut() {
        *last = truncate_with_ellipsis(last, width);
    }
    lines
}

fn truncate(text: &str, width: u16) -> String {
    if UnicodeWidthStr::width(text) <= usize::from(width) {
        return text.to_string();
    }
    truncate_with_ellipsis(text, width)
}

fn truncate_with_ellipsis(text: &str, width: u16) -> String {
    if width == 0 {
        return String::new();
    }
    let target = usize::from(width.saturating_sub(1));
    let mut rendered = String::new();
    let mut rendered_width = 0;
    for character in text.chars() {
        let character_width = character.width().unwrap_or(0);
        if rendered_width + character_width > target {
            break;
        }
        rendered.push(character);
        rendered_width += character_width;
    }
    rendered.push('…');
    rendered
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width.min(area.width),
        height.min(area.height),
    )
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
