//! The session list and background requests, behind `[features] multi_session`.

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

const ALPHA_ANSWER: &str = "alpha agent answer";
const BETA_ANSWER: &str = "beta agent answer";
const BACKGROUND_REQUEST: &str = "mock:request-permission:rm -rf target/debug";

fn config() -> SessionConfig {
    SessionConfig::new()
        .with_extra_config_toml("[features]\nmulti_session = true\n")
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL", ALPHA_ANSWER)
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", BETA_ANSWER)
        .with_agent_env("MOCK_AGENT_PERMISSION_DELAY_MS", "1500")
}

fn spawn() -> TuiSession {
    let mut session = TuiSession::spawn_with_config(40, 110, config()).expect("spawn");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    session
}

fn ask(session: &mut TuiSession, prompt: &str, answer: &str) {
    session.submit_input(prompt).unwrap();
    session
        .wait_for_text(answer, TIMEOUT)
        .unwrap_or_else(|e| panic!("{answer:?} should arrive: {e}"));
    session.settle(Duration::from_millis(300));
}

fn start_alt_session(session: &mut TuiSession) {
    session.submit_input("/agent").unwrap();
    session
        .wait_for(
            |screen| screen.contains("Select agent") || screen.contains("mock-model"),
            Duration::from_secs(8),
        )
        .expect("agent picker should open");
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Start a new session", Duration::from_secs(10))
        .expect("candidate session picker should open");
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text(
            "Started new conversation with agent: Mock ACP Alt",
            Duration::from_secs(10),
        )
        .expect("the alternate session should start");
    std::thread::sleep(TIMEOUT_INPUT);
}

fn start_new_session(session: &mut TuiSession) {
    session.submit_input("/new").unwrap();
    session
        .wait_for(
            |screen| screen.contains("Mock Default Model") && screen.contains("›"),
            TIMEOUT,
        )
        .expect("a new session should start");
    session.settle(Duration::from_millis(500));
}

/// Three sessions: a finished alpha session, a beta session whose request
/// arrives while it is in the background, and a finished gamma session on
/// screen.
fn three_sessions() -> TuiSession {
    let mut session = spawn();
    ask(&mut session, "alpha question", ALPHA_ANSWER);
    start_alt_session(&mut session);
    session.submit_input(BACKGROUND_REQUEST).unwrap();
    session.settle(Duration::from_millis(200));
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for_text("1 waiting on you", Duration::from_secs(8))
        .expect("the beta request should arrive in the background");
    start_new_session(&mut session);
    ask(&mut session, "gamma question", ALPHA_ANSWER);
    session
}

