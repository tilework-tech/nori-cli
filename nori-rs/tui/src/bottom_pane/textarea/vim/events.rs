use super::VimMotion;
use super::VimTextObject;
use super::VimTextObjectScope;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

pub(super) fn plain_char(event: KeyEvent, ch: char) -> bool {
    event.code == KeyCode::Char(ch) && event.modifiers == KeyModifiers::NONE
}

pub(super) fn text_object_scope_for_event(event: KeyEvent) -> Option<VimTextObjectScope> {
    if plain_char(event, 'i') {
        Some(VimTextObjectScope::Inner)
    } else if plain_char(event, 'a') {
        Some(VimTextObjectScope::Around)
    } else {
        None
    }
}

pub(super) fn text_object_for_event(event: KeyEvent) -> Option<VimTextObject> {
    match event.code {
        KeyCode::Char('w') if event.modifiers == KeyModifiers::NONE => Some(VimTextObject::Word),
        KeyCode::Char('W') if event.modifiers.intersects(KeyModifiers::SHIFT) => {
            Some(VimTextObject::BigWord)
        }
        KeyCode::Char('(' | ')') => Some(VimTextObject::Parentheses),
        KeyCode::Char('[' | ']') => Some(VimTextObject::Brackets),
        KeyCode::Char('{' | '}') => Some(VimTextObject::Braces),
        KeyCode::Char('"') => Some(VimTextObject::DoubleQuote),
        KeyCode::Char('\'') => Some(VimTextObject::SingleQuote),
        KeyCode::Char('`') => Some(VimTextObject::Backtick),
        _ => None,
    }
}

pub(super) fn motion_for_event(event: KeyEvent) -> Option<VimMotion> {
    match event {
        KeyEvent {
            code: KeyCode::Char('h') | KeyCode::Left,
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(VimMotion::Left),
        KeyEvent {
            code: KeyCode::Char('l') | KeyCode::Right,
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(VimMotion::Right),
        KeyEvent {
            code: KeyCode::Char('j') | KeyCode::Down,
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(VimMotion::Down),
        KeyEvent {
            code: KeyCode::Char('k') | KeyCode::Up,
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(VimMotion::Up),
        KeyEvent {
            code: KeyCode::Char('w'),
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(VimMotion::WordForward),
        KeyEvent {
            code: KeyCode::Char('b'),
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(VimMotion::WordBackward),
        KeyEvent {
            code: KeyCode::Char('e'),
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(VimMotion::WordEnd),
        KeyEvent {
            code: KeyCode::Char('0'),
            modifiers: KeyModifiers::NONE,
            ..
        } => Some(VimMotion::LineStart),
        KeyEvent {
            code: KeyCode::Char('$'),
            ..
        } => Some(VimMotion::LineEnd),
        _ => None,
    }
}
