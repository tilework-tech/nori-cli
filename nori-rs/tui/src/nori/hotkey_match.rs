//! Hotkey matching: converts `HotkeyBinding` strings to crossterm `KeyEvent` matches.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use nori_acp::config::HotkeyBinding;

/// Parse a `HotkeyBinding` string (e.g. "ctrl+t", "alt+g", "f1") into
/// `(KeyCode, KeyModifiers)`, or `None` if the binding is unbound.
pub(crate) fn parse_binding(binding: &HotkeyBinding) -> Option<(KeyCode, KeyModifiers)> {
    if binding.is_none() {
        return None;
    }

    let s = binding.as_str();
    let parts: Vec<&str> = s.split('+').collect();

    let mut modifiers = KeyModifiers::NONE;
    let key_part = match parts.len() {
        1 => parts[0],
        2 => {
            match parts[0] {
                "ctrl" => modifiers |= KeyModifiers::CONTROL,
                "alt" => modifiers |= KeyModifiers::ALT,
                "shift" => modifiers |= KeyModifiers::SHIFT,
                _ => return None,
            }
            parts[1]
        }
        3 => {
            for &modifier in &parts[..2] {
                match modifier {
                    "ctrl" => modifiers |= KeyModifiers::CONTROL,
                    "alt" => modifiers |= KeyModifiers::ALT,
                    "shift" => modifiers |= KeyModifiers::SHIFT,
                    _ => return None,
                }
            }
            parts[2]
        }
        _ => return None,
    };

    let key_code = parse_key_code(key_part)?;
    Some((key_code, modifiers))
}

/// Parse a key name string into a `KeyCode`.
fn parse_key_code(s: &str) -> Option<KeyCode> {
    match s {
        "enter" => Some(KeyCode::Enter),
        "esc" => Some(KeyCode::Esc),
        "backspace" => Some(KeyCode::Backspace),
        "tab" => Some(KeyCode::Tab),
        "up" => Some(KeyCode::Up),
        "down" => Some(KeyCode::Down),
        "left" => Some(KeyCode::Left),
        "right" => Some(KeyCode::Right),
        "home" => Some(KeyCode::Home),
        "end" => Some(KeyCode::End),
        "pageup" => Some(KeyCode::PageUp),
        "pagedown" => Some(KeyCode::PageDown),
        "delete" => Some(KeyCode::Delete),
        "insert" => Some(KeyCode::Insert),
        "space" => Some(KeyCode::Char(' ')),
        "f1" => Some(KeyCode::F(1)),
        "f2" => Some(KeyCode::F(2)),
        "f3" => Some(KeyCode::F(3)),
        "f4" => Some(KeyCode::F(4)),
        "f5" => Some(KeyCode::F(5)),
        "f6" => Some(KeyCode::F(6)),
        "f7" => Some(KeyCode::F(7)),
        "f8" => Some(KeyCode::F(8)),
        "f9" => Some(KeyCode::F(9)),
        "f10" => Some(KeyCode::F(10)),
        "f11" => Some(KeyCode::F(11)),
        "f12" => Some(KeyCode::F(12)),
        s if s.len() == 1 => s.chars().next().map(KeyCode::Char),
        _ => None,
    }
}

/// Check if a `KeyEvent` matches a `HotkeyBinding`.
pub(crate) fn matches_binding(binding: &HotkeyBinding, event: &KeyEvent) -> bool {
    if event.kind != KeyEventKind::Press && event.kind != KeyEventKind::Repeat {
        return false;
    }

    let Some((key_code, modifiers)) = parse_binding(binding) else {
        return false;
    };

    event.code == key_code && event.modifiers == modifiers
}

