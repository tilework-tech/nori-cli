//! Selection list widget with search and navigation.

use super::MAX_POPUP_ROWS;
use super::common::{GenericDisplayRow, measure_rows_height, render_rows};
use crate::key_hint::KeyBinding;
use crate::render::{Insets, RectExt, Renderable};
use crate::scroll_state::ScrollState;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use itertools::Itertools;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget};

/// Configuration for a single selection item.
#[derive(Clone)]
pub struct SelectionItem<T> {
    /// The data payload for this item
    pub data: T,
    /// Display name
    pub name: String,
    /// Optional keyboard shortcut hint
    pub display_shortcut: Option<KeyBinding>,
    /// Optional description shown when not selected
    pub description: Option<String>,
    /// Optional description shown when selected
    pub selected_description: Option<String>,
    /// Whether this is the current/active item
    pub is_current: bool,
    /// Optional search value (defaults to name if None)
    pub search_value: Option<String>,
}

/// Configuration for SelectionList appearance and behavior.
#[derive(Clone)]
pub struct SelectionListConfig {
    /// Optional title at top
    pub title: Option<String>,
    /// Optional subtitle below title
    pub subtitle: Option<String>,
    /// Optional footer hint
    pub footer_hint: Option<Line<'static>>,
    /// Style for the popup block
    pub block_style: Style,
    /// Whether search is enabled
    pub is_searchable: bool,
    /// Placeholder text for search box
    pub search_placeholder: Option<String>,
    /// Message shown when list is empty
    pub empty_message: String,
}

impl Default for SelectionListConfig {
    fn default() -> Self {
        Self {
            title: None,
            subtitle: None,
            footer_hint: None,
            block_style: Style::default(),
            is_searchable: false,
            search_placeholder: None,
            empty_message: "no matches".to_string(),
        }
    }
}

impl SelectionListConfig {
    /// Creates a new configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets the subtitle.
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Sets the footer hint.
    pub fn with_footer_hint(mut self, hint: Line<'static>) -> Self {
        self.footer_hint = Some(hint);
        self
    }

    /// Sets the block style.
    pub fn with_block_style(mut self, style: Style) -> Self {
        self.block_style = style;
        self
    }

    /// Enables search with optional placeholder.
    pub fn with_search(mut self, placeholder: Option<String>) -> Self {
        self.is_searchable = true;
        self.search_placeholder = placeholder;
        self
    }

    /// Sets the empty message.
    pub fn with_empty_message(mut self, message: impl Into<String>) -> Self {
        self.empty_message = message.into();
        self
    }
}

/// Events emitted by SelectionList keyboard handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionListEvent {
    /// An item was selected (index into original items vec)
    Selected(usize),
    /// Selection was cancelled
    Cancelled,
    /// No significant event
    None,
}

/// Interactive selection list widget.
///
/// A generic, configurable selection list with keyboard navigation, optional search,
/// and customizable styling. The widget is generic over the data type `T` associated
/// with each item.
///
/// # Examples
///
/// ```rust
/// use tui_components::selection::{SelectionList, SelectionListConfig, SelectionItem};
///
/// #[derive(Clone)]
/// struct MyData {
///     id: u32,
/// }
///
/// let config = SelectionListConfig::new()
///     .with_title("Select an option")
///     .with_search(Some("Type to search...".to_string()));
///
/// let items = vec![
///     SelectionItem {
///         data: MyData { id: 1 },
///         name: "Option 1".to_string(),
///         description: Some("First option".to_string()),
///         search_value: Some("option 1".to_string()),
///         is_current: false,
///         display_shortcut: None,
///         selected_description: None,
///     },
/// ];
///
/// let mut list = SelectionList::new(config, items, Box::new(()));
/// ```
pub struct SelectionList<T> {
    config: SelectionListConfig,
    items: Vec<SelectionItem<T>>,
    state: ScrollState,
    search_query: String,
    filtered_indices: Vec<usize>,
    header: Box<dyn Renderable>,
}

