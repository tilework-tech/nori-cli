/// Key input types
pub enum Key {
    Enter,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Backspace,
    Tab,
    Ctrl(char),
    Char(char),
    /// A printable key pressed with Alt, sent as ESC followed by the key.
    Alt(char),
    /// Down arrow pressed with Alt.
    AltDown,
    /// Shift+Tab.
    BackTab,
    /// The terminal window gained focus.
    FocusIn,
    /// The terminal window lost focus.
    FocusOut,
}

impl Key {
    pub fn to_escape_sequence(&self) -> Vec<u8> {
        match self {
            Key::Enter => vec![b'\r'],
            Key::Escape => vec![0x1b],
            Key::Up => vec![0x1b, b'[', b'A'],
            Key::Down => vec![0x1b, b'[', b'B'],
            Key::Right => vec![0x1b, b'[', b'C'],
            Key::Left => vec![0x1b, b'[', b'D'],
            Key::Backspace => vec![0x7f],
            Key::Tab => vec![b'\t'],
            Key::Ctrl(c) => vec![(*c as u8) & 0x1f],
            Key::Char(c) => c.to_string().into_bytes(),
            Key::Alt(c) => {
                let mut bytes = vec![0x1b];
                bytes.extend(c.to_string().into_bytes());
                bytes
            }
            Key::AltDown => b"\x1b[1;3B".to_vec(),
            Key::BackTab => b"\x1b[Z".to_vec(),
            Key::FocusIn => b"\x1b[I".to_vec(),
            Key::FocusOut => b"\x1b[O".to_vec(),
        }
    }
}
