//! Reading an API key without showing it: at a terminal only the key's known
//! public prefix is echoed and every other character is drawn as `*`; piped
//! input is read silently.

use std::io::IsTerminal;
use std::io::Read;
use std::io::Write;

use crossterm::cursor::MoveToColumn;
use crossterm::event::DisableBracketedPaste;
use crossterm::event::EnableBracketedPaste;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::terminal::Clear;
use crossterm::terminal::ClearType;

/// Read the API key for `provider`, from the terminal when there is one.
pub(super) fn read_key(provider: &str) -> anyhow::Result<String> {
    let mut stdin = std::io::stdin();
    if !stdin.is_terminal() {
        let mut key = String::new();
        stdin.read_to_string(&mut key)?;
        return Ok(key);
    }
    // Echo is switched off before the prompt appears, so nothing typed after
    // it is ever drawn by the terminal itself.
    let _raw = RawInput::enable()?;
    let mut out = std::io::stderr();
    write!(
        out,
        "Type or paste the {provider} API key, then press Enter (Esc cancels).\r\n"
    )?;
    let mut key = String::new();
    loop {
        crossterm::queue!(out, MoveToColumn(0), Clear(ClearType::CurrentLine))?;
        write!(out, "Key: {}", nori_providers::masked_key(&key))?;
        out.flush()?;
        match crossterm::event::read()? {
            Event::Paste(text) => key.extend(text.chars().filter(|c| !c.is_control())),
            Event::Key(event) if event.kind != KeyEventKind::Release => {
                let control = event.modifiers.contains(KeyModifiers::CONTROL);
                match event.code {
                    KeyCode::Enter => break,
                    KeyCode::Char('d') if control => break,
                    KeyCode::Char('c') if control => anyhow::bail!("cancelled"),
                    KeyCode::Char('u') if control => key.clear(),
                    KeyCode::Esc => anyhow::bail!("cancelled"),
                    KeyCode::Backspace => {
                        key.pop();
                    }
                    KeyCode::Char(c) if !control => key.push(c),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    write!(out, "\r\n")?;
    Ok(key)
}

/// Raw mode with bracketed paste, restored when dropped (including on error).
struct RawInput;

impl RawInput {
    fn enable() -> anyhow::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        let guard = RawInput;
        crossterm::execute!(std::io::stderr(), EnableBracketedPaste)?;
        Ok(guard)
    }
}

impl Drop for RawInput {
    fn drop(&mut self) {
        let _ = crossterm::execute!(std::io::stderr(), DisableBracketedPaste);
        let _ = crossterm::terminal::disable_raw_mode();
    }
}
