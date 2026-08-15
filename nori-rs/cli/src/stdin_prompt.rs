//! Piped-stdin prompt ingestion shared by the interactive and `exec` entry points.
//!
//! A prompt can arrive as a command-line argument, on piped stdin, or as both.
//! When both are present the argument is the instruction and the piped bytes are
//! the context it operates on, so `git diff | nori "review this"` reads the way
//! it looks.

use anyhow::Context;
use std::io::IsTerminal;
use std::io::Read;
use std::io::Write;

/// Prompt argument that means "the prompt is on stdin", matching the `-`
/// convention the upstream Codex CLI uses for the same purpose.
pub const STDIN_SENTINEL: &str = "-";

/// Upper bound on a piped prompt. Nothing legitimate approaches this, and
/// without it an unbounded producer (`yes | nori`) grows a `String` until the
/// process is killed. `u64` because that is what [`Read::take`] takes.
const MAX_PIPED_BYTES: u64 = 10 * 1024 * 1024;

/// Resolves the prompt from the argument and, only when asked, from piped stdin.
///
/// Stdin is consumed in exactly three cases: there is no prompt argument at all,
/// the argument is the `-` sentinel, or the caller passed `--stdin`. A plain
/// prompt argument never touches stdin, so `nori exec "..."` cannot swallow a
/// pipe it merely inherited from a parent (a `while read` loop, a git hook, a
/// `curl | bash` script).
///
/// Callers must not invoke this when stdin is reserved for another protocol
/// (notably `nori exec --acp`, which speaks JSON-RPC over stdin).
pub fn resolve_prompt(
    argument: Option<String>,
    stdin_requested: bool,
) -> anyhow::Result<Option<String>> {
    let is_sentinel = argument.as_deref() == Some(STDIN_SENTINEL);
    let instruction = if is_sentinel { None } else { argument };
    let wants_stdin = is_sentinel || stdin_requested || instruction.is_none();
    let piped = if wants_stdin {
        read_piped_stdin()?
    } else {
        None
    };
    Ok(compose_prompt(instruction, piped))
}

/// Reads stdin to EOF when it is a pipe or a redirect, capped at
/// [`MAX_PIPED_BYTES`].
///
/// Returns `None` when stdin is a terminal, meaning nothing was piped in.
fn read_piped_stdin() -> anyhow::Result<Option<String>> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;

        let file_type = std::fs::metadata("/dev/stdin").map(|metadata| metadata.file_type());
        let diagnostic = format!(
            "[DEBUG-a4f2] stdin is not a terminal: {:?}\n",
            file_type.map(|file_type| (
                file_type.is_char_device(),
                file_type.is_fifo(),
                file_type.is_file(),
                file_type.is_socket(),
            ))
        );
        if let Ok(nori_home) = std::env::var("NORI_HOME") {
            let _ = std::fs::write(
                std::path::Path::new(&nori_home).join("stdin-debug.log"),
                diagnostic,
            );
        }
    }
    let mut piped = String::new();
    let read = stdin
        .lock()
        .take(MAX_PIPED_BYTES)
        .read_to_string(&mut piped)
        .context("failed to read the prompt from stdin")?;
    if read as u64 == MAX_PIPED_BYTES {
        anyhow::bail!("piped prompt exceeded the {MAX_PIPED_BYTES} byte limit");
    }
    Ok(Some(piped))
}

/// Re-points file descriptor 0 at the controlling terminal after a pipe has
/// been drained, for the interactive path only.
///
/// Everything the TUI spawns with inherited stdin -- `$EDITOR`, the file
/// browser -- would otherwise get an EOF'd pipe and exit immediately with
/// "input is not from a terminal". Fixing the descriptor once here covers every
/// such child instead of patching each spawn site.
///
/// Best effort: if there is no controlling terminal, `tui::init` reports that
/// with a clearer message than this could.
#[cfg(unix)]
pub fn restore_stdin_from_terminal() {
    use std::os::fd::AsRawFd;

    let log_stage = |stage: &str| {
        if let Ok(nori_home) = std::env::var("NORI_HOME") {
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(std::path::Path::new(&nori_home).join("stdin-debug.log"))
                .and_then(|mut file| writeln!(file, "[DEBUG-a4f2] {stage}"));
        }
    };
    log_stage("restore stdin entered");
    let Ok(tty) = std::fs::File::open("/dev/tty") else {
        log_stage("open /dev/tty failed");
        return;
    };
    log_stage("open /dev/tty succeeded");
    // Safety: both descriptors are valid for the duration of the call, and
    // stdin is not borrowed elsewhere this early in startup.
    unsafe {
        let result = libc::dup2(tty.as_raw_fd(), libc::STDIN_FILENO);
        log_stage(&format!("dup2 returned {result}"));
    }
}

/// Windows consoles resolve their input handle through `CONIN$` rather than the
/// process's stdin handle, so a drained pipe on stdin does not need repointing.
#[cfg(windows)]
pub fn restore_stdin_from_terminal() {}

/// Combines an argument prompt with piped stdin into the single prompt slot.
///
/// Returns `None` when neither source carried anything but whitespace, which
/// callers translate into either "start an empty interactive session" or "a
/// prompt is required" depending on the mode.
fn compose_prompt(argument: Option<String>, piped: Option<String>) -> Option<String> {
    let instruction = argument.as_deref().and_then(normalize);
    let context = piped.as_deref().and_then(normalize);
    match (instruction, context) {
        (Some(instruction), Some(context)) => Some(format!("{instruction}\n\n{context}")),
        (Some(instruction), None) => Some(instruction),
        (None, Some(context)) => Some(context),
        (None, None) => None,
    }
}

/// Normalizes line endings and trims surrounding whitespace, collapsing a blank
/// input to `None` so the two prompt sources compose without stray separators.
fn normalize(raw: &str) -> Option<String> {
    let normalized = raw.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn argument_leads_and_piped_stdin_follows_as_context() {
        assert_eq!(
            compose_prompt(
                Some("review this".to_string()),
                Some("diff --git a/x b/x\n".to_string())
            ),
            Some("review this\n\ndiff --git a/x b/x".to_string())
        );
    }

    /// A blank pipe must not leave a dangling separator on the argument, which
    /// is what `nori -p "hi" < /dev/null` produces.
    #[test]
    fn a_blank_pipe_leaves_the_argument_untouched() {
        assert_eq!(
            compose_prompt(Some("just this".to_string()), Some("\n".to_string())),
            Some("just this".to_string())
        );
    }

    #[test]
    fn carriage_returns_are_normalized() {
        assert_eq!(
            compose_prompt(None, Some("first\r\nsecond\rthird".to_string())),
            Some("first\nsecond\nthird".to_string())
        );
    }
}