/// Convert a `KeyEvent` to a `HotkeyBinding` string for persistence.
/// Only handles Press events with Ctrl/Alt/Shift modifiers + a key.
pub(crate) fn key_event_to_binding(event: &KeyEvent) -> Option<HotkeyBinding> {
    if event.kind != KeyEventKind::Press {
        return None;
    }

    let key_name = match event.code {
        KeyCode::Char(c) => c.to_lowercase().to_string(),
        KeyCode::Enter => "enter".to_string(),
        KeyCode::Esc => "esc".to_string(),
        KeyCode::Backspace => "backspace".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::Up => "up".to_string(),
        KeyCode::Down => "down".to_string(),
        KeyCode::Left => "left".to_string(),
        KeyCode::Right => "right".to_string(),
        KeyCode::Home => "home".to_string(),
        KeyCode::End => "end".to_string(),
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::Delete => "delete".to_string(),
        KeyCode::Insert => "insert".to_string(),
        KeyCode::F(n) => format!("f{n}"),
        _ => return None,
    };

    let mut parts = Vec::new();
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl");
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt");
    }
    if event.modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift");
    }
    parts.push(&key_name);

    Some(HotkeyBinding::from_str(&parts.join("+")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    use pretty_assertions::assert_eq;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn parse_ctrl_t() {
        let binding = HotkeyBinding::from_str("ctrl+t");
        let parsed = parse_binding(&binding);
        assert_eq!(parsed, Some((KeyCode::Char('t'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn parse_alt_g() {
        let binding = HotkeyBinding::from_str("alt+g");
        let parsed = parse_binding(&binding);
        assert_eq!(parsed, Some((KeyCode::Char('g'), KeyModifiers::ALT)));
    }

    #[test]
    fn parse_shift_f1() {
        let binding = HotkeyBinding::from_str("shift+f1");
        let parsed = parse_binding(&binding);
        assert_eq!(parsed, Some((KeyCode::F(1), KeyModifiers::SHIFT)));
    }

    #[test]
    fn parse_ctrl_shift_a() {
        let binding = HotkeyBinding::from_str("ctrl+shift+a");
        let parsed = parse_binding(&binding);
        assert_eq!(
            parsed,
            Some((
                KeyCode::Char('a'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ))
        );
    }

    #[test]
    fn parse_plain_enter() {
        let binding = HotkeyBinding::from_str("enter");
        let parsed = parse_binding(&binding);
        assert_eq!(parsed, Some((KeyCode::Enter, KeyModifiers::NONE)));
    }

    #[test]
    fn parse_f12() {
        let binding = HotkeyBinding::from_str("f12");
        let parsed = parse_binding(&binding);
        assert_eq!(parsed, Some((KeyCode::F(12), KeyModifiers::NONE)));
    }

    #[test]
    fn parse_none_returns_none() {
        let binding = HotkeyBinding::none();
        assert_eq!(parse_binding(&binding), None);
    }

    #[test]
    fn parse_invalid_modifier_returns_none() {
        let binding = HotkeyBinding::from_str("super+t");
        assert_eq!(parse_binding(&binding), None);
    }

    #[test]
    fn matches_ctrl_t_event() {
        let binding = HotkeyBinding::from_str("ctrl+t");
        let event = press(KeyCode::Char('t'), KeyModifiers::CONTROL);
        assert!(matches_binding(&binding, &event));
    }

    #[test]
    fn does_not_match_wrong_key() {
        let binding = HotkeyBinding::from_str("ctrl+t");
        let event = press(KeyCode::Char('g'), KeyModifiers::CONTROL);
        assert!(!matches_binding(&binding, &event));
    }

    #[test]
    fn does_not_match_wrong_modifier() {
        let binding = HotkeyBinding::from_str("ctrl+t");
        let event = press(KeyCode::Char('t'), KeyModifiers::ALT);
        assert!(!matches_binding(&binding, &event));
    }

    #[test]
    fn unbound_matches_nothing() {
        let binding = HotkeyBinding::none();
        let event = press(KeyCode::Char('t'), KeyModifiers::CONTROL);
        assert!(!matches_binding(&binding, &event));
    }

    #[test]
    fn does_not_match_release_events() {
        let binding = HotkeyBinding::from_str("ctrl+t");
        let mut event = press(KeyCode::Char('t'), KeyModifiers::CONTROL);
        event.kind = KeyEventKind::Release;
        assert!(!matches_binding(&binding, &event));
    }

    #[test]
    fn key_event_to_binding_ctrl_t() {
        let event = press(KeyCode::Char('t'), KeyModifiers::CONTROL);
        let binding = key_event_to_binding(&event).unwrap();
        assert_eq!(binding, HotkeyBinding::from_str("ctrl+t"));
    }

    #[test]
    fn key_event_to_binding_alt_g() {
        let event = press(KeyCode::Char('g'), KeyModifiers::ALT);
        let binding = key_event_to_binding(&event).unwrap();
        assert_eq!(binding, HotkeyBinding::from_str("alt+g"));
    }

    #[test]
    fn key_event_to_binding_f5() {
        let event = press(KeyCode::F(5), KeyModifiers::NONE);
        let binding = key_event_to_binding(&event).unwrap();
        assert_eq!(binding, HotkeyBinding::from_str("f5"));
    }

    #[test]
    fn key_event_to_binding_ctrl_shift_a() {
        let event = press(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        let binding = key_event_to_binding(&event).unwrap();
        assert_eq!(binding, HotkeyBinding::from_str("ctrl+shift+a"));
    }

    #[test]
    fn key_event_to_binding_release_returns_none() {
        let mut event = press(KeyCode::Char('t'), KeyModifiers::CONTROL);
        event.kind = KeyEventKind::Release;
        assert!(key_event_to_binding(&event).is_none());
    }

    #[test]
    fn roundtrip_parse_and_match() {
        // Parse a binding, create a matching KeyEvent, and verify it matches
        let binding = HotkeyBinding::from_str("ctrl+g");
        let (code, mods) = parse_binding(&binding).unwrap();
        let event = press(code, mods);
        assert!(matches_binding(&binding, &event));

        // Also verify the reverse: key_event_to_binding produces the same binding
        let roundtripped = key_event_to_binding(&event).unwrap();
        assert_eq!(roundtripped, binding);
    }
}