#[test]
#[cfg(target_os = "linux")]
fn left_on_an_empty_prompt_lists_sessions_with_status_and_preview() {
    let mut session = three_sessions();
    session.send_key(Key::Left).unwrap();
    session
        .wait_for_text("Sessions 3", Duration::from_secs(3))
        .expect("Left on an empty prompt opens the session list");
    session.settle(Duration::from_millis(300));
    let screen = session.screen_contents();
    for expected in [
        "alpha question",
        "approve: rm -rf target/debug",
        "waiting on you",
        "gamma question",
        "(current)",
    ] {
        assert!(screen.contains(expected), "{expected:?} missing:\n{screen}");
    }
    insta::assert_snapshot!(
        "session_list_three_sessions",
        normalize_for_input_snapshot(screen)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn tab_shows_only_sessions_waiting_on_you_and_typing_filters() {
    let mut session = three_sessions();
    session.send_key(Key::Alt('s')).unwrap();
    session
        .wait_for_text("Sessions 3", Duration::from_secs(3))
        .expect("Alt+S opens the session list");

    session.send_key(Key::Tab).unwrap();
    session
        .wait_for(
            |screen| screen.contains("tab: all sessions") && !screen.contains("○ gamma question"),
            Duration::from_secs(3),
        )
        .expect("Tab keeps only the session waiting on you");
    session.settle(Duration::from_millis(200));
    insta::assert_snapshot!(
        "session_list_waiting_on_you",
        normalize_for_input_snapshot(session.screen_contents())
    );
    session.send_key(Key::Tab).unwrap();

    for c in "gamma".chars() {
        session.send_key(Key::Char(c)).unwrap();
    }
    session
        .wait_for(
            |screen| screen.contains("○ gamma question") && !screen.contains("○ alpha question"),
            Duration::from_secs(3),
        )
        .expect("typing filters by title");

    session.send_key(Key::Escape).unwrap();
    session
        .wait_for_text("○ alpha question", Duration::from_secs(3))
        .expect("Esc clears the filter");
    session.send_key(Key::Escape).unwrap();
    session
        .wait_for(
            |screen| !screen.contains("Sessions 3"),
            Duration::from_secs(3),
        )
        .expect("a second Esc closes the list");
}

#[test]
#[cfg(target_os = "linux")]
fn enter_in_the_list_jumps_to_that_session() {
    let mut session = three_sessions();
    session.submit_input("/sessions").unwrap();
    session
        .wait_for_text("Sessions 3", Duration::from_secs(3))
        .expect("/sessions opens the session list");
    session.send_key(Key::Up).unwrap();
    session.send_key(Key::Up).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for(
            |screen| screen.contains("alpha question") && !screen.contains("gamma question"),
            Duration::from_secs(5),
        )
        .expect("Enter shows the chosen session");
}

#[test]
#[cfg(target_os = "linux")]
fn session_list_is_absent_without_the_flag() {
    let mut session = TuiSession::spawn_with_config(30, 100, SessionConfig::new()).expect("spawn");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Left).unwrap();
    session.send_key(Key::Alt('s')).unwrap();
    session.type_input("/sess").unwrap();
    session.settle(Duration::from_millis(500));
    let screen = session.screen_contents();
    assert!(!screen.contains("Sessions"), "{screen}");
    assert!(!screen.contains("/sessions"), "{screen}");
}

#[test]
#[cfg(target_os = "linux")]
fn a_background_request_is_named_and_one_key_jumps_to_it_for_an_answer() {
    let mut session = spawn();
    ask(&mut session, "alpha question", ALPHA_ANSWER);
    start_alt_session(&mut session);
    session.submit_input(BACKGROUND_REQUEST).unwrap();
    session.settle(Duration::from_millis(200));
    session.send_key(Key::Alt(',')).unwrap();

    start_new_session(&mut session);
    session
        .submit_input("mock:request-permission:git push --force")
        .unwrap();
    session.settle(Duration::from_millis(200));
    session.send_key(Key::Alt('.')).unwrap();

    session
        .wait_for(
            |screen| {
                screen.contains("waiting on you · approve: rm -rf target/debug")
                    && screen.contains("(+1 more)")
                    && screen.contains("to jump")
            },
            Duration::from_secs(8),
        )
        .expect("the request line names the first waiting session and its request");
    insta::assert_snapshot!(
        "background_request_line",
        normalize_for_input_snapshot(session.screen_contents())
    );

    session.send_key(Key::AltDown).unwrap();
    session
        .wait_for(
            |screen| {
                screen.contains("Would you like to allow") && screen.contains("rm -rf target/debug")
            },
            Duration::from_secs(5),
        )
        .expect("next_waiting_session shows the waiting request");
    session.send_key(Key::Char('1')).unwrap();
    session
        .wait_for_text(
            "Permission granted with option: allow",
            Duration::from_secs(5),
        )
        .expect("the answered turn completes");

    session.send_key(Key::AltDown).unwrap();
    session
        .wait_for(
            |screen| {
                screen.contains("Would you like to allow") && screen.contains("git push --force")
            },
            Duration::from_secs(5),
        )
        .expect("the second request is told apart by its command");
}

#[test]
#[cfg(target_os = "linux")]
fn an_agent_that_dies_fails_only_its_session_which_can_be_resumed() {
    let mut session = spawn();
    ask(&mut session, "alpha question", ALPHA_ANSWER);
    start_new_session(&mut session);
    ask(&mut session, "doomed question", ALPHA_ANSWER);
    session.submit_input("mock:exit-during-prompt").unwrap();
    session
        .wait_for_text("1 failed · 1 ready", Duration::from_secs(8))
        .expect("the dead agent fails only its own session");

    session.send_key(Key::Alt(',')).unwrap();
    ask(&mut session, "alpha still works", ALPHA_ANSWER);

    session.send_key(Key::Alt('s')).unwrap();
    session
        .wait_for(
            |screen| {
                screen.contains("failed") && screen.contains("agent exited · resume available")
            },
            Duration::from_secs(3),
        )
        .expect("the failed row offers a resume");
    session.send_key(Key::Down).unwrap();
    session
        .wait_for_text("ctrl+r resume", Duration::from_secs(3))
        .expect("the key hint names the resume key on a failed row");
    insta::assert_snapshot!(
        "session_list_failed_row",
        normalize_for_input_snapshot(session.screen_contents())
    );
    session.send_key(Key::Ctrl('r')).unwrap();
    session
        .wait_for(
            |screen| screen.contains("doomed question") && screen.contains("2 ready"),
            Duration::from_secs(10),
        )
        .expect("resuming replays the failed session's transcript on a new agent");
    ask(&mut session, "after resume", ALPHA_ANSWER);
}

#[test]
#[cfg(target_os = "linux")]
fn a_fork_is_listed_under_its_parent_and_its_request_marks_the_parent() {
    let mut session = spawn();
    ask(&mut session, "parent question", ALPHA_ANSWER);
    session.submit_input("/fork").unwrap();
    session
        .wait_for_text("parent question", Duration::from_secs(5))
        .expect("the fork picker lists the parent's messages");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for(|screen| screen.contains("2 ready"), Duration::from_secs(10))
        .expect("the fork starts beside its parent");
    session.settle(Duration::from_millis(500));
    // The fork prefills the composer with the message it rewound to.
    session.send_key(Key::Ctrl('u')).unwrap();
    session.submit_input(BACKGROUND_REQUEST).unwrap();
    session.settle(Duration::from_millis(200));
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for_text("1 waiting on you", Duration::from_secs(8))
        .expect("the child's request arrives while the parent is on screen");

    session.send_key(Key::Alt('s')).unwrap();
    session
        .wait_for(
            |screen| screen.contains("└─ ● mock:request-permission"),
            Duration::from_secs(3),
        )
        .expect("the fork is indented under its parent");
    let screen = session.screen_contents();
    let parent_row = screen
        .lines()
        .find(|line| line.contains("parent question (current)"))
        .unwrap_or_else(|| panic!("parent row missing:\n{screen}"));
    assert!(
        parent_row.contains("waiting on you"),
        "the child's request marks the parent row:\n{screen}"
    );
    insta::assert_snapshot!(
        "session_list_fork_under_parent",
        normalize_for_input_snapshot(screen)
    );
}

#[test]
#[cfg(target_os = "linux")]
fn an_unfocused_terminal_is_notified_when_a_background_session_needs_you() {
    let mut session = spawn();
    ask(&mut session, "alpha question", ALPHA_ANSWER);
    start_new_session(&mut session);
    session.submit_input(BACKGROUND_REQUEST).unwrap();
    session.settle(Duration::from_millis(200));
    session.send_key(Key::Alt(',')).unwrap();
    session.send_key(Key::FocusOut).unwrap();
    session
        .wait_for_text("1 waiting on you", Duration::from_secs(8))
        .expect("the request arrives in the background");
    session.settle(Duration::from_millis(500));
    let raw = String::from_utf8_lossy(session.raw_output()).to_string();
    let notification = raw
        .split("\u{1b}]9;")
        .nth(1)
        .and_then(|rest| rest.split('\u{7}').next())
        .unwrap_or_else(|| panic!("no OSC 9 notification in the output"));
    assert_eq!(
        notification,
        "mock:request-permission:rm -rf target/debug: Approval requested: Execute shell command"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn a_focused_terminal_gets_no_notification() {
    let mut session = spawn();
    session.send_key(Key::FocusIn).unwrap();
    ask(&mut session, "alpha question", ALPHA_ANSWER);
    start_new_session(&mut session);
    session.submit_input(BACKGROUND_REQUEST).unwrap();
    session.settle(Duration::from_millis(200));
    session.send_key(Key::Alt(',')).unwrap();
    session
        .wait_for_text("1 waiting on you", Duration::from_secs(8))
        .expect("the request arrives in the background");
    session.settle(Duration::from_millis(500));
    let raw = String::from_utf8_lossy(session.raw_output()).to_string();
    assert!(!raw.contains("\u{1b}]9;"), "unexpected notification");
}
