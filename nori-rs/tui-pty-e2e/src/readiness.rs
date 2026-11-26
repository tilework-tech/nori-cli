//! Startup readiness waits for the PTY end-to-end tests.
//!
//! `〉` alone is not a readiness signal: the welcome header (`〉nori CLI`) is
//! drawn with it before the composer exists and long before the first session
//! has started. A test that types once it sees `〉` races agent startup, and
//! under load its input lands in the startup placeholder instead of a session.
//! Wait with [`TuiSession::wait_until_ready`] instead, or with
//! [`TuiSession::wait_for_composer`] when the test means to type while the
//! agent is still starting. The `bare_prompt_waits_are_not_readiness` test
//! below rejects `wait_for_text("〉", ..)` anywhere in `tests/`.

use crate::TIMEOUT;
use crate::TuiSession;

/// Whether `screen` shows the composer's prompt row.
///
/// The composer prompt is `〉` in the first column, followed by the input or a
/// placeholder; the welcome header draws its `〉` indented. A list's focused
/// row shows `›` after its `▏` rail, and while a list is open it takes the
/// composer's place.
pub fn has_composer_row(screen: &str) -> bool {
    screen.lines().any(|line| line.starts_with('〉')) && !screen.contains("▏›")
}

/// Whether `screen` shows that a session has started.
///
/// A local session announces its options in its own history
/// (`• Mock ACP options: Model=…`). A Nori Cloud session adopts them silently
/// and instead extends the header's agent row with the resolved model
/// (`Agent        Nori Cloud · Mock Default Model · Medium`). Both appear only
/// once the session is the one on screen.
fn has_started_session(screen: &str) -> bool {
    screen.lines().any(|line| {
        (line.starts_with("• ") && line.contains(" options: "))
            || (line.trim_start().starts_with("Agent ") && line.contains(" · "))
    })
}

impl TuiSession {
    /// Wait until the composer accepts input, whether or not a session has
    /// started.
    ///
    /// Use this only when a test means to interact while the agent is still
    /// starting, such as typing into the startup placeholder or opening a
    /// picker during preparation. Everything else waits with
    /// [`TuiSession::wait_until_ready`].
    pub fn wait_for_composer(&mut self) -> Result<(), String> {
        self.wait_for(has_composer_row, TIMEOUT)
            .map_err(|error| format!("waiting for the composer prompt row: {error}"))
    }

    /// Wait until the composer accepts input and the first session has
    /// started, so prompts and commands reach a live session.
    ///
    /// A started session is recognised by its options banner or, in Nori
    /// Cloud, by the resolved model on the header's agent row.
    pub fn wait_until_ready(&mut self) -> Result<(), String> {
        self.wait_for(
            |screen| has_composer_row(screen) && has_started_session(screen),
            TIMEOUT,
        )
        .map_err(|error| format!("waiting for the composer and a started session: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::has_composer_row;
    use super::has_started_session;

    const STARTING: &str = "  〉nori CLI v0.0.0\n\n  System       /tmp/x\n  Agent        Mock ACP\n\n  Preparing Mock ACP…\n\n〉? for shortcuts\n";
    const STARTED: &str = "  〉nori CLI v0.0.0\n\n  Agent        Mock ACP\n\n• Mock ACP options: Model=Mock Default Model, Thought Level=Medium (/config\nto change)\n\n〉/ for slash command menu\n";

    #[test]
    fn the_welcome_header_is_not_a_composer_row() {
        assert!(!has_composer_row(
            "  〉nori CLI v0.0.0\n\n  Agent        Mock ACP\n"
        ));
    }

    #[test]
    fn an_open_list_hides_the_composer() {
        assert!(!has_composer_row(
            "  Select agent\n\n  ▏› 1. Mock ACP  ▕\n     2. Claude\n\n〉? for shortcuts\n"
        ));
    }

    #[test]
    fn the_composer_shows_before_a_session_starts() {
        assert!(has_composer_row(STARTING));
        assert!(!has_started_session(STARTING));
    }

    #[test]
    fn a_connected_cloud_session_shows_its_model_in_the_header() {
        assert!(!has_started_session(
            "  Agent        Nori Cloud\n\n〉? for shortcuts\n"
        ));
        assert!(has_started_session(
            "  Agent        Nori Cloud · Mock Default Model · Medium\n\n〉? for shortcuts\n"
        ));
    }

    #[test]
    fn a_started_session_shows_its_options_banner() {
        assert!(has_composer_row(STARTED));
        assert!(has_started_session(STARTED));
    }

    /// Readiness waits on a bare `〉` race agent startup; see the module docs.
    #[test]
    fn bare_prompt_waits_are_not_readiness() {
        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let pattern = concat!("wait_for_text(", "\"〉\"");
        let mut offenders = Vec::new();
        let mut dirs = vec![tests_dir];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("read tests dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let source = std::fs::read_to_string(&path).expect("read test source");
                    // rustfmt may break the call across lines.
                    let compact: String = source.split_whitespace().collect();
                    if compact.contains(pattern) {
                        offenders.push(path.display().to_string());
                    }
                }
            }
        }
        offenders.sort();
        assert!(
            offenders.is_empty(),
            "use wait_until_ready() or wait_for_composer() instead of waiting for a bare \"〉\": {offenders:?}"
        );
    }
}
