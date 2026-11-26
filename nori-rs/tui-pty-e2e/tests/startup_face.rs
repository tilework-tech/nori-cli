#![cfg(unix)]

//! Jiro's face on the welcome header while the default session starts: it
//! holds open-eyed until the session is actually ready, then winks and
//! retypes into the versioned wordmark, without ever holding up the composer.

use std::time::Duration;

use pretty_assertions::assert_eq;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_snapshot;

const HOLDING: &str = "〉_ ▐";
const WINK: &str = "〉_ -";
const WORDMARK: &str = "〉nori CLI v";
const CONFIG_ROW: &str = "Mock ACP options";
const AGENT_ROW: &str = "Agent        Mock ACP";

fn composer(screen: &str) -> String {
    screen
        .lines()
        .rev()
        .find(|line| line.starts_with('〉'))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// `output` with its escape sequences removed, so text drawn in one write
/// reads as one run.
fn without_escapes(output: &str) -> String {
    let mut text = String::new();
    let mut chars = output.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            text.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    text
}

fn count(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

fn gated(release: &std::path::Path) -> SessionConfig {
    SessionConfig::new().with_agent_env(
        "MOCK_AGENT_NEW_SESSION_RELEASE_FILE",
        release.to_str().unwrap(),
    )
}

#[test]
fn a_slow_session_holds_the_open_face_while_typing_then_winks_and_retypes_once_ready() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    let mut session = TuiSession::spawn_with_config(24, 80, gated(&release)).unwrap();

    session.wait_for_composer().unwrap();
    session.type_input("draft before ready").unwrap();
    session
        .wait_for_text("〉draft before ready", TIMEOUT)
        .unwrap();
    session
        .wait_for_text(HOLDING, Duration::from_secs(15))
        .unwrap();
    // Up to three blinks are over within four seconds; the face then holds
    // still.
    session.settle(Duration::from_secs(5));
    let holding = session.screen_contents();
    assert!(holding.contains(HOLDING), "{holding}");
    assert!(
        !holding.contains(WINK),
        "no wink before readiness:\n{holding}"
    );
    assert!(!holding.contains(WORDMARK), "not ready yet:\n{holding}");
    assert!(!holding.contains(CONFIG_ROW), "{holding}");

    std::fs::write(&release, "go").unwrap();
    session.wait_for_text(CONFIG_ROW, TIMEOUT).unwrap();
    session.wait_for_text(WORDMARK, TIMEOUT).unwrap();
    session.settle(Duration::from_millis(500));

    let history = session.history_contents();
    assert_eq!(count(&history, "〉nori CLI"), 1, "{history}");
    assert_eq!(count(&history, CONFIG_ROW), 1, "{history}");
    assert_eq!(count(&history, AGENT_ROW), 1, "{history}");
    assert!(!history.contains(HOLDING), "{history}");
    let screen = session.screen_contents();
    assert_eq!(composer(&screen), "〉draft before ready");
    insta::assert_snapshot!(
        "startup_face_after_slow_readiness",
        normalize_for_snapshot(screen)
    );
}

#[test]
fn the_config_row_is_drawn_before_the_retype_finishes() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    let mut session = TuiSession::spawn_with_config(24, 80, gated(&release)).unwrap();
    session
        .wait_for_text(HOLDING, Duration::from_secs(15))
        .unwrap();

    std::fs::write(&release, "go").unwrap();
    session.wait_for_text(WORDMARK, TIMEOUT).unwrap();

    // Order in the terminal output, not timing: the row is written before
    // the retype types its first letter.
    let text = without_escapes(&String::from_utf8_lossy(session.raw_output()));
    let config = text.find("options:").expect("config row drawn");
    let first_letter = text.find("n_ ▐").expect("retype drawn");
    assert!(
        config < first_letter,
        "the config row waited for the retype"
    );
}

