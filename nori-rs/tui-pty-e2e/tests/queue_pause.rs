//! Choosing one queued follow-up from the list, and a queue held after an
//! interrupted or failed turn until the user resumes it.
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

/// Queued rows as drawn, with the selected one marked `>`.
fn queued_rows(screen: &str) -> Vec<String> {
    screen
        .lines()
        .filter_map(|line| {
            line.strip_prefix("  ↳ ")
                .map(|row| row.trim_end().to_string())
                .or_else(|| {
                    line.strip_prefix("  › ")
                        .filter(|row| !row.starts_with("Nori CLI"))
                        .map(|row| format!("> {}", row.trim_end()))
                })
        })
        .collect()
}

fn spawn_session(log: &Path, delay_ms: &str) -> TuiSession {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_owned())
        .with_mock_response("MOCK_TURN_DONE")
        .with_agent_env("MOCK_AGENT_DELAY_MS", delay_ms)
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

fn key(session: &mut TuiSession, key: Key) {
    session.send_key(key).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
}

fn queue_three_behind(session: &mut TuiSession, log: &Path, first: &str) {
    session.submit_input(first).unwrap();
    wait_for_deliveries(log, 1, TIMEOUT);
    for text in ["alpha", "dup", "dup"] {
        queue_input(session, text);
    }
    session
        .wait_for(
            |screen| queued_rows(screen) == ["alpha", "dup", "dup"],
            TIMEOUT,
        )
        .expect("three queued rows");
}

#[test]
#[cfg(target_os = "linux")]
fn the_selected_duplicate_is_edited_and_delivered_once() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("prompts.jsonl");
    let mut session = spawn_session(&log, "8000");
    queue_three_behind(&mut session, &log, "turn one");

    key(&mut session, Key::Up);
    key(&mut session, Key::Char('k'));
    session
        .wait_for(
            |screen| queued_rows(screen) == ["alpha", "> dup", "dup"],
            TIMEOUT,
        )
        .expect("the earlier duplicate is selected");
    key(&mut session, Key::Enter);
    session
        .wait_for(
            |screen| queued_rows(screen) == ["alpha", "dup"] && screen.contains("› dup"),
            TIMEOUT,
        )
        .expect("only the selected duplicate leaves the queue");
    queue_input(&mut session, " edited");

    assert_eq!(
        wait_for_deliveries(&log, 4, Duration::from_secs(30)),
        ["turn one", "alpha", "dup", "dup edited"],
        "{}",
        session.screen_contents()
    );
}

#[test]
#[cfg(target_os = "linux")]
fn the_selected_message_is_removed_and_never_delivered() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("prompts.jsonl");
    let mut session = spawn_session(&log, "8000");
    queue_three_behind(&mut session, &log, "turn one");

    key(&mut session, Key::Up);
    key(&mut session, Key::Char('k'));
    key(&mut session, Key::Char('k'));
    key(&mut session, Key::Char('x'));
    session
        .wait_for(|screen| queued_rows(screen) == ["> dup", "dup"], TIMEOUT)
        .expect("alpha leaves the queue and the selection moves to its neighbour");
    key(&mut session, Key::Escape);
    session.type_input("draft kept").unwrap();
    session
        .wait_for(
            |screen| queued_rows(screen) == ["dup", "dup"] && screen.contains("› draft kept"),
            TIMEOUT,
        )
        .expect("the composer holds only what the user typed");

    let delivered = wait_for_deliveries(&log, 3, Duration::from_secs(30));
    std::thread::sleep(Duration::from_secs(5));
    assert_eq!(delivered, ["turn one", "dup", "dup"]);
    assert_eq!(delivered_prompts(&log), delivered, "nothing else is sent");
}

#[test]
#[cfg(target_os = "linux")]
fn an_interrupt_holds_the_queue_until_enter_resumes_it() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("prompts.jsonl");
    let mut session = spawn_session(&log, "300");
    queue_three_behind(&mut session, &log, "mock:stream-until-cancel");

    key(&mut session, Key::Escape);
    session
        .wait_for_text("3 queued · paused (interrupted)", TIMEOUT)
        .expect("the paused queue shows its count");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        delivered_prompts(&log),
        ["mock:stream-until-cancel"],
        "nothing is sent on its own:\n{}",
        session.screen_contents()
    );
    assert_eq!(
        queued_rows(&session.screen_contents()),
        ["alpha", "dup", "dup"]
    );

    key(&mut session, Key::Enter);
    let delivered = wait_for_deliveries(&log, 4, Duration::from_secs(30));
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        delivered,
        ["mock:stream-until-cancel", "alpha", "dup", "dup"],
        "{}",
        session.screen_contents()
    );
    assert_eq!(delivered_prompts(&log), delivered, "each is sent once");
}

#[test]
#[cfg(target_os = "linux")]
fn a_turn_error_holds_the_queue_until_enter_resumes_it() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("prompts.jsonl");
    let mut session = spawn_session(&log, "3000");
    queue_three_behind(&mut session, &log, "mock:prompt-fail-after-delay");

    session
        .wait_for_text("3 queued · paused (turn error)", Duration::from_secs(15))
        .expect("the paused queue shows its count");
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(delivered_prompts(&log), ["mock:prompt-fail-after-delay"]);

    key(&mut session, Key::Enter);
    let delivered = wait_for_deliveries(&log, 4, Duration::from_secs(30));
    assert_eq!(
        delivered,
        ["mock:prompt-fail-after-delay", "alpha", "dup", "dup"],
        "{}",
        session.screen_contents()
    );
}
