use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;

/// After pressing Escape to interrupt a streaming turn, submitting a new
/// message should immediately enter the working state (spinner + "esc to
/// interrupt" hint). Previously a second message was required because the
/// old prompt task's late TurnLifecycle::Completed event would reset the
/// TUI to idle.
///
/// Uses a 2-second cancel delay in the mock agent to widen the race window:
/// the old prompt task won't return (and emit Completed) until well after
/// the user resubmits and the new task emits Started. This ensures the
/// event ordering is Aborted → Started → (stale) Completed, which is
/// the problematic sequence.
#[test]
#[cfg(target_os = "linux")]
fn test_escape_then_resubmit_enters_working_state() {
    let config = SessionConfig::new()
        .with_agent_env("MOCK_AGENT_MULTI_TURN_STREAM_UNTIL_CANCEL", "1")
        .with_agent_env("MOCK_AGENT_CANCEL_DELAY_MS", "2000");
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    // Wait for the prompt to appear
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit first prompt — agent will stream until cancel
    session.send_str("first message").unwrap();
    session.wait_for_text("first message", TIMEOUT).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for streaming to start — confirms working state for first turn
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("First turn did not enter working state");

    // Let some streaming happen
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Press Escape to interrupt
    session.send_key(Key::Escape).unwrap();

    // Wait for the interrupt message — TUI resets to idle
    session
        .wait_for_text("Conversation interrupted", TIMEOUT)
        .expect("Interrupt message did not appear");

    // Resubmit quickly while the old prompt task is still in cancel delay.
    // The sequence we're testing: Aborted (done) → user resubmits →
    // Started (new task) → stale Completed (old task, after delay)
    session.send_str("second message").unwrap();
    session.wait_for_text("second message", TIMEOUT).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // THE CORE ASSERTION: The TUI must enter working state for the second
    // turn. The "esc to interrupt" hint proves the spinner is showing and
    // the TUI considers itself busy. If the stale Completed event from the
    // old task resets working state, this hint will disappear.
    session.wait_for_text("esc to interrupt", TIMEOUT).expect(
        "Second turn did not enter working state — the stale \
             TurnLifecycle::Completed from the first turn's cancelled \
             prompt task likely reset is_task_running to false",
    );

    // Also verify the second turn eventually produces a response
    session
        .wait_for_text("Turn 1 response", TIMEOUT)
        .expect("Second turn response did not appear after entering working state");
}

/// Variant with a longer cancel delay and zero pause between escape and
/// resubmit. Maximizes pressure on the race window.
#[test]
#[cfg(target_os = "linux")]
fn test_escape_then_fast_resubmit_enters_working_state() {
    let config = SessionConfig::new()
        .with_agent_env("MOCK_AGENT_MULTI_TURN_STREAM_UNTIL_CANCEL", "1")
        .with_agent_env("MOCK_AGENT_CANCEL_DELAY_MS", "3000");
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("go").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("First turn did not start");

    // Escape immediately
    session.send_key(Key::Escape).unwrap();
    session
        .wait_for_text("Conversation interrupted", TIMEOUT)
        .expect("Interrupt not shown");

    // Resubmit immediately — maximum race pressure
    session.send_str("fast resubmit").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Must enter working state despite the delayed stale Completed
    session.wait_for_text("esc to interrupt", TIMEOUT).expect(
        "Fast resubmit did not enter working state — race between \
             old Completed and new Started events",
    );

    session
        .wait_for_text("Turn 1 response", TIMEOUT)
        .expect("Fast resubmit response did not appear");
}

/// Run the escape-then-resubmit flow with sacp-tee capturing all ACP wire
/// traffic. This test is `#[ignore]` by default because it requires `sacp-tee`
/// to be installed. Run manually with:
///
///   cargo test -p tui-pty-e2e test_escape_resubmit_with_sacp_tee -- --ignored
///
/// After the test, the JSONL log is printed to stderr for manual inspection.
#[test]
#[ignore]
#[cfg(target_os = "linux")]
fn test_escape_resubmit_with_sacp_tee() {
    let sacp_log = "sacp-tee-escape-test.jsonl";
    let config = SessionConfig::new()
        .with_agent_env("MOCK_AGENT_MULTI_TURN_STREAM_UNTIL_CANCEL", "1")
        .with_agent_env("MOCK_AGENT_CANCEL_DELAY_MS", "2000")
        .with_sacp_tee(sacp_log);
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("sacp-tee test").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("First turn did not start streaming");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    session.send_key(Key::Escape).unwrap();
    session
        .wait_for_text("Conversation interrupted", TIMEOUT)
        .expect("Interrupt not shown");

    session.send_str("after interrupt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Check for working state
    session
        .wait_for_text("esc to interrupt", TIMEOUT)
        .expect("Did not enter working state with sacp-tee");

    session
        .wait_for_text("Turn 1 response", TIMEOUT)
        .expect("Second turn response did not appear with sacp-tee");

    // Read and dump the sacp-tee log for manual inspection
    if let Some(log_content) = session.sacp_tee_log(sacp_log) {
        eprintln!("\n=== sacp-tee wire log ({sacp_log}) ===");
        for line in log_content.lines() {
            eprintln!("  {line}");
        }
        eprintln!("=== end sacp-tee log ===\n");

        assert!(
            !log_content.is_empty(),
            "sacp-tee log is empty — proxy may not have started"
        );
    } else {
        panic!("sacp-tee log file not found — is sacp-tee installed and on PATH?");
    }
}
