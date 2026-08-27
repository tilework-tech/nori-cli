use std::collections::BTreeMap;

use nucleo_matcher::Config;
use nucleo_matcher::Matcher;
use nucleo_matcher::Utf32Str;
use nucleo_matcher::pattern::AtomKind;
use nucleo_matcher::pattern::CaseMatching;
use nucleo_matcher::pattern::Normalization;
use nucleo_matcher::pattern::Pattern;
use ratatui::text::Line;

use crate::ProviderKind;

mod render;

pub use render::Picker;

/// How a picker column receives horizontal space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerColumnWidth {
    Fixed(u16),
    Flexible { min: u16, max: u16, weight: u16 },
}

/// Declarative picker column. Callers control order by vector order and can
/// suppress a column below a viewport width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerColumn {
    pub key: String,
    pub header: String,
    pub width: PickerColumnWidth,
    pub hide_below: Option<u16>,
}

impl PickerColumn {
    pub fn flexible(key: impl Into<String>, header: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            header: header.into(),
            width: PickerColumnWidth::Flexible {
                min: 8,
                max: 48,
                weight: 1,
            },
            hide_below: None,
        }
    }

    pub fn fixed(key: impl Into<String>, header: impl Into<String>, width: u16) -> Self {
        Self {
            key: key.into(),
            header: header.into(),
            width: PickerColumnWidth::Fixed(width),
            hide_below: None,
        }
    }

    pub fn width(mut self, width: PickerColumnWidth) -> Self {
        self.width = width;
        self
    }

    pub fn hide_below(mut self, viewport_width: u16) -> Self {
        self.hide_below = Some(viewport_width);
        self
    }
}

/// One domain-free picker row. The key is returned unchanged in outcomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerItem<K> {
    pub key: K,
    pub cells: BTreeMap<String, String>,
    pub cell_tones: BTreeMap<String, ProviderKind>,
    pub search_text: String,
    pub category: Option<String>,
    pub detail: Vec<Line<'static>>,
    pub details: Vec<PickerDetail>,
    pub description: Option<String>,
    pub disabled: bool,
    pub section_heading: bool,
    pub current: bool,
    pub default: bool,
    pub read_only: bool,
    pub pinned: bool,
}

impl<K> PickerItem<K> {
    pub fn new(key: K, primary_column: impl Into<String>, label: impl Into<String>) -> Self {
        let label = label.into();
        Self {
            key,
            cells: BTreeMap::from([(primary_column.into(), label.clone())]),
            cell_tones: BTreeMap::new(),
            search_text: label,
            category: None,
            detail: Vec::new(),
            details: Vec::new(),
            description: None,
            disabled: false,
            section_heading: false,
            current: false,
            default: false,
            read_only: false,
            pinned: false,
        }
    }

    pub fn cell(mut self, column: impl Into<String>, value: impl Into<String>) -> Self {
        self.cells.insert(column.into(), value.into());
        self
    }

    /// Apply an agent/provider tone to one rendered cell.
    pub fn cell_tone(mut self, column: impl Into<String>, provider: ProviderKind) -> Self {
        self.cell_tones.insert(column.into(), provider);
        self
    }

    pub fn search_text(mut self, search_text: impl Into<String>) -> Self {
        self.search_text = search_text.into();
        self
    }

    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    pub fn detail(mut self, detail: impl IntoIterator<Item = Line<'static>>) -> Self {
        self.detail = detail.into_iter().collect();
        self
    }

    /// Add structured metadata for the detail pane.
    ///
    /// Prefer this to [`Self::detail`]. Labels are aligned by the renderer and
    /// must not contain trailing colons.
    pub fn details(mut self, details: impl IntoIterator<Item = PickerDetail>) -> Self {
        self.details = details.into_iter().collect();
        self
    }

    /// Add the secondary row shown in normal density.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Render this non-interactive row as a bold section heading.
    pub fn section_heading(mut self, section_heading: bool) -> Self {
        self.section_heading = section_heading;
        if section_heading {
            self.disabled = true;
        }
        self
    }

    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }

    pub fn default_item(mut self, default: bool) -> Self {
        self.default = default;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    fn is_noninteractive(&self) -> bool {
        self.disabled || self.section_heading
    }
}

/// One label/value entry in a picker's aligned metadata pane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerDetail {
    pub label: String,
    pub value: Line<'static>,
}

impl PickerDetail {
    pub fn new(label: impl Into<String>, value: impl Into<Line<'static>>) -> Self {
        Self {
            label: label.into().trim_end_matches(':').to_string(),
            value: value.into(),
        }
    }
}

/// Vertical anatomy used to render picker rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PickerDensity {
    Compact,
    #[default]
    Normal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PickerMode {
    #[default]
    Single,
    Toggle,
    Multi,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum SearchMode {
    None,
    #[default]
    Substring,
    Fuzzy,
    /// Caller-supplied scoring function. `None` excludes an item; larger
    /// scores sort before smaller scores.
    Custom(fn(query: &str, search_text: &str) -> Option<u32>),
}

impl PartialEq for SearchMode {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::None, Self::None)
                | (Self::Substring, Self::Substring)
                | (Self::Fuzzy, Self::Fuzzy)
                | (Self::Custom(_), Self::Custom(_))
        )
    }
}