#[test]
fn a_prompt_sent_before_readiness_goes_out_once_the_session_is_ready() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    let mut session = TuiSession::spawn_with_config(
        30,
        80,
        gated(&release).with_mock_response("synthetic answer from the gulls"),
    )
    .unwrap();
    session.wait_for_composer().unwrap();
    session.submit_input("count the gulls").unwrap();
    session
        .wait_for_text(HOLDING, Duration::from_secs(15))
        .unwrap();

    std::fs::write(&release, "go").unwrap();
    session
        .wait_for_text("synthetic answer from the gulls", TIMEOUT)
        .unwrap();
    session.settle(Duration::from_millis(2_000));

    let history = session.history_contents();
    assert_eq!(count(&history, "〉nori CLI"), 1, "{history}");
    assert_eq!(count(&history, "count the gulls"), 1, "{history}");
    let header = history.find("〉nori CLI").unwrap();
    let prompt = history.find("count the gulls").unwrap();
    assert!(
        header < prompt,
        "the header stays above the prompt:\n{history}"
    );
}

#[test]
fn peeking_at_the_transcript_or_resizing_while_waiting_keeps_the_face() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    let mut session = TuiSession::spawn_with_config(24, 80, gated(&release)).unwrap();
    session
        .wait_for_text(HOLDING, Duration::from_secs(15))
        .unwrap();
    session.send_key(tui_pty_e2e::Key::Ctrl('t')).unwrap();
    session
        .wait_for_text("T R A N S C R I P T", TIMEOUT)
        .unwrap();
    session.send_key(tui_pty_e2e::Key::Char('q')).unwrap();
    session.settle(Duration::from_millis(400));
    session.resize(26, 72).unwrap();
    session.settle(Duration::from_millis(400));

    std::fs::write(&release, "go").unwrap();
    session.wait_for_text(WORDMARK, TIMEOUT).unwrap();
    session.settle(Duration::from_millis(500));

    let history = session.history_contents();
    assert_eq!(count(&history, "〉nori CLI"), 1, "{history}");
    assert_eq!(count(&history, AGENT_ROW), 1, "{history}");
}

#[test]
fn a_fast_session_ends_on_one_versioned_wordmark() {
    let mut session = TuiSession::spawn_with_config(24, 80, SessionConfig::new()).unwrap();

    session.wait_until_ready().unwrap();
    session.wait_for_text(WORDMARK, TIMEOUT).unwrap();
    session.settle(Duration::from_millis(500));

    let history = session.history_contents();
    assert_eq!(count(&history, "〉nori CLI"), 1, "{history}");
    assert_eq!(count(&history, CONFIG_ROW), 1, "{history}");
    assert!(!history.contains(HOLDING), "{history}");
}

#[test]
fn a_failed_initialize_keeps_the_open_face_and_a_usable_composer() {
    // Tall enough that the agent picker opened by the failure does not push
    // the header out of the emulator.
    let mut session = TuiSession::spawn_with_config(
        50,
        80,
        SessionConfig::new().with_agent_env("MOCK_AGENT_INITIALIZE_FAIL", "1"),
    )
    .unwrap();

    session.wait_for_composer().unwrap();
    session
        .wait_for_text("No session is active", Duration::from_secs(15))
        .unwrap();
    session.settle(Duration::from_secs(2));
    session.send_key(tui_pty_e2e::Key::Escape).unwrap();
    session.type_input("still typing").unwrap();
    session.wait_for_text("〉still typing", TIMEOUT).unwrap();

    let history = session.history_contents();
    assert!(history.contains(HOLDING), "{history}");
    assert!(!history.contains(WINK), "{history}");
    assert!(!history.contains("nori CLI"), "{history}");
    assert_eq!(count(&history, AGENT_ROW), 1, "{history}");
}

