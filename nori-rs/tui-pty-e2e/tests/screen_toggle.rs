//! `/screen` switches the running TUI between the inline view and a
//! full-screen view on the terminal's alternate screen, and back.
//!
//! The switch changes only how the session is drawn: the session, a turn in
//! progress, and the transcript carry on, and every line of history reaches
//! the inline scrollback exactly once.

use std::time::Duration;
use std::time::Instant;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

const ANSWER: &str = "The lighthouse keeper counted nine gulls.";

fn spawn(config: SessionConfig) -> TuiSession {
    let mut session = TuiSession::spawn_with_config(30, 100, config).expect("spawn");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    session
}

/// Wait until the TUI is (or is no longer) on the alternate screen.
fn wait_for_screen(session: &mut TuiSession, full_screen: bool) {
    let deadline = Instant::now() + TIMEOUT;
    while session.is_alternate_screen() != full_screen {
        assert!(
            Instant::now() < deadline,
            "the TUI should {} the full-screen view:\n{}",
            if full_screen { "enter" } else { "leave" },
            session.screen_contents()
        );
        session.settle(Duration::from_millis(50));
    }
    session.settle(Duration::from_millis(300));
}

fn toggle_screen(session: &mut TuiSession, full_screen: bool) {
    session.submit_input("/screen").unwrap();
    wait_for_screen(session, full_screen);
}

fn count(haystack: &str, needle: &str) -> i64 {
    haystack.matches(needle).fold(0, |total, _| total + 1)
}

/// Erase `text` from the composer one character at a time.
fn erase(session: &mut TuiSession, text: &str) {
    for _ in text.chars() {
        session.send_key(Key::Backspace).unwrap();
    }
    session.settle(Duration::from_millis(200));
}

#[test]
fn screen_switches_to_full_screen_and_back_without_reprinting_history() {
    let mut session = spawn(SessionConfig::new().with_mock_response(ANSWER));
    session.submit_input("what did the keeper count").unwrap();
    session
        .wait_for_text(ANSWER, TIMEOUT)
        .expect("inline answer");
    session.settle(Duration::from_millis(300));
    assert!(!session.is_alternate_screen(), "plain nori starts inline");

    toggle_screen(&mut session, true);
    let screen = session.screen_contents();
    assert!(
        screen.contains(ANSWER) && screen.contains("what did the keeper count"),
        "the full-screen view shows the transcript so far:\n{screen}"
    );

    let draft = "unsent draft about the gulls";
    session.type_input(draft).unwrap();
    session.send_key(Key::Ctrl('t')).unwrap();
    session
        .wait_for_text("T R A N S C R I P T", TIMEOUT)
        .expect("Ctrl+T opens the transcript over the full-screen view");
    session.send_key(Key::Char('q')).unwrap();
    session.settle(Duration::from_millis(400));
    assert!(
        session.is_alternate_screen(),
        "closing the transcript returns to the full-screen view, not inline"
    );
    let screen = session.screen_contents();
    assert!(
        !screen.contains("T R A N S C R I P T") && screen.contains(ANSWER),
        "the full-screen view is back:\n{screen}"
    );
    assert!(screen.contains(draft), "the draft survives:\n{screen}");

    session.resize(26, 90).unwrap();
    session.settle(Duration::from_millis(500));
    let screen = session.screen_contents();
    assert!(
        session.is_alternate_screen() && screen.contains(ANSWER) && screen.contains(draft),
        "a resize keeps the full-screen view, its transcript, and the draft:\n{screen}"
    );

    erase(&mut session, draft);
    toggle_screen(&mut session, false);
    let history = session.history_contents();
    assert_eq!(
        count(&history, ANSWER),
        1,
        "the answer is in the scrollback exactly once:\n{history}"
    );
    assert_eq!(
        count(&history, "what did the keeper count"),
        1,
        "the prompt is in the scrollback exactly once:\n{history}"
    );

    toggle_screen(&mut session, true);
    toggle_screen(&mut session, false);
    let history = session.history_contents();
    assert_eq!(
        count(&history, ANSWER),
        1,
        "switching again prints nothing twice:\n{history}"
    );
}

