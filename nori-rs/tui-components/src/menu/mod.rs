//! A bounded action menu with validated shortcuts and centered overlay presentation.
//!
//! Consumers translate raw events into [`MenuAction`], route [`MenuOutcome`],
//! and provide the rectangle passed to [`OverlayMenu`]. The component owns
//! only menu-local selection, viewport state, validation, and presentation.

use std::error::Error;
use std::fmt;

mod layout;
mod render;

pub use render::OverlayMenu;

/// A shortcut explicitly assigned to one menu item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuShortcut {
    /// A case-insensitive ASCII alphabetic mnemonic.
    Character(char),
    /// A visible single-digit shortcut in the range `1..=9`.
    Number(i32),
}

/// Semantic consequence of activating a menu item.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuItemTone {
    /// An ordinary action.
    #[default]
    Default,
    /// An action whose consequence deserves caution.
    Warning,
    /// An action with a destructive consequence.
    Destructive,
}

/// One domain-free action in a bounded menu.
///
/// Consumers translate their own key or event types into these actions. This
/// crate intentionally has no direct crossterm dependency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// Select the previous enabled item, wrapping at the start.
    MoveUp,
    /// Select the next enabled item, wrapping at the end.
    MoveDown,
    /// Move toward the first item by the last rendered viewport capacity.
    PageUp,
    /// Move toward the last item by the last rendered viewport capacity.
    PageDown,
    /// Select the first enabled item.
    First,
    /// Select the last enabled item.
    Last,
    /// Activate the selected enabled item.
    ActivateSelected,
    /// Immediately activate the enabled item assigned to a shortcut.
    InvokeShortcut(MenuShortcut),
    /// Report cancellation without performing application routing.
    Cancel,
}

/// Typed result of applying one menu action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuOutcome<K> {
    /// The action made no menu-local change.
    Unchanged,
    /// Selection changed to the returned stable key, or to no item.
    SelectionChanged(Option<K>),
    /// The item with the returned stable key was invoked.
    Activated(K),
    /// The consumer requested cancellation.
    Cancelled,
}

/// Validation failure for a bounded menu model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuModelError {
    /// Two items have the same stable key.
    DuplicateKey,
    /// Two items have the same case-insensitive mnemonic.
    DuplicateCharacterShortcut(char),
    /// Two items have the same number shortcut.
    DuplicateNumberShortcut(i32),
    /// A mnemonic is not an ASCII alphabetic character.
    InvalidCharacterShortcut(char),
    /// A number shortcut is outside `1..=9`.
    InvalidNumberShortcut(i32),
    /// A mnemonic does not match the first visible label character.
    MnemonicDoesNotMatchLabel { mnemonic: char, label: String },
}

impl fmt::Display for MenuModelError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateKey => write!(formatter, "menu item keys must be unique"),
            Self::DuplicateCharacterShortcut(character) => {
                write!(formatter, "duplicate character shortcut: {character}")
            }
            Self::DuplicateNumberShortcut(number) => {
                write!(formatter, "duplicate number shortcut: {number}")
            }
            Self::InvalidCharacterShortcut(character) => {
                write!(formatter, "invalid character shortcut: {character}")
            }
            Self::InvalidNumberShortcut(number) => {
                write!(formatter, "invalid number shortcut: {number}")
            }
            Self::MnemonicDoesNotMatchLabel { mnemonic, label } => {
                write!(
                    formatter,
                    "mnemonic {mnemonic} does not match label {label}"
                )
            }
        }
    }
}

impl Error for MenuModelError {}

/// One bounded menu action. The stable key is returned unchanged in outcomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuItem<K> {
    pub(crate) key: K,
    pub(crate) label: String,
    pub(crate) description: Option<String>,
    pub(crate) mnemonic: Option<char>,
    pub(crate) number_shortcut: Option<i32>,
    pub(crate) disabled: bool,
    pub(crate) current: bool,
    pub(crate) tone: MenuItemTone,
}

impl<K> MenuItem<K> {
    /// Creates an enabled, default-tone item with no shortcuts or description.
    pub fn new(key: K, label: impl Into<String>) -> Self {
        Self {
            key,
            label: label.into(),
            description: None,
            mnemonic: None,
            number_shortcut: None,
            disabled: false,
            current: false,
            tone: MenuItemTone::Default,
        }
    }

    /// Adds supporting prose rendered below the primary label.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Assigns the visible, case-insensitive first-character mnemonic.
    pub fn mnemonic(mut self, mnemonic: char) -> Self {
        self.mnemonic = Some(mnemonic);
        self
    }

