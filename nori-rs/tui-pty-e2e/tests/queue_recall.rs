//! Recalling a queued follow-up while the agent is busy.
//!
//! The mock agent records every delivered prompt in a log file, so these
//! tests assert on what actually reached the agent, not only on the screen.

use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use pretty_assertions::assert_eq;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

const TURN_DELAY_MS: &str = "4000";

fn delivered_prompts(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str::<String>(line).expect("prompt log line"))
        .collect()
}

fn wait_for_deliveries(log: &Path, count: usize, timeout: Duration) -> Vec<String> {
    let deadline = Instant::now() + timeout;
    loop {
        let delivered = delivered_prompts(log);
        if delivered.len() >= count || Instant::now() > deadline {
            return delivered;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn queued_rows(screen: &str) -> Vec<String> {
    screen
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("↳ "))
        .map(|row| row.trim_end().to_string())
        .collect()
}

fn spawn_busy_session(log: &Path) -> TuiSession {
    spawn_busy_session_with(log, SessionConfig::new())
}

fn spawn_busy_session_with(log: &Path, config: SessionConfig) -> TuiSession {
    let config = config
        .with_agent("mock-model".to_owned())
        .with_mock_response("MOCK_TURN_DONE")
        .with_agent_env("MOCK_AGENT_DELAY_MS", TURN_DELAY_MS)
        .with_agent_env("MOCK_AGENT_PROMPT_LOG", log.display().to_string());
    let mut session = TuiSession::spawn_with_config(30, 90, config).expect("spawn nori");
    session.wait_for_text("›", TIMEOUT).expect("composer ready");
    std::thread::sleep(TIMEOUT_INPUT);
    session
}

#[test]
#[cfg(target_os = "linux")]
fn up_reclaims_newest_queued_message_and_edited_resubmission_is_delivered_once() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("prompts.jsonl");
    let mut session = spawn_busy_session(&log);

    session.submit_input("turn one").unwrap();
    wait_for_deliveries(&log, 1, TIMEOUT);
    session.submit_input("alpha").unwrap();
    session.submit_input("dup").unwrap();
    session.submit_input("dup").unwrap();
    session
        .wait_for(
            |screen| queued_rows(screen) == ["alpha", "dup", "dup"],
            TIMEOUT,
        )
        .expect("three queued rows");

    session.send_key(Key::Up).unwrap();
    session
        .wait_for(
            |screen| queued_rows(screen) == ["alpha", "dup"] && screen.contains("› dup"),
            TIMEOUT,
        )
        .expect("the newest duplicate leaves the queue and enters the composer");

    session.send_str(" edited").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for(
            |screen| queued_rows(screen) == ["alpha", "dup", "dup edited"],
            TIMEOUT,
        )
        .expect("edited message is queued once, after the others");

    let delivered = wait_for_deliveries(&log, 4, Duration::from_secs(30));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        delivered_prompts(&log),
        ["turn one", "alpha", "dup", "dup edited"],
        "delivered so far: {delivered:?}\n{}",
        session.screen_contents()
    );
}

#[test]
#[cfg(target_os = "linux")]
fn alt_up_reclaims_the_only_queued_message_so_it_is_never_delivered() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("prompts.jsonl");
    let mut session = spawn_busy_session(&log);

    session.submit_input("turn one").unwrap();
    wait_for_deliveries(&log, 1, TIMEOUT);
    session.submit_input("withdraw me").unwrap();
    session
        .wait_for(|screen| queued_rows(screen) == ["withdraw me"], TIMEOUT)
        .expect("queued row");

    session.send_key(Key::AltUp).unwrap();
    session
        .wait_for(
            |screen| queued_rows(screen).is_empty() && screen.contains("› withdraw me"),
            TIMEOUT,
        )
        .expect("message moves into the composer and the queue empties");
    assert_eq!(
        delivered_prompts(&log),
        ["turn one"],
        "the queue emptied by reclaim, not by dispatch"
    );

    session
        .wait_for_text("MOCK_TURN_DONE", Duration::from_secs(15))
        .expect("first turn completes");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(delivered_prompts(&log), ["turn one"]);
    assert!(
        session.screen_contents().contains("› withdraw me"),
        "the draft stays in the composer:\n{}",
        session.screen_contents()
    );
}

#[test]
#[cfg(target_os = "linux")]
fn alt_up_outside_a_delegated_child_reclaims_with_several_sessions_enabled() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("prompts.jsonl");
    let mut session = spawn_busy_session_with(
        &log,
        SessionConfig::new().with_extra_config_toml("[features]\nmulti_session = true\n"),
    );

    session.submit_input("turn one").unwrap();
    wait_for_deliveries(&log, 1, TIMEOUT);
    session.submit_input("take me back").unwrap();
    session
        .wait_for(|screen| queued_rows(screen) == ["take me back"], TIMEOUT)
        .expect("queued row");

    session.send_key(Key::AltUp).unwrap();
    session
        .wait_for(
            |screen| queued_rows(screen).is_empty() && screen.contains("› take me back"),
            TIMEOUT,
        )
        .expect("alt + up reaches the queue when there is no parent session");
    assert_eq!(delivered_prompts(&log), ["turn one"]);
}