#[test]
fn the_full_screen_view_comes_back_after_the_external_editor() {
    let scripts = tempfile::tempdir().unwrap();
    let editor = scripts.path().join("leaves-the-alternate-screen");
    // Like vim or nano, the editor switches the terminal to the normal
    // screen as it exits.
    std::fs::write(&editor, "#!/bin/sh\nprintf '\\033[?1049l'\n").unwrap();
    std::fs::set_permissions(&editor, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let config = SessionConfig::new()
        .with_mock_response(ANSWER)
        .with_agent_env("VISUAL", editor.display().to_string());
    let mut session = spawn(config);
    session.submit_input("what did the keeper count").unwrap();
    session
        .wait_for_text(ANSWER, TIMEOUT)
        .expect("inline answer");
    session.settle(Duration::from_millis(300));
    toggle_screen(&mut session, true);

    session.send_key(Key::Ctrl('g')).unwrap();
    session.settle(Duration::from_millis(800));
    let screen = session.screen_contents();
    assert!(
        session.is_alternate_screen() && screen.contains(ANSWER),
        "the full-screen view is back after the editor exits:\n{screen}"
    );

    toggle_screen(&mut session, false);
    let history = session.history_contents();
    assert_eq!(count(&history, ANSWER), 1, "{history}");
}

const STREAM_LINES: [&str; 8] = [
    "harbour line one",
    "harbour line two",
    "harbour line three",
    "harbour line four",
    "harbour line five",
    "harbour line six",
    "harbour line seven",
    "harbour line eight",
];

#[test]
fn a_streaming_turn_carries_on_across_the_switch_and_lands_once() {
    let response = STREAM_LINES
        .iter()
        .map(|line| format!("{line}\n\n"))
        .collect::<String>();
    let config = SessionConfig::new()
        .with_mock_response_streamed(response, 6)
        .with_agent_env("MOCK_AGENT_RESPONSE_CHUNK_DELAY_MS", "120");
    let mut session = spawn(config);

    session.submit_input("describe the harbour").unwrap();
    session
        .wait_for_text(STREAM_LINES[0], TIMEOUT)
        .expect("the turn starts streaming inline");
    assert!(
        !session.screen_contents().contains(STREAM_LINES[7]),
        "the turn is still streaming when the view switches"
    );

    toggle_screen(&mut session, true);
    session
        .wait_for_text(STREAM_LINES[7], TIMEOUT)
        .expect("the turn keeps streaming into the full-screen view");
    session.settle(Duration::from_millis(800));
    assert!(session.is_alternate_screen());

    toggle_screen(&mut session, false);
    let history = session.history_contents();
    for line in STREAM_LINES {
        assert_eq!(
            count(&history, line),
            1,
            "{line:?} is in the scrollback exactly once:\n{history}"
        );
    }
    let first = history.find(STREAM_LINES[0]).unwrap();
    let last = history.find(STREAM_LINES[7]).unwrap();
    assert!(first < last, "the lines keep their order:\n{history}");
}

#[test]
fn nori_tui_switches_screens_too() {
    let mut config = SessionConfig::new().with_mock_response(ANSWER);
    config.binary = "nori-tui";
    let mut session = spawn(config);
    session.submit_input("what did the keeper count").unwrap();
    session
        .wait_for_text(ANSWER, TIMEOUT)
        .expect("inline answer");
    session.settle(Duration::from_millis(300));
    assert!(!session.is_alternate_screen(), "nori-tui starts inline");

    toggle_screen(&mut session, true);
    assert!(session.screen_contents().contains(ANSWER));

    toggle_screen(&mut session, false);
    let history = session.history_contents();
    assert_eq!(count(&history, ANSWER), 1, "{history}");
}

/// Submit `prompt` and wait for its answer to land.
fn ask(session: &mut TuiSession, prompt: &str) {
    let before = count(&session.screen_contents(), ANSWER);
    session.submit_input(prompt).unwrap();
    let deadline = Instant::now() + TIMEOUT;
    while count(&session.screen_contents(), ANSWER) <= before {
        assert!(
            Instant::now() < deadline,
            "{prompt:?} should be answered:\n{}",
            session.screen_contents()
        );
        session.settle(Duration::from_millis(50));
    }
    session.settle(Duration::from_millis(300));
}

#[test]
fn moving_between_sessions_while_full_screen_shows_each_session_and_reprints_on_return() {
    let mut session = spawn(SessionConfig::new().with_mock_response(ANSWER));
    ask(&mut session, "question in the first session");
    session.submit_input("/new").unwrap();
    session
        .wait_for(
            |screen| !screen.contains("question in the first session"),
            TIMEOUT,
        )
        .expect("/new starts on a clean screen");
    session.settle(Duration::from_millis(500));
    ask(&mut session, "question in the second session");

    toggle_screen(&mut session, true);
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for(
            |screen| {
                screen.contains("question in the first session")
                    && !screen.contains("question in the second session")
            },
            TIMEOUT,
        )
        .expect("the full-screen view follows the session on screen");
    assert!(session.is_alternate_screen());

    toggle_screen(&mut session, false);
    let history = session.history_contents();
    assert_eq!(
        count(&history, "question in the first session"),
        1,
        "{history}"
    );
    assert_eq!(
        count(&history, "question in the second session"),
        0,
        "the scrollback belongs to the session on screen:\n{history}"
    );
}

/// The text on the composer's prompt line, without the `›` marker.
fn composer_line(screen: &str) -> String {
    screen
        .lines()
        .rev()
        .find_map(|line| line.trim_start().strip_prefix('›'))
        .map(|rest| rest.trim().to_string())
        .unwrap_or_default()
}

#[test]
fn the_screen_key_switches_views_and_leaves_an_unsent_draft_alone() {
    let mut session = spawn(SessionConfig::new().with_mock_response(ANSWER));
    session.submit_input("what did the keeper count").unwrap();
    session
        .wait_for_text(ANSWER, TIMEOUT)
        .expect("inline answer");
    session.settle(Duration::from_millis(300));

    let draft = "count the terns at noon";
    session.type_input(draft).unwrap();
    session.settle(Duration::from_millis(300));

    session.send_key(Key::Ctrl('l')).unwrap();
    wait_for_screen(&mut session, true);
    let screen = session.screen_contents();
    assert_eq!(
        composer_line(&screen),
        draft,
        "the full-screen composer holds the draft:\n{screen}"
    );
    assert_eq!(count(&screen, ANSWER), 1, "nothing was sent:\n{screen}");
    assert_eq!(count(&screen, draft), 1, "nothing was sent:\n{screen}");

    session.send_key(Key::Ctrl('l')).unwrap();
    wait_for_screen(&mut session, false);
    let screen = session.screen_contents();
    assert_eq!(
        composer_line(&screen),
        draft,
        "the inline composer holds the draft:\n{screen}"
    );
    let history = session.history_contents();
    assert_eq!(count(&history, ANSWER), 1, "nothing was sent:\n{history}");
    assert_eq!(count(&history, draft), 1, "nothing was sent:\n{history}");

    session.send_key(Key::Enter).unwrap();
    session
        .wait_for(|screen| count(screen, ANSWER) == 2, TIMEOUT)
        .expect("Enter sends the draft and it is answered");
    session.settle(Duration::from_millis(800));
    let history = session.history_contents();
    assert_eq!(count(&history, ANSWER), 2, "one new answer:\n{history}");
    assert_eq!(count(&history, draft), 1, "sent exactly once:\n{history}");
}

/// Start full-screen against an agent that lists and reloads saved
/// sessions, then resume one. Its reload replays two rows.
fn resume_while_full_screen() -> TuiSession {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1")
        .with_agent_env("MOCK_AGENT_LOAD_SESSION_USER_NOTIFICATION", "1")
        .with_agent_env("MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT", "1");
    let mut session = spawn(config);
    toggle_screen(&mut session, true);

    session.submit_input("/resume").unwrap();
    session
        .wait_for(
            |screen| screen.contains("First mock session") && screen.contains("mock-session-2"),
            TIMEOUT,
        )
        .expect("the picker lists the agent's sessions");
    // Row 0 is the pinned "Start a new session" action.
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
}

#[test]
fn a_session_resumed_while_full_screen_shows_its_replayed_history() {
    let mut session = resume_while_full_screen();

    // Nothing else happens after the replay, so only the replay itself can
    // bring its rows on screen.
    session
        .wait_for(
            |screen| screen.contains("loaded user activity") && screen.contains("replay chunk 0"),
            TIMEOUT,
        )
        .expect("the full-screen view shows the replayed history");
    assert!(
        session.is_alternate_screen(),
        "resuming keeps the full-screen view:\n{}",
        session.screen_contents()
    );

    toggle_screen(&mut session, false);
    session
        .wait_for_text("replay chunk 0", TIMEOUT)
        .expect("the inline view shows the replayed history");
    let history = session.history_contents();
    assert_eq!(count(&history, "loaded user activity"), 1, "{history}");
    assert_eq!(count(&history, "replay chunk 0"), 1, "{history}");
}

#[test]
fn quitting_full_screen_after_a_resume_leaves_the_resumed_history_in_the_scrollback() {
    let mut session = resume_while_full_screen();
    session
        .wait_for(
            |screen| screen.contains("loaded user activity") && screen.contains("replay chunk 0"),
            TIMEOUT,
        )
        .expect("the full-screen view shows the replayed history");

    session.submit_input("/quit").unwrap();
    session
        .wait_for_exit_status(TIMEOUT)
        .unwrap()
        .expect("/quit exits");
    assert!(!session.is_alternate_screen());
    let history = session.history_contents();
    assert_eq!(count(&history, "loaded user activity"), 1, "{history}");
    assert_eq!(count(&history, "replay chunk 0"), 1, "{history}");
}