impl<T> SelectionList<T> {
    /// Creates a new SelectionList.
    ///
    /// # Arguments
    ///
    /// * `config` - Configuration for appearance and behavior
    /// * `items` - List of selectable items
    /// * `header` - Custom header renderable (use `Box::new(())` for none)
    pub fn new(
        config: SelectionListConfig,
        items: Vec<SelectionItem<T>>,
        header: Box<dyn Renderable>,
    ) -> Self {
        let mut header = header;
        if config.title.is_some() || config.subtitle.is_some() {
            let title = config.title.as_ref().map(|t| Line::from(t.clone().bold()));
            let subtitle = config
                .subtitle
                .as_ref()
                .map(|s| Line::from(s.clone().dim()));
            header = Box::new(crate::render::ColumnRenderable::with([
                header,
                Box::new(title),
                Box::new(subtitle),
            ]));
        }

        let mut s = Self {
            config,
            items,
            state: ScrollState::new(),
            search_query: String::new(),
            filtered_indices: Vec::new(),
            header,
        };
        s.apply_filter();
        s
    }

    fn visible_len(&self) -> usize {
        self.filtered_indices.len()
    }

    fn max_visible_rows(len: usize) -> usize {
        MAX_POPUP_ROWS.min(len.max(1))
    }

    fn apply_filter(&mut self) {
        let previously_selected = self
            .state
            .selected_idx
            .and_then(|visible_idx| self.filtered_indices.get(visible_idx).copied())
            .or_else(|| {
                (!self.config.is_searchable)
                    .then(|| self.items.iter().position(|item| item.is_current))
                    .flatten()
            });

        if self.config.is_searchable && !self.search_query.is_empty() {
            let query_lower = self.search_query.to_lowercase();
            self.filtered_indices = self
                .items
                .iter()
                .positions(|item| {
                    item.search_value
                        .as_ref()
                        .is_some_and(|v| v.to_lowercase().contains(&query_lower))
                })
                .collect();
        } else {
            self.filtered_indices = (0..self.items.len()).collect();
        }

        let len = self.filtered_indices.len();
        self.state.selected_idx = self
            .state
            .selected_idx
            .and_then(|visible_idx| {
                self.filtered_indices
                    .get(visible_idx)
                    .and_then(|idx| self.filtered_indices.iter().position(|cur| cur == idx))
            })
            .or_else(|| {
                previously_selected.and_then(|actual_idx| {
                    self.filtered_indices
                        .iter()
                        .position(|idx| *idx == actual_idx)
                })
            })
            .or_else(|| (len > 0).then_some(0));

        let visible = Self::max_visible_rows(len);
        self.state.clamp_selection(len);
        self.state.ensure_visible(len, visible);
    }

    fn build_rows(&self) -> Vec<GenericDisplayRow> {
        self.filtered_indices
            .iter()
            .enumerate()
            .filter_map(|(visible_idx, actual_idx)| {
                self.items.get(*actual_idx).map(|item| {
                    let is_selected = self.state.selected_idx == Some(visible_idx);
                    let prefix = if is_selected { '›' } else { ' ' };
                    let name = item.name.as_str();
                    let name_with_marker = if item.is_current {
                        format!("{name} (current)")
                    } else {
                        item.name.clone()
                    };
                    let n = visible_idx + 1;
                    let display_name = if self.config.is_searchable {
                        format!("{prefix} {name_with_marker}")
                    } else {
                        format!("{prefix} {n}. {name_with_marker}")
                    };
                    let description = is_selected
                        .then(|| item.selected_description.clone())
                        .flatten()
                        .or_else(|| item.description.clone());
                    GenericDisplayRow {
                        name: display_name,
                        display_shortcut: item.display_shortcut,
                        match_indices: None,
                        is_current: item.is_current,
                        description,
                    }
                })
            })
            .collect()
    }

    /// Moves selection up by one, wrapping to bottom.
    pub fn move_up(&mut self) {
        let len = self.visible_len();
        self.state.move_up_wrap(len);
        let visible = Self::max_visible_rows(len);
        self.state.ensure_visible(len, visible);
    }

    /// Moves selection down by one, wrapping to top.
    pub fn move_down(&mut self) {
        let len = self.visible_len();
        self.state.move_down_wrap(len);
        let visible = Self::max_visible_rows(len);
        self.state.ensure_visible(len, visible);
    }

    /// Returns the currently selected item, if any.
    pub fn selected_item(&self) -> Option<&SelectionItem<T>> {
        self.state
            .selected_idx
            .and_then(|idx| self.filtered_indices.get(idx))
            .and_then(|actual_idx| self.items.get(*actual_idx))
    }

    /// Returns the index of the currently selected item in the original items vec.
    pub fn selected_index(&self) -> Option<usize> {
        self.state
            .selected_idx
            .and_then(|idx| self.filtered_indices.get(idx).copied())
    }

