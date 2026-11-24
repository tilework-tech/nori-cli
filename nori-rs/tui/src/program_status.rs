//! Program status reports via OSC 7501.
//!
//! Tells the terminal whether Nori is idle, working, waiting on the user,
//! finished, or failed, so terminals that show many programs at once (tab
//! spinners, session pickers) can track agent sessions without parsing the
//! window title. Terminals that do not know the sequence ignore it.
//!
//! Spec: <https://www.superlogical.com/rex/docs/build/program-status>

use std::fmt;
use std::io;
use std::io::IsTerminal;
use std::io::stdout;

use base64::Engine;
use crossterm::Command;
use crossterm::execute;

/// Stable identifier terminals use to group and label Nori's reports.
const APP_ID: &str = "nori";

/// What the foreground session is doing, as one OSC 7501 root record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProgramStatus {
    /// At the prompt, waiting for the next instruction.
    Idle,
    /// A turn is running.
    Working,
    /// The agent waits for the user to approve something.
    Blocked { message: String },
    /// A turn finished and its result is ready to read.
    Done,
    /// A turn failed and stopped.
    Error,
}

/// Write `status` as an OSC 7501 report. Skipped when stdout is not a terminal.
pub(crate) fn write_program_status(status: &ProgramStatus) -> io::Result<()> {
    if !stdout().is_terminal() {
        return Ok(());
    }
    execute!(stdout(), ProgramStatusReport(encode_report(status)))
}

/// Remove every record Nori reported on this terminal.
pub(crate) fn clear_program_status() -> io::Result<()> {
    if !stdout().is_terminal() {
        return Ok(());
    }
    execute!(stdout(), ProgramStatusReport("state=clear".to_string()))
}

/// Build the `key=value` body of a report. Free text is sanitized and base64
/// encoded because the spec refuses control characters and `:` in values.
fn encode_report(status: &ProgramStatus) -> String {
    let state = match status {
        ProgramStatus::Idle => "idle",
        ProgramStatus::Working => "working",
        ProgramStatus::Blocked { .. } => "blocked",
        ProgramStatus::Done => "done",
        ProgramStatus::Error => "error",
    };
    let mut report = format!("state={state}:app={APP_ID}");
    if let ProgramStatus::Blocked { message } = status {
        report.push_str(":kind=permission");
        let message = crate::terminal_title::sanitize_terminal_title(message);
        if !message.is_empty() {
            let msg = base64::engine::general_purpose::STANDARD.encode(message);
            report.push_str(&format!(":msg={msg}"));
        }
    }
    report
}

#[derive(Debug, Clone)]
struct ProgramStatusReport(String);

impl Command for ProgramStatusReport {
    fn write_ansi(&self, f: &mut impl fmt::Write) -> fmt::Result {
        write!(f, "\x1b]7501;{}\x1b\\", self.0)
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> Result<()> {
        Err(io::Error::other(
            "ProgramStatusReport: use ANSI, not WinAPI",
        ))
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn decode_msg(report: &str) -> String {
        let msg = report
            .split(':')
            .find_map(|pair| pair.strip_prefix("msg="))
            .expect("report has a msg");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(msg)
            .expect("msg is base64");
        String::from_utf8(bytes).expect("msg is UTF-8")
    }

    #[test]
    fn plain_states_name_the_app() {
        assert_eq!(encode_report(&ProgramStatus::Idle), "state=idle:app=nori");
        assert_eq!(
            encode_report(&ProgramStatus::Working),
            "state=working:app=nori"
        );
        assert_eq!(encode_report(&ProgramStatus::Done), "state=done:app=nori");
        assert_eq!(encode_report(&ProgramStatus::Error), "state=error:app=nori");
    }

    #[test]
    fn blocked_is_a_permission_request_with_its_title() {
        let report = encode_report(&ProgramStatus::Blocked {
            message: "Run cargo test".to_string(),
        });

        assert!(report.starts_with("state=blocked:app=nori:kind=permission:msg="));
        assert_eq!(decode_msg(&report), "Run cargo test");
    }

    #[test]
    fn blocked_message_drops_control_characters() {
        let report = encode_report(&ProgramStatus::Blocked {
            message: "rm -rf build\n\x1b[31mtarget\t".to_string(),
        });

        assert_eq!(decode_msg(&report), "rm -rf build [31mtarget");
    }

    #[test]
    fn blocked_without_text_leaves_msg_out() {
        let report = encode_report(&ProgramStatus::Blocked {
            message: " \n ".to_string(),
        });

        assert_eq!(report, "state=blocked:app=nori:kind=permission");
    }

    #[test]
    fn report_values_stay_in_the_spec_character_set() {
        let report = encode_report(&ProgramStatus::Blocked {
            message: "Edit src/main.rs: add a; fix".to_string(),
        });

        for pair in report.split(':') {
            let (_, value) = pair.split_once('=').expect("pair has =");
            assert!(
                value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "_.,+/=-".contains(c)),
                "value {value:?} has a character outside the spec set"
            );
        }
    }

    #[test]
    fn command_wraps_the_body_in_osc_7501() {
        let mut out = String::new();
        ProgramStatusReport("state=idle:app=nori".to_string())
            .write_ansi(&mut out)
            .expect("write");

        assert_eq!(out, "\x1b]7501;state=idle:app=nori\x1b\\");
    }
}
