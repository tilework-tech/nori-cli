//! Reading a finished screen in the PTY end-to-end tests.
//!
//! The screen only changes inside [`TuiSession::poll`], so a
//! `std::thread::sleep` before reading it does not let the TUI catch up: the
//! test reads whatever the emulator held when the last wait matched, which can
//! be a frame the TUI had not finished drawing. Read the screen for a snapshot
//! or an assertion with [`TuiSession::settled_screen`] instead, or keep
//! reading for a fixed time with [`TuiSession::settle`] when a test checks
//! that nothing happens. The `raw_sleeps_do_not_precede_screen_reads` test
//! below rejects a sleep directly before a screen read anywhere in `tests/`.

use crate::TIMEOUT;
use crate::TuiSession;
use std::time::Duration;
use std::time::Instant;

/// How long the screen must stay unchanged to count as settled.
pub const QUIET_WINDOW: Duration = Duration::from_millis(300);

impl TuiSession {
    /// Keep reading output until the screen has not changed for
    /// [`QUIET_WINDOW`] and no synchronized frame is open, then return it.
    ///
    /// Panics if the screen is still changing after [`TIMEOUT`].
    pub fn settled_screen(&mut self) -> String {
        let deadline = Instant::now() + TIMEOUT;
        let mut screen = self.screen_contents();
        let mut quiet_since = Instant::now();
        loop {
            // Once the TUI exits, reads fail and the screen is final.
            let _ = self.poll();
            let current = self.screen_contents();
            if current != screen || self.emulator.is_frame_open() {
                screen = current;
                quiet_since = Instant::now();
            } else if quiet_since.elapsed() >= QUIET_WINDOW {
                return screen;
            }
            assert!(
                Instant::now() < deadline,
                "the screen did not settle within {TIMEOUT:?}; last screen:\n{screen}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[cfg(test)]
mod tests {
    /// Screen reads in a whitespace-free test source.
    const SCREEN_READS: [&str; 3] = [
        "screen_contents()",
        "history_contents()",
        "assert_snapshot!(",
    ];

    /// Whether `source` sleeps in the statement directly before one that reads
    /// the screen or takes a snapshot, in the same block.
    fn sleeps_before_screen_read(source: &str) -> bool {
        let code: String = source
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .flat_map(str::split_whitespace)
            .collect();
        let statements: Vec<&str> = code.split(';').collect();
        statements.windows(2).any(|pair| {
            let last_in_sleep_statement = pair[0].rsplit(['{', '}']).next().unwrap_or_default();
            let read_at = SCREEN_READS
                .iter()
                .filter_map(|read| pair[1].find(read))
                .min();
            last_in_sleep_statement.contains("thread::sleep(")
                && read_at.is_some_and(|at| !pair[1][..at].contains('}'))
        })
    }

    #[test]
    fn a_sleep_then_a_screen_read_is_rejected() {
        assert!(sleeps_before_screen_read(
            "session.wait_for_text(\"x\", TIMEOUT).unwrap();\nstd::thread::sleep(Duration::from_millis(500));\nlet screen = session.screen_contents();\n"
        ));
        assert!(sleeps_before_screen_read(
            "std::thread::sleep(PAUSE);\n// comment\ninsta::assert_snapshot!(\"name\", contents);\n"
        ));
    }

    #[test]
    fn a_sleep_between_inputs_or_at_the_end_of_a_block_is_allowed() {
        assert!(!sleeps_before_screen_read(
            "std::thread::sleep(TIMEOUT_INPUT);\nsession.send_key(Key::Enter).unwrap();\nlet screen = session.screen_contents();\n"
        ));
        assert!(!sleeps_before_screen_read(
            "fn a() {\n    std::thread::sleep(TIMEOUT_INPUT);\n}\nfn b(s: &mut TuiSession) {\n    let x = s.screen_contents();\n}\n"
        ));
        assert!(!sleeps_before_screen_read(
            "session.settle(Duration::from_secs(1));\nlet screen = session.screen_contents();\n"
        ));
    }

    /// A sleep reads nothing from the PTY; see the module docs.
    #[test]
    fn raw_sleeps_do_not_precede_screen_reads() {
        let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let mut offenders = Vec::new();
        let mut dirs = vec![tests_dir];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("read tests dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let source = std::fs::read_to_string(&path).expect("read test source");
                    if sleeps_before_screen_read(&source) || source.contains("TIMEOUT_PRESNAPSHOT")
                    {
                        offenders.push(path.display().to_string());
                    }
                }
            }
        }
        offenders.sort();
        assert!(
            offenders.is_empty(),
            "read the screen with settled_screen() (or settle() for a fixed wait), not after a raw sleep: {offenders:?}"
        );
    }
}
