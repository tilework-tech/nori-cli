//! Selection list widget with search and navigation.

use crate::key_hint::KeyBinding;
use ratatui::style::Style;
use ratatui::text::Line;

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
pub struct SelectionList<T> {
    _phantom: std::marker::PhantomData<T>,
}

impl<T> SelectionList<T> {
    /// Creates a new SelectionList.
    pub fn new(
        _config: SelectionListConfig,
        _items: Vec<SelectionItem<T>>,
        _header: Box<dyn crate::render::Renderable>,
    ) -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}