impl Eq for SearchMode {}

/// Caller-controlled asynchronous content state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PickerLoadState {
    #[default]
    Ready,
    Loading(String),
    Failed(String),
}

/// Input vocabulary understood by picker state. Consumers translate their
/// own keymaps and event systems into these actions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerAction {
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    First,
    Last,
    Submit,
    Toggle,
    Cancel,
    ActivateSearch,
    DeactivateSearch,
    AppendQuery(char),
    Backspace,
    ClearQuery,
    NextCategory,
    PreviousCategory,
}

/// Typed result of applying one picker action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickerOutcome<K> {
    Unchanged,
    SelectionChanged(Option<K>),
    Selected(K),
    Toggled { key: K, selected: bool },
    Submitted(Vec<K>),
    SearchModeChanged(bool),
    QueryChanged(String),
    CategoryChanged(Option<String>),
    Cancelled,
}

/// Caller-owned picker state. It contains no terminal or application event
/// handles and can be updated independently from rendering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerState<K> {
    pub title: String,
    pub subtitle: Option<String>,
    pub columns: Vec<PickerColumn>,
    pub items: Vec<PickerItem<K>>,
    pub mode: PickerMode,
    pub search_mode: SearchMode,
    pub search_active: bool,
    pub query: String,
    pub categories: Vec<String>,
    pub category_tones: BTreeMap<String, ProviderKind>,
    pub active_category: Option<String>,
    pub selected_index: Option<usize>,
    pub selected_keys: Vec<K>,
    pub page_size: usize,
    pub load_state: PickerLoadState,
    pub search_placeholder: String,
}

