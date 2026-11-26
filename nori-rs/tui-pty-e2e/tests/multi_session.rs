//! Several sessions in one TUI.
//!
//! Each session runs on its own mock agent process. The scrollback always
//! belongs to the session on screen: moving to another session clears it and
//! reprints that session's history.

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

const ALPHA_ANSWER: &str = "alpha agent answer";
const BETA_ANSWER: &str = "beta agent answer";

fn multi_session_config() -> SessionConfig {
    SessionConfig::new()
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL", ALPHA_ANSWER)
        .with_agent_env("MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT", BETA_ANSWER)
}

fn spawn(config: SessionConfig) -> TuiSession {
    let mut session = TuiSession::spawn_with_config(30, 100, config).expect("spawn");
    session.wait_until_ready().expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    session
}

/// Start a second session on the alternate mock agent through `/agent`.
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

/// Submit `prompt` and wait for a new copy of `answer`, so an answer already
/// in the history from an earlier turn does not count.
fn ask(session: &mut TuiSession, prompt: &str, answer: &str) {
    let before = session.history_contents().matches(answer).count();
    session.submit_input(prompt).unwrap();
    let deadline = std::time::Instant::now() + TIMEOUT;
    while session.history_contents().matches(answer).count() <= before {
        assert!(
            std::time::Instant::now() < deadline,
            "{answer:?} should arrive for {prompt:?}:\n{}",
            session.screen_contents()
        );
        session.settle(Duration::from_millis(50));
    }
    session.settle(Duration::from_millis(300));
}

/// Send a key and wait until the full history satisfies `pred`.
fn wait_for_history(session: &mut TuiSession, pred: impl Fn(&str) -> bool, what: &str) -> String {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        session.settle(Duration::from_millis(100));
        let history = session.history_contents();
        if pred(&history) {
            return history;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "{what}; history was:\n{history}"
        );
    }
}