    /// Sets the search query and reapplies filtering.
    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.apply_filter();
    }

    /// Returns the current search query.
    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    /// Handles a keyboard event and returns the resulting event.
    ///
    /// # Arguments
    ///
    /// * `key_event` - The keyboard event to handle
    ///
    /// # Returns
    ///
    /// A `SelectionListEvent` indicating what action occurred.
    pub fn handle_key_event(&mut self, key_event: KeyEvent) -> SelectionListEvent {
        match key_event {
            KeyEvent {
                code: KeyCode::Up, ..
            } => {
                self.move_up();
                SelectionListEvent::None
            }
            KeyEvent {
                code: KeyCode::Down,
                ..
            } => {
                self.move_down();
                SelectionListEvent::None
            }
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } if self.config.is_searchable => {
                self.search_query.pop();
                self.apply_filter();
                SelectionListEvent::None
            }
            KeyEvent {
                code: KeyCode::Esc, ..
            } => SelectionListEvent::Cancelled,
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                ..
            } if self.config.is_searchable
                && !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
            {
                self.search_query.push(c);
                self.apply_filter();
                SelectionListEvent::None
            }
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                ..
            } if !self.config.is_searchable
                && !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(idx) = c
                    .to_digit(10)
                    .map(|d| d as usize)
                    .and_then(|d| d.checked_sub(1))
                    && idx < self.filtered_indices.len()
                    && let Some(actual_idx) = self.filtered_indices.get(idx)
                {
                    return SelectionListEvent::Selected(*actual_idx);
                }
                SelectionListEvent::None
            }
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => {
                if let Some(actual_idx) = self.selected_index() {
                    SelectionListEvent::Selected(actual_idx)
                } else {
                    SelectionListEvent::None
                }
            }
            _ => SelectionListEvent::None,
        }
    }
}

impl<T> Renderable for SelectionList<T> {
    fn desired_height(&self, width: u16) -> u16 {
        let rows = self.build_rows();
        let rows_height = measure_rows_height(&rows, &self.state, MAX_POPUP_ROWS, width);

        let mut height = self.header.desired_height(width.saturating_sub(4));
        height = height.saturating_add(rows_height + 3);
        if self.config.is_searchable {
            height = height.saturating_add(1);
        }
        if self.config.footer_hint.is_some() {
            height = height.saturating_add(1);
        }
        height
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let [content_area, footer_area] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(if self.config.footer_hint.is_some() {
                1
            } else {
                0
            }),
        ])
        .areas(area);

        Block::default()
            .style(self.config.block_style)
            .render(content_area, buf);

        let header_height = self
            .header
            .desired_height(content_area.width.saturating_sub(4));
        let rows = self.build_rows();
        let rows_height =
            measure_rows_height(&rows, &self.state, MAX_POPUP_ROWS, content_area.width);
        let [header_area, _, search_area, list_area] = Layout::vertical([
            Constraint::Max(header_height),
            Constraint::Max(1),
            Constraint::Length(if self.config.is_searchable { 1 } else { 0 }),
            Constraint::Length(rows_height),
        ])
        .areas(content_area.inset(Insets::vh(1, 2)));

        if header_area.height < header_height {
            let [header_area, elision_area] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(header_area);
            self.header.render(header_area, buf);
            Paragraph::new(vec![Line::from(
                format!("[… {header_height} lines] ctrl + a view all").dim(),
            )])
            .render(elision_area, buf);
        } else {
            self.header.render(header_area, buf);
        }

        if self.config.is_searchable {
            let query_span: Span<'static> = if self.search_query.is_empty() {
                self.config
                    .search_placeholder
                    .as_ref()
                    .map(|placeholder| placeholder.clone().dim())
                    .unwrap_or_else(|| "".into())
            } else {
                self.search_query.clone().into()
            };
            Line::from(query_span).render(search_area, buf);
        }

        if list_area.height > 0 {
            let list_area = Rect {
                x: list_area.x.saturating_sub(2),
                y: list_area.y,
                width: list_area.width.saturating_add(2),
                height: list_area.height,
            };
            render_rows(
                list_area,
                buf,
                &rows,
                &self.state,
                list_area.height as usize,
                &self.config.empty_message,
            );
        }

        if let Some(hint) = &self.config.footer_hint {
            let hint_area = Rect {
                x: footer_area.x.saturating_add(2),
                y: footer_area.y,
                width: footer_area.width.saturating_sub(2),
                height: footer_area.height,
            };
            hint.clone().dim().render(hint_area, buf);
        }
    }
}