#[test]
fn a_failed_new_session_keeps_the_open_face_and_a_usable_composer() {
    let mut session = TuiSession::spawn_with_config(
        50,
        80,
        SessionConfig::new().with_agent_env("MOCK_AGENT_FAIL_NEW_SESSION_FROM", "0"),
    )
    .unwrap();

    session.wait_for_composer().unwrap();
    session
        .wait_for_text("Couldn't activate Mock ACP", Duration::from_secs(15))
        .unwrap();
    session.settle(Duration::from_secs(2));
    session.type_input("still typing").unwrap();
    session.wait_for_text("〉still typing", TIMEOUT).unwrap();

    let history = session.history_contents();
    assert!(history.contains(HOLDING), "{history}");
    assert!(!history.contains("nori CLI"), "{history}");
    assert_eq!(count(&history, AGENT_ROW), 1, "{history}");
}

#[test]
fn a_short_terminal_writes_the_header_once() {
    let mut session = TuiSession::spawn_with_config(12, 80, SessionConfig::new()).unwrap();

    session.wait_until_ready().unwrap();
    session.settle(Duration::from_millis(2_000));

    // The test emulator keeps no scrollback for rows scrolled inside a
    // scroll region, so on 12 rows the title can be gone from it; a real
    // terminal keeps it. What matters here is that nothing is written twice.
    let history = session.history_contents();
    assert!(count(&history, "〉nori CLI") <= 1, "{history}");
    assert_eq!(count(&history, AGENT_ROW), 1, "{history}");
    assert_eq!(count(&history, CONFIG_ROW), 1, "{history}");
}

#[test]
fn a_narrow_terminal_writes_the_header_once() {
    let mut session = TuiSession::spawn_with_config(24, 30, SessionConfig::new()).unwrap();

    session.wait_until_ready().unwrap();
    session.wait_for_text("〉nori CLI", TIMEOUT).unwrap();
    // The config row wraps at 30 columns, so match one word of it.
    session.wait_for_text("options", TIMEOUT).unwrap();
    session.settle(Duration::from_millis(1_500));

    let history = session.history_contents();
    assert_eq!(count(&history, "〉nori CLI"), 1, "{history}");
    assert_eq!(count(&history, AGENT_ROW), 1, "{history}");
    assert!(!history.contains(HOLDING), "{history}");
}

#[test]
fn with_animations_off_the_wordmark_shows_before_the_session_is_ready() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    let mut session = TuiSession::spawn_with_config(
        24,
        80,
        gated(&release).with_extra_config_toml("[tui]\nanimations = false\n"),
    )
    .unwrap();

    session.wait_for_composer().unwrap();
    session.wait_for_text(WORDMARK, TIMEOUT).unwrap();
    let screen = session.screen_contents();
    assert!(!screen.contains(CONFIG_ROW), "{screen}");
    assert!(!screen.contains(HOLDING), "{screen}");

    std::fs::write(&release, "go").unwrap();
    session.wait_for_text(CONFIG_ROW, TIMEOUT).unwrap();
    session.settle(Duration::from_millis(500));
    assert_eq!(count(&session.history_contents(), "〉nori CLI"), 1);
}

#[test]
fn switching_to_full_screen_before_readiness_keeps_the_header_once() {
    let dir = tempfile::tempdir().unwrap();
    let release = dir.path().join("release");
    let mut session = TuiSession::spawn_with_config(24, 80, gated(&release)).unwrap();
    session
        .wait_for_text(HOLDING, Duration::from_secs(15))
        .unwrap();

    session.submit_input("/screen").unwrap();
    session.settle(Duration::from_millis(500));
    assert!(session.is_alternate_screen());
    let full = session.screen_contents();
    assert_eq!(
        count(&full, AGENT_ROW),
        1,
        "the card is in the full-screen view:\n{full}"
    );

    std::fs::write(&release, "go").unwrap();
    session.wait_for_text(CONFIG_ROW, TIMEOUT).unwrap();
    session.settle(Duration::from_millis(500));
    let full = session.screen_contents();
    assert_eq!(count(&full, AGENT_ROW), 1, "{full}");
    assert!(
        !full.contains("nori CLI"),
        "a face frozen before readiness never winks:\n{full}"
    );
}