#[test]
#[cfg(target_os = "linux")]
fn session_moves_reprint_only_the_target_and_keep_unsent_input() {
    let mut session = spawn(multi_session_config());
    ask(&mut session, "first alpha question", ALPHA_ANSWER);

    start_alt_session(&mut session);
    let history = session.history_contents();
    assert!(
        !history.contains(ALPHA_ANSWER),
        "a new session starts on a clean screen:\n{history}"
    );
    ask(&mut session, "first beta question", BETA_ANSWER);

    session.type_input("unsent beta draft").unwrap();

    session.send_key(Key::Alt(',')).unwrap();
    let history = wait_for_history(
        &mut session,
        |h| h.contains("first alpha question") && !h.contains("first beta question"),
        "previous_session should show only the alpha session",
    );
    assert!(!history.contains("unsent beta draft"), "{history}");

    ask(&mut session, "second alpha question", ALPHA_ANSWER);

    session.send_key(Key::Alt('.')).unwrap();
    let history = wait_for_history(
        &mut session,
        |h| h.contains("first beta question") && !h.contains("alpha question"),
        "next_session should show only the beta session",
    );
    assert!(
        session.screen_contents().contains("unsent beta draft"),
        "the beta session keeps its unsent input:\n{history}"
    );

    session.send_key(Key::Alt('.')).unwrap();
    let history = wait_for_history(
        &mut session,
        |h| h.contains("second alpha question") && !h.contains("beta question"),
        "next_session wraps around to the alpha session",
    );
    assert_eq!(
        history.matches("first alpha question").count(),
        1,
        "each alpha turn appears once after the reprint:\n{history}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn new_keeps_the_previous_session_running_behind_the_new_one() {
    let mut session = spawn(multi_session_config());
    ask(&mut session, "question before new", ALPHA_ANSWER);

    session.submit_input("/new").unwrap();
    wait_for_history(
        &mut session,
        |h| !h.contains("question before new") && h.contains("Mock Default Model"),
        "/new should start a session on a clean screen",
    );
    ask(&mut session, "question after new", ALPHA_ANSWER);

    session.send_key(Key::Alt(',')).unwrap();
    wait_for_history(
        &mut session,
        |h| h.contains("question before new") && !h.contains("question after new"),
        "the first session is still there behind the new one",
    );
}

#[test]
#[cfg(target_os = "linux")]
fn status_line_counts_sessions_by_status() {
    let mut session = spawn(multi_session_config());
    ask(&mut session, "finished question", ALPHA_ANSWER);

    session.submit_input("/new").unwrap();
    session
        .wait_for_text("Mock Default Model", TIMEOUT)
        .expect("a second session should start");
    std::thread::sleep(Duration::from_millis(500));
    session.submit_input("mock:stream-until-cancel").unwrap();
    session
        .wait_for_text("1 working · 1 ready", TIMEOUT)
        .expect("the status line counts one working and one ready session");
}

#[test]
#[cfg(target_os = "linux")]
fn one_session_shows_no_status_line() {
    let mut session = spawn(multi_session_config());
    ask(&mut session, "only question", ALPHA_ANSWER);
    session.settle(Duration::from_millis(500));
    let screen = session.screen_contents();
    assert!(!screen.contains("1 ready"), "{screen}");
}

/// Measures how long a jump to a session with a long history takes: from the
/// key press until the reprint stops arriving. Run with
/// `cargo test -p tui-pty-e2e --test multi_session jump_timing -- --ignored --nocapture`.
#[test]
#[ignore = "measurement, takes minutes"]
#[cfg(target_os = "linux")]
fn jump_timing_on_a_long_history() {
    let turns: i32 = std::env::var("JUMP_TIMING_TURNS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(500);
    let config = multi_session_config().with_agent_env(
        "MOCK_AGENT_RESPONSE_MOCK_MODEL",
        "A three line answer\nwith a second line\nand a third line",
    );
    let mut session = TuiSession::spawn_with_config(40, 120, config).expect("spawn");
    session.wait_until_ready().expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);
    for turn in 0..turns {
        let prompt = format!("long history turn {turn}");
        session.submit_input(&prompt).unwrap();
        session
            .wait_for(
                |screen| screen.contains("and a third line") && screen.contains(&prompt),
                TIMEOUT,
            )
            .unwrap_or_else(|e| panic!("turn {turn}: {e}"));
    }
    session.settle(Duration::from_millis(500));
    start_alt_session(&mut session);
    ask(&mut session, "short history", BETA_ANSWER);

    let mut samples = Vec::new();
    for _ in 0..5 {
        for (key, marker) in [
            (',', format!("long history turn {}", turns - 1)),
            ('.', "short history".to_string()),
        ] {
            session.settle(Duration::from_millis(300));
            let before = session.raw_output().len();
            let started = std::time::Instant::now();
            session.send_key(Key::Alt(key)).unwrap();
            let mut last_byte = started;
            let mut seen = session.raw_output().len();
            loop {
                let _ = session.poll();
                let len = session.raw_output().len();
                if len != seen {
                    seen = len;
                    last_byte = std::time::Instant::now();
                }
                if last_byte.elapsed() > Duration::from_millis(250)
                    && session.screen_contents().contains(&marker)
                {
                    break;
                }
                assert!(
                    started.elapsed() < Duration::from_secs(20),
                    "jump never settled"
                );
                std::thread::sleep(Duration::from_millis(2));
            }
            if key == ',' {
                samples.push((last_byte - started, seen - before));
            }
        }
    }
    let history = session.history_contents();
    eprintln!(
        "JUMP_TIMING turns={turns} history_lines={} samples_ms={:?} bytes={:?}",
        history.lines().count(),
        samples
            .iter()
            .map(|(d, _)| d.as_millis())
            .collect::<Vec<_>>(),
        samples.iter().map(|(_, b)| *b).collect::<Vec<_>>()
    );
}

#[test]
#[cfg(target_os = "linux")]
fn session_moves_follow_the_bindings_in_config() {
    let config = multi_session_config()
        .with_extra_config_toml("[tui.hotkeys]\nprevious_session = \"alt+j\"\n");
    let mut session = spawn(config);
    ask(&mut session, "first alpha question", ALPHA_ANSWER);
    start_alt_session(&mut session);
    ask(&mut session, "first beta question", BETA_ANSWER);

    session.send_key(Key::Alt(',')).unwrap();
    session.settle(Duration::from_millis(800));
    assert!(
        !session.history_contents().contains("first alpha question"),
        "the default binding no longer moves once rebound"
    );
    session.send_key(Key::Alt('j')).unwrap();
    wait_for_history(
        &mut session,
        |h| h.contains("first alpha question") && !h.contains("first beta question"),
        "the configured binding moves to the previous session",
    );
}

#[test]
#[cfg(target_os = "linux")]
fn a_burst_of_moves_lands_on_the_last_target() {
    let mut session = spawn(multi_session_config());
    ask(&mut session, "first alpha question", ALPHA_ANSWER);
    session.submit_input("/new").unwrap();
    wait_for_history(
        &mut session,
        |h| !h.contains("first alpha question"),
        "/new",
    );
    ask(&mut session, "second alpha question", ALPHA_ANSWER);
    start_alt_session(&mut session);
    ask(&mut session, "first beta question", BETA_ANSWER);

    session.send_str("\x1b.\x1b.\x1b.\x1b.").unwrap();
    wait_for_history(
        &mut session,
        |h| {
            h.contains("first alpha question")
                && !h.contains("beta question")
                && !h.contains("second alpha")
        },
        "four moves from the third of three sessions land on the first",
    );
}
