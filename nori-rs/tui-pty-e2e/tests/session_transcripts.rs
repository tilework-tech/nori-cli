//! Each session keeps its own transcript.
//!
//! Resuming a session that is already on screen must show its turns once, in
//! the terminal and in its scrollback, and a resize afterwards must reprint
//! only the resumed session.

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

const PROMPT: &str = "remember the walrus";

fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

#[test]
#[cfg(target_os = "linux")]
fn resume_after_new_shows_each_turn_once() {
    let config = SessionConfig::new().with_mock_response("The walrus is remembered");
    let mut session = TuiSession::spawn_with_config(30, 100, config).expect("spawn");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    session.submit_input(PROMPT).unwrap();
    session
        .wait_for_text("The walrus is remembered", TIMEOUT)
        .expect("first turn should finish");
    std::thread::sleep(Duration::from_millis(500));

    session.submit_input("/new").unwrap();
    session
        .wait_for_text("Mock Default Model", TIMEOUT)
        .expect("a new session should start");
    std::thread::sleep(Duration::from_millis(500));

    session.submit_input("/resume").unwrap();
    session
        .wait_for(|screen| screen.contains(PROMPT), Duration::from_secs(5))
        .expect("the resume picker should list the first session");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        session.poll().ok();
        let history = session.history_contents();
        if occurrences(&history, "The walrus is remembered") >= 1
            && !session.screen_contents().contains("Resume")
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "resumed session should replay its turn:\n{history}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    session.settle(Duration::from_secs(1));

    let history = session.history_contents();
    assert_eq!(
        occurrences(&history, "The walrus is remembered"),
        1,
        "the resumed turn should appear once:\n{history}"
    );

    session.resize(30, 80).unwrap();
    session.settle(Duration::from_secs(1));
    let history = session.history_contents();
    assert_eq!(
        occurrences(&history, "The walrus is remembered"),
        1,
        "a resize should reprint the resumed turn once:\n{history}"
    );
}