impl<K: Clone + Eq> PickerState<K> {
    pub fn new(
        title: impl Into<String>,
        columns: impl IntoIterator<Item = PickerColumn>,
        items: impl IntoIterator<Item = PickerItem<K>>,
    ) -> Self {
        let mut state = Self {
            title: title.into(),
            subtitle: None,
            columns: columns.into_iter().collect(),
            items: items.into_iter().collect(),
            mode: PickerMode::Single,
            search_mode: SearchMode::Substring,
            search_active: false,
            query: String::new(),
            categories: Vec::new(),
            category_tones: BTreeMap::new(),
            active_category: None,
            selected_index: None,
            selected_keys: Vec::new(),
            page_size: 8,
            load_state: PickerLoadState::Ready,
            search_placeholder: "Type to search".to_string(),
        };
        state.select_first_available();
        state
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn mode(mut self, mode: PickerMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn search_mode(mut self, search_mode: SearchMode) -> Self {
        self.search_mode = search_mode;
        if matches!(search_mode, SearchMode::None) {
            self.search_active = false;
            self.query.clear();
        }
        self
    }

    pub fn categories(mut self, categories: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.categories = categories.into_iter().map(Into::into).collect();
        self
    }

    /// Apply an agent/provider tone to one category tab.
    pub fn category_tone(mut self, category: impl Into<String>, provider: ProviderKind) -> Self {
        self.category_tones.insert(category.into(), provider);
        self
    }

    pub fn search_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.search_placeholder = placeholder.into();
        self
    }

    pub fn page_size(mut self, page_size: usize) -> Self {
        self.page_size = page_size.max(1);
        self
    }

    pub fn selected_item(&self) -> Option<&PickerItem<K>> {
        self.selected_index.and_then(|index| self.items.get(index))
    }

    pub fn visible_indices(&self) -> Vec<usize> {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let pattern = Pattern::new(
            &self.query,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Fuzzy,
        );
        let mut scored = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                self.active_category
                    .as_ref()
                    .is_none_or(|category| item.category.as_ref() == Some(category))
            })
            .filter_map(|(index, item)| {
                let score = match self.search_mode {
                    SearchMode::None => self.query.is_empty().then_some(0),
                    SearchMode::Substring => item
                        .search_text
                        .to_lowercase()
                        .contains(&self.query.to_lowercase())
                        .then_some(0),
                    SearchMode::Fuzzy => {
                        let mut haystack_buf = Vec::new();
                        pattern.score(
                            Utf32Str::new(&item.search_text, &mut haystack_buf),
                            &mut matcher,
                        )
                    }
                    SearchMode::Custom(score) => score(&self.query, &item.search_text),
                }?;
                Some((index, item.pinned, score))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| {
            right
                .1
                .cmp(&left.1)
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.cmp(&right.0))
        });
        scored.into_iter().map(|(index, _, _)| index).collect()
    }

    pub fn handle(&mut self, action: PickerAction) -> PickerOutcome<K> {
        match action {
            PickerAction::MoveUp => self.move_selection(-1),
            PickerAction::MoveDown => self.move_selection(1),
            PickerAction::PageUp => self.move_selection(-(self.page_size as i32)),
            PickerAction::PageDown => self.move_selection(self.page_size as i32),
            PickerAction::First => self.select_edge(false),
            PickerAction::Last => self.select_edge(true),
            PickerAction::Submit => self.submit(),
            PickerAction::Toggle => self.toggle(),
            PickerAction::Cancel => PickerOutcome::Cancelled,
            PickerAction::ActivateSearch => {
                if matches!(self.search_mode, SearchMode::None) || self.search_active {
                    return PickerOutcome::Unchanged;
                }
                self.search_active = true;
                PickerOutcome::SearchModeChanged(true)
            }
            PickerAction::DeactivateSearch => {
                if !self.search_active {
                    return PickerOutcome::Unchanged;
                }
                self.search_active = false;
                self.query.clear();
                self.select_first_available();
                PickerOutcome::SearchModeChanged(false)
            }
            PickerAction::AppendQuery(character) => {
                if !self.search_active {
                    return PickerOutcome::Unchanged;
                }
                self.query.push(character);
                self.select_first_available();
                PickerOutcome::QueryChanged(self.query.clone())
            }
            PickerAction::Backspace => {
                if !self.search_active {
                    return PickerOutcome::Unchanged;
                }
                if self.query.pop().is_none() {
                    return PickerOutcome::Unchanged;
                }
                self.select_first_available();
                PickerOutcome::QueryChanged(self.query.clone())
            }
            PickerAction::ClearQuery => {
                if !self.search_active || self.query.is_empty() {
                    return PickerOutcome::Unchanged;
                }
                self.query.clear();
                self.select_first_available();
                PickerOutcome::QueryChanged(String::new())
            }
            PickerAction::NextCategory => self.move_category(1),
            PickerAction::PreviousCategory => self.move_category(-1),
        }
    }

    fn select_first_available(&mut self) {
        self.selected_index = self
            .visible_indices()
            .into_iter()
            .find(|index| !self.items[*index].is_noninteractive());
    }

    fn move_selection(&mut self, delta: i32) -> PickerOutcome<K> {
        let visible = self.visible_indices();
        let available = visible
            .into_iter()
            .filter(|index| !self.items[*index].is_noninteractive())
            .collect::<Vec<_>>();
        if available.is_empty() {
            self.selected_index = None;
            return PickerOutcome::SelectionChanged(None);
        }
        let current = self
            .selected_index
            .and_then(|selected| available.iter().position(|index| *index == selected))
            .unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, available.len() as i32 - 1) as usize;
        self.selected_index = Some(available[next]);
        PickerOutcome::SelectionChanged(
            Some(available[next]).map(|index| self.items[index].key.clone()),
        )
    }

    fn select_edge(&mut self, last: bool) -> PickerOutcome<K> {
        let available = self
            .visible_indices()
            .into_iter()
            .filter(|index| !self.items[*index].is_noninteractive())
            .collect::<Vec<_>>();
        let selected = if last {
            available.last().copied()
        } else {
            available.first().copied()
        };
        self.selected_index = selected;
        PickerOutcome::SelectionChanged(selected.map(|index| self.items[index].key.clone()))
    }

    fn submit(&mut self) -> PickerOutcome<K> {
        if self.mode == PickerMode::Multi {
            return PickerOutcome::Submitted(self.selected_keys.clone());
        }
        let Some(item) = self.selected_item() else {
            return PickerOutcome::Unchanged;
        };
        if item.is_noninteractive() || item.read_only {
            return PickerOutcome::Unchanged;
        }
        PickerOutcome::Selected(item.key.clone())
    }

    fn toggle(&mut self) -> PickerOutcome<K> {
        if self.mode == PickerMode::Single {
            return self.submit();
        }
        let Some(item) = self.selected_item() else {
            return PickerOutcome::Unchanged;
        };
        if item.is_noninteractive() || item.read_only {
            return PickerOutcome::Unchanged;
        }
        let key = item.key.clone();
        let selected = if let Some(index) = self
            .selected_keys
            .iter()
            .position(|selected| *selected == key)
        {
            self.selected_keys.remove(index);
            false
        } else {
            self.selected_keys.push(key.clone());
            true
        };
        PickerOutcome::Toggled { key, selected }
    }

    fn move_category(&mut self, delta: i32) -> PickerOutcome<K> {
        if self.categories.is_empty() {
            return PickerOutcome::Unchanged;
        }
        let options = std::iter::once(None)
            .chain(self.categories.iter().cloned().map(Some))
            .collect::<Vec<_>>();
        let current = options
            .iter()
            .position(|option| option == &self.active_category)
            .unwrap_or(0) as i32;
        let next = (current + delta).rem_euclid(options.len() as i32) as usize;
        self.active_category = options[next].clone();
        self.select_first_available();
        PickerOutcome::CategoryChanged(self.active_category.clone())
    }
}

#[cfg(test)]
mod tests;