    /// Assigns a visible single-digit shortcut.
    pub fn number_shortcut(mut self, number: i32) -> Self {
        self.number_shortcut = Some(number);
        self
    }

    /// Sets whether navigation and activation skip this item.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Marks the item as the consumer's current value without selecting it.
    pub fn current(mut self, current: bool) -> Self {
        self.current = current;
        self
    }

    /// Sets the semantic consequence used when the item is not selected.
    pub fn tone(mut self, tone: MenuItemTone) -> Self {
        self.tone = tone;
        self
    }

    /// Returns the stable consumer key.
    pub fn key(&self) -> &K {
        &self.key
    }

    /// Returns the primary label unchanged.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the optional supporting prose.
    pub fn description_text(&self) -> Option<&str> {
        self.description.as_deref()
    }

    /// Returns the optional character mnemonic.
    pub fn mnemonic_shortcut(&self) -> Option<char> {
        self.mnemonic
    }

    /// Returns the optional number shortcut.
    pub fn number(&self) -> Option<i32> {
        self.number_shortcut
    }

    /// Reports whether the item is unavailable for navigation and activation.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Reports whether the consumer marked this as its current value.
    pub fn is_current(&self) -> bool {
        self.current
    }

    /// Returns the semantic consequence tone.
    pub fn item_tone(&self) -> MenuItemTone {
        self.tone
    }
}

/// Caller-owned, menu-local interaction and viewport state.
///
/// This state contains no terminal handles, raw input events, application
/// actions, callbacks, async tasks, focus stack, or persistence behavior.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuState<K> {
    pub(crate) items: Vec<MenuItem<K>>,
    pub(crate) selected_index: Option<usize>,
    pub(crate) viewport_offset: usize,
    pub(crate) viewport_capacity: usize,
}

impl<K: Eq> MenuState<K> {
    /// Validates a bounded menu and selects its first enabled item.
    ///
    /// Keys and shortcuts must be unique. Mnemonics are explicit ASCII
    /// alphabetic characters matching the label's first visible character;
    /// number shortcuts are restricted to `1..=9`. Empty and all-disabled
    /// menus are valid and begin without a selection.
    pub fn try_new(items: impl IntoIterator<Item = MenuItem<K>>) -> Result<Self, MenuModelError> {
        let items = items.into_iter().collect::<Vec<_>>();
        for (index, item) in items.iter().enumerate() {
            if items[..index].iter().any(|prior| prior.key == item.key) {
                return Err(MenuModelError::DuplicateKey);
            }
            if let Some(number) = item.number_shortcut {
                if !(1..=9).contains(&number) {
                    return Err(MenuModelError::InvalidNumberShortcut(number));
                }
                if items[..index]
                    .iter()
                    .any(|prior| prior.number_shortcut == Some(number))
                {
                    return Err(MenuModelError::DuplicateNumberShortcut(number));
                }
            }
            if let Some(mnemonic) = item.mnemonic {
                if !mnemonic.is_ascii_alphabetic() {
                    return Err(MenuModelError::InvalidCharacterShortcut(mnemonic));
                }
                let normalized = mnemonic.to_ascii_lowercase();
                if items[..index].iter().any(|prior| {
                    prior
                        .mnemonic
                        .is_some_and(|prior| prior.to_ascii_lowercase() == normalized)
                }) {
                    return Err(MenuModelError::DuplicateCharacterShortcut(normalized));
                }
                let matches_label = item
                    .label
                    .trim_start()
                    .chars()
                    .next()
                    .is_some_and(|first| first.eq_ignore_ascii_case(&mnemonic));
                if !matches_label {
                    return Err(MenuModelError::MnemonicDoesNotMatchLabel {
                        mnemonic,
                        label: item.label.clone(),
                    });
                }
            }
        }
        let selected_index = items.iter().position(|item| !item.disabled);
        Ok(Self {
            items,
            selected_index,
            viewport_offset: 0,
            viewport_capacity: 1,
        })
    }

    /// Returns the validated items in display order.
    pub fn items(&self) -> &[MenuItem<K>] {
        &self.items
    }

    /// Returns the selected display index, if an enabled item is selected.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Returns the selected item.
    pub fn selected_item(&self) -> Option<&MenuItem<K>> {
        self.selected_index.and_then(|index| self.items.get(index))
    }

