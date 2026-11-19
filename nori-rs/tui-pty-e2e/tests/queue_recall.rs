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
        .with_agent_env("MOCK_AGENT_STEERING", "1")
        .with_agent_env("MOCK_AGENT_DELAY_MS", TURN_DELAY_MS)
        .with_agent_env("MOCK_AGENT_PROMPT_LOG", log.display().to_string());
    let mut session = TuiSession::spawn_with_config(30, 90, config).expect("spawn nori");
    session.wait_for_text("›", TIMEOUT).expect("composer ready");
    std::thread::sleep(TIMEOUT_INPUT);
    session
}

fn queue_input(session: &mut TuiSession, text: &str) {
    session.type_input(text).unwrap();
    session.send_key(Key::Tab).unwrap();
}

#[test]
#[cfg(target_os = "linux")]
fn up_reclaims_newest_queued_message_and_edited_resubmission_is_delivered_once() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("prompts.jsonl");
    let mut session = spawn_busy_session(&log);

    session.submit_input("turn one").unwrap();
    wait_for_deliveries(&log, 1, TIMEOUT);
    queue_input(&mut session, "alpha");
    session
        .wait_for_text("• Queued for the next turn.", TIMEOUT)
        .expect("explicit queue receipt");
    queue_input(&mut session, "dup");
    queue_input(&mut session, "dup");
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

    queue_input(&mut session, " edited");
    session
        .wait_for(
            |screen| queued_rows(screen) == ["alpha", "dup", "dup edited"],
            TIMEOUT,
        )
        .expect("edited message is queued once, after the others");

    queue_input(&mut session, "after edited follow-up");
    assert_eq!(
        wait_for_deliveries(&log, 5, Duration::from_secs(30)),
        [
            "turn one",
            "alpha",
            "dup",
            "dup edited",
            "after edited follow-up"
        ],
        "{}",
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
    queue_input(&mut session, "withdraw me");
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
        !session.screen_contents().contains("withdrawn"),
        "taking a message back is not reported as an error:\n{}",
        session.screen_contents()
    );
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
    queue_input(&mut session, "take me back");
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

#[test]
#[cfg(target_os = "linux")]
fn enter_steers_with_the_edited_draft_after_up_withdraws_it() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("prompts.jsonl");
    let mut session = spawn_busy_session(&log);

    session.submit_input("Sketch the orchard plan").unwrap();
    session
        .wait_for_text("Survey in progress", TIMEOUT)
        .unwrap();
    queue_input(&mut session, "check the north row");
    session
        .wait_for(
            |screen| queued_rows(screen) == ["check the north row"],
            TIMEOUT,
        )
        .unwrap();
    session.send_key(Key::Up).unwrap();
    session
        .wait_for(
            |screen| queued_rows(screen).is_empty() && screen.contains("› check the north row"),
            TIMEOUT,
        )
        .unwrap();

    session.submit_input(" first").unwrap();
    session
        .wait_for(
            |screen| {
                screen.contains("Survey correction received: check the north row first.")
                    && !screen.contains("esc to interrupt")
            },
            TIMEOUT,
        )
        .expect("the original turn consumed the edited correction");
    session.submit_input("after correction").unwrap();
    assert_eq!(
        wait_for_deliveries(&log, 2, TIMEOUT),
        ["Sketch the orchard plan", "after correction"],
        "the withdrawn draft was never sent as a separate prompt"
    );
}
