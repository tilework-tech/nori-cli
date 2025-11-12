///! Keyboard shortcut hint rendering
///!
///! Provides platform-aware keyboard shortcut formatting for displaying in terminal UIs.
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::style::{Style, Stylize};
use ratatui::text::Span;

#[cfg(test)]
const ALT_PREFIX: &str = "⌥ + ";
#[cfg(all(not(test), target_os = "macos"))]
const ALT_PREFIX: &str = "⌥ + ";
#[cfg(all(not(test), not(target_os = "macos")))]
const ALT_PREFIX: &str = "alt + ";
const CTRL_PREFIX: &str = "ctrl + ";
const SHIFT_PREFIX: &str = "shift + ";

/// Represents a keyboard shortcut with key and modifiers
///
/// # Examples
///
/// ```
/// use crossterm::event::{KeyCode, KeyModifiers};
/// use tui_components::key_hint::KeyBinding;
///
/// let enter = KeyBinding::new(KeyCode::Enter, KeyModifiers::NONE);
/// let ctrl_c = KeyBinding::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyBinding {
    key: KeyCode,
    modifiers: KeyModifiers,
}

impl KeyBinding {
    /// Creates a new key binding with the specified key and modifiers
    ///
    /// # Example
    /// ```
    /// use crossterm::event::{KeyCode, KeyModifiers};
    /// use tui_components::key_hint::KeyBinding;
    ///
    /// let ctrl_s = KeyBinding::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    /// ```
    pub const fn new(key: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { key, modifiers }
    }

    /// Checks if a key event matches this binding (press or repeat)
    ///
    /// Returns true if the event's key code and modifiers match this binding,
    /// and the event kind is either Press or Repeat (not Release).
    ///
    /// # Example
    /// ```
    /// use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    /// use tui_components::key_hint::KeyBinding;
    ///
    /// let binding = KeyBinding::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    /// // Check against incoming key events...
    /// ```
    pub fn is_press(&self, event: KeyEvent) -> bool {
        self.key == event.code
            && self.modifiers == event.modifiers
            && (event.kind == KeyEventKind::Press || event.kind == KeyEventKind::Repeat)
    }
}

/// Creates a key binding without modifiers
///
/// # Example
/// ```
/// use crossterm::event::KeyCode;
/// use tui_components::key_hint::plain;
///
/// let enter_key = plain(KeyCode::Enter);
/// ```
pub const fn plain(key: KeyCode) -> KeyBinding {
    KeyBinding::new(key, KeyModifiers::NONE)
}

/// Creates a key binding with Alt modifier
///
/// On macOS, this is displayed with the ⌥ symbol.
///
/// # Example
/// ```
/// use crossterm::event::KeyCode;
/// use tui_components::key_hint::alt;
///
/// let alt_f = alt(KeyCode::Char('f'));
/// ```
pub const fn alt(key: KeyCode) -> KeyBinding {
    KeyBinding::new(key, KeyModifiers::ALT)
}

/// Creates a key binding with Shift modifier
///
/// # Example
/// ```
/// use crossterm::event::KeyCode;
/// use tui_components::key_hint::shift;
///
/// let shift_tab = shift(KeyCode::Tab);
/// ```
pub const fn shift(key: KeyCode) -> KeyBinding {
    KeyBinding::new(key, KeyModifiers::SHIFT)
}

/// Creates a key binding with Ctrl modifier
///
/// # Example
/// ```
/// use crossterm::event::KeyCode;
/// use tui_components::key_hint::ctrl;
///
/// let ctrl_c = ctrl(KeyCode::Char('c'));
/// ```
pub const fn ctrl(key: KeyCode) -> KeyBinding {
    KeyBinding::new(key, KeyModifiers::CONTROL)
}

fn modifiers_to_string(modifiers: KeyModifiers) -> String {
    let mut result = String::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        result.push_str(CTRL_PREFIX);
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        result.push_str(SHIFT_PREFIX);
    }
    if modifiers.contains(KeyModifiers::ALT) {
        result.push_str(ALT_PREFIX);
    }
    result
}

impl From<KeyBinding> for Span<'static> {
    fn from(binding: KeyBinding) -> Self {
        (&binding).into()
    }
}

impl From<&KeyBinding> for Span<'static> {
    fn from(binding: &KeyBinding) -> Self {
        let KeyBinding { key, modifiers } = binding;
        let modifiers = modifiers_to_string(*modifiers);
        let key = match key {
            KeyCode::Enter => "enter".to_string(),
            KeyCode::Up => "↑".to_string(),
            KeyCode::Down => "↓".to_string(),
            KeyCode::Left => "←".to_string(),
            KeyCode::Right => "→".to_string(),
            KeyCode::PageUp => "pgup".to_string(),
            KeyCode::PageDown => "pgdn".to_string(),
            _ => format!("{key}").to_ascii_lowercase(),
        };
        Span::styled(format!("{modifiers}{key}"), key_hint_style())
    }
}

fn key_hint_style() -> Style {
    Style::default().dim()
}
