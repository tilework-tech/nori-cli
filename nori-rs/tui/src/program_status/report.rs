//! Encodes and writes OSC 7501 reports.
//!
//! A report is `ESC ] 7501 ; state=<state>:app=nori[:kind=permission:msg=<b64>] ESC \`.
//! Values in the body are limited to `[A-Za-z0-9_.,+/=-]`, so free text goes
//! in `msg` as standard base64 of single-line UTF-8. Every report replaces the
//! whole record, and `state=clear` removes it.

use std::io;
use std::io::IsTerminal;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use base64::Engine;

use super::ProgramState;
use super::ProgramStatus;

/// Message for a blocked report unless approval titles are opted in.
const GENERIC_APPROVAL_MESSAGE: &str = "Approval needed";

/// Largest decoded `msg` the protocol accepts, in bytes.
const MAX_MESSAGE_BYTES: usize = 2048;

const CLEAR: &str = "\x1b]7501;state=clear\x1b\\";

/// Whether this process left a status on its terminal, so the panic hook can
/// clear it without reaching the reporter.
static TERMINAL_HOLDS_STATUS: AtomicBool = AtomicBool::new(false);

/// Clear a status this process left on its terminal; for the panic hook.
pub(crate) fn clear_after_panic() {
    if TERMINAL_HOLDS_STATUS.swap(false, Ordering::SeqCst) {
        let mut stdout = io::stdout();
        let _ = stdout.write_all(CLEAR.as_bytes());
        let _ = stdout.flush();
    }
}

/// Writes the foreground session's status to the terminal when it changes.
#[derive(Debug)]
pub(crate) struct ProgramStatusReporter {
    approval_titles: bool,
    /// The report the terminal holds, or `None` when it holds none.
    last: Option<String>,
}

impl ProgramStatusReporter {
    /// A reporter when `[tui] program_status` is on, otherwise `None`.
    pub(crate) fn from_config(config: &nori_config::NoriConfig) -> Option<Self> {
        config.program_status.then_some(Self {
            approval_titles: config.program_status_approval_titles,
            last: None,
        })
    }

    /// Write `status` to `out` unless the terminal already shows it.
    pub(crate) fn report(
        &mut self,
        out: &mut impl Write,
        status: &ProgramStatus,
    ) -> io::Result<()> {
        let report = self.encode(status);
        if self.last.as_deref() == Some(report.as_str()) {
            return Ok(());
        }
        out.write_all(report.as_bytes())?;
        out.flush()?;
        self.last = Some(report);
        Ok(())
    }

    /// Remove the status from the terminal if one was reported.
    pub(crate) fn clear(&mut self, out: &mut impl Write) -> io::Result<()> {
        if self.last.take().is_none() {
            return Ok(());
        }
        out.write_all(CLEAR.as_bytes())?;
        out.flush()
    }

    /// Report `status` on this process's terminal; skipped when stdout is
    /// not a terminal.
    pub(crate) fn report_to_terminal(&mut self, status: &ProgramStatus) -> io::Result<()> {
        let mut stdout = io::stdout();
        if !stdout.is_terminal() {
            return Ok(());
        }
        self.report(&mut stdout, status)?;
        TERMINAL_HOLDS_STATUS.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Clear the status on this process's terminal.
    pub(crate) fn clear_terminal(&mut self) -> io::Result<()> {
        let mut stdout = io::stdout();
        if !stdout.is_terminal() {
            return Ok(());
        }
        TERMINAL_HOLDS_STATUS.store(false, Ordering::SeqCst);
        self.clear(&mut stdout)
    }

    fn encode(&self, status: &ProgramStatus) -> String {
        let state = match status.state {
            ProgramState::Idle => "idle",
            ProgramState::Working => "working",
            ProgramState::Blocked => "blocked",
            ProgramState::Done => "done",
            ProgramState::Error => "error",
        };
        let mut body = format!("state={state}:app=nori");
        if status.state == ProgramState::Blocked {
            let message = match (&status.approval_title, self.approval_titles) {
                (Some(title), true) => single_line(title),
                (None, true) | (Some(_), false) | (None, false) => {
                    GENERIC_APPROVAL_MESSAGE.to_string()
                }
            };
            let message = base64::engine::general_purpose::STANDARD.encode(message);
            body.push_str(&format!(":kind=permission:msg={message}"));
        }
        format!("\x1b]7501;{body}\x1b\\")
    }
}

/// `text` as one line the protocol accepts: control and bidirectional
/// formatting characters become spaces, and the result fits the message
/// limit without splitting a character.
fn single_line(text: &str) -> String {
    let mut line = String::new();
    for ch in text.chars() {
        let bidi_control = matches!(
            ch,
            '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        );
        let ch = if ch.is_control() || bidi_control {
            ' '
        } else {
            ch
        };
        if line.len() + ch.len_utf8() > MAX_MESSAGE_BYTES {
            break;
        }
        line.push(ch);
    }
    line
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