    /// Returns the first item in the last rendered viewport.
    pub fn viewport_offset(&self) -> usize {
        self.viewport_offset
    }
}

impl<K: Clone + Eq> MenuState<K> {
    /// Selects an enabled item by stable key.
    pub fn select_key(&mut self, key: &K) -> MenuOutcome<K> {
        let Some(index) = self
            .items
            .iter()
            .position(|item| &item.key == key && !item.disabled)
        else {
            return MenuOutcome::Unchanged;
        };
        self.select_index(index)
    }

    /// Applies a domain-free action and returns a typed outcome.
    ///
    /// This mutates only menu-local selection and viewport state. Activation
    /// and cancellation remain facts for the consumer to route.
    pub fn handle(&mut self, action: MenuAction) -> MenuOutcome<K> {
        match action {
            MenuAction::MoveUp => self.move_wrapping(-1),
            MenuAction::MoveDown => self.move_wrapping(1),
            MenuAction::PageUp => self.move_page(false),
            MenuAction::PageDown => self.move_page(true),
            MenuAction::First => self.select_edge(false),
            MenuAction::Last => self.select_edge(true),
            MenuAction::ActivateSelected => self.activate_selected(),
            MenuAction::InvokeShortcut(shortcut) => self.invoke_shortcut(shortcut),
            MenuAction::Cancel => MenuOutcome::Cancelled,
        }
    }

    fn available_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (!item.disabled).then_some(index))
            .collect()
    }

    fn move_wrapping(&mut self, delta: i32) -> MenuOutcome<K> {
        let available = self.available_indices();
        if available.len() <= 1 {
            return MenuOutcome::Unchanged;
        }
        let current = self
            .selected_index
            .and_then(|selected| available.iter().position(|index| *index == selected))
            .unwrap_or(0) as i32;
        let next = (current + delta).rem_euclid(available.len() as i32) as usize;
        self.select_index(available[next])
    }

    fn move_page(&mut self, forward: bool) -> MenuOutcome<K> {
        let available = self.available_indices();
        if available.len() <= 1 {
            return MenuOutcome::Unchanged;
        }
        let current = self
            .selected_index
            .and_then(|selected| available.iter().position(|index| *index == selected))
            .unwrap_or(0);
        let distance = self.viewport_capacity.max(1);
        let next = if forward {
            current.saturating_add(distance).min(available.len() - 1)
        } else {
            current.saturating_sub(distance)
        };
        self.select_index(available[next])
    }

    fn select_edge(&mut self, last: bool) -> MenuOutcome<K> {
        let available = self.available_indices();
        let selected = if last {
            available.last().copied()
        } else {
            available.first().copied()
        };
        selected.map_or(MenuOutcome::Unchanged, |index| self.select_index(index))
    }

    fn activate_selected(&self) -> MenuOutcome<K> {
        self.selected_item()
            .filter(|item| !item.disabled)
            .map_or(MenuOutcome::Unchanged, |item| {
                MenuOutcome::Activated(item.key.clone())
            })
    }

    fn invoke_shortcut(&mut self, shortcut: MenuShortcut) -> MenuOutcome<K> {
        let matched = self.items.iter().position(|item| match shortcut {
            MenuShortcut::Character(character) => item
                .mnemonic
                .is_some_and(|mnemonic| mnemonic.eq_ignore_ascii_case(&character)),
            MenuShortcut::Number(number) => item.number_shortcut == Some(number),
        });
        let Some(index) = matched.filter(|index| !self.items[*index].disabled) else {
            return MenuOutcome::Unchanged;
        };
        self.selected_index = Some(index);
        self.ensure_selected_visible();
        MenuOutcome::Activated(self.items[index].key.clone())
    }

    fn select_index(&mut self, index: usize) -> MenuOutcome<K> {
        if self.selected_index == Some(index) {
            return MenuOutcome::Unchanged;
        }
        self.selected_index = Some(index);
        self.ensure_selected_visible();
        MenuOutcome::SelectionChanged(Some(self.items[index].key.clone()))
    }

    fn ensure_selected_visible(&mut self) {
        let Some(selected) = self.selected_index else {
            self.viewport_offset = 0;
            return;
        };
        if selected < self.viewport_offset {
            self.viewport_offset = selected;
        } else if selected >= self.viewport_offset.saturating_add(self.viewport_capacity) {
            self.viewport_offset = selected
                .saturating_add(1)
                .saturating_sub(self.viewport_capacity);
        }
    }
}

#[cfg(test)]
mod tests;
