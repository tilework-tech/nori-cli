//! Piped-stdin prompt ingestion shared by the interactive and `exec` entry points.
//!
//! A prompt can arrive as a command-line argument, on piped stdin, or as both.
//! When both are present the argument is the instruction and the piped bytes are
//! the context it operates on, so `git diff | nori "review this"` reads the way
//! it looks.

use std::io::IsTerminal;
use std::io::Read;

/// Reads stdin to EOF when it is a pipe or a redirect.
///
/// Returns `None` when stdin is a terminal, meaning the caller was invoked
/// normally and nothing was piped in. The raw text is returned as-is;
/// [`compose_prompt`] is responsible for normalizing it and discarding pipes
/// that carried only whitespace.
///
/// Callers must not invoke this when stdin is reserved for another protocol
/// (notably `nori exec --acp`, which speaks JSON-RPC over stdin).
pub fn read_piped_stdin() -> std::io::Result<Option<String>> {
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }
    let mut piped = String::new();
    stdin.read_to_string(&mut piped)?;
    Ok(Some(piped))
}

/// Combines an argument prompt with piped stdin into the single prompt slot.
///
/// Returns `None` when neither source carried anything but whitespace, which
/// callers translate into either "start an empty interactive session" or "a
/// prompt is required" depending on the mode.
pub fn compose_prompt(argument: Option<String>, piped: Option<String>) -> Option<String> {
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
