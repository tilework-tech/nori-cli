use insta::assert_snapshot;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

#[test]
#[cfg(target_os = "linux")]
fn test_submit_text() {
    let config = SessionConfig::new().with_stream_until_cancel();
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit prompt
    session.send_str("testing!!!").unwrap();
    session.wait_for_text("testing!!!", TIMEOUT).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    std::thread::sleep(TIMEOUT_INPUT);
    session.wait_for_text("? for shortcuts", TIMEOUT).unwrap();

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);
    assert_snapshot!(
        "submit_input",
        normalize_for_input_snapshot(session.screen_contents())
    );
}

#[test]
#[cfg(target_os = "linux")]
fn test_escape_cancels_streaming() {
    // Use git_init to prevent "Snapshots disabled" from racing with "Working" status
    let config = SessionConfig::new().with_stream_until_cancel();
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    // Wait for the prompt to appear (indicated by the chevron character)
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit prompt
    session.send_str("testing!!!").unwrap();
    session.wait_for_text("testing!!!", TIMEOUT).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for streaming to start
    session
        .wait_for_text("Working", TIMEOUT)
        .expect("Conversation did not start");

    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Escape).unwrap();
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Verify cancellation completed
    // (exact behavior depends on TUI implementation)
    session
        .wait_for_text(
            "Conversation interrupted - tell the model what to do differently",
            TIMEOUT,
        )
        .expect("No interrupt reported");

    // There are timing issues for when the "Streaming..." chunk shows up,
    // that make a snapshot here very flaky. Rely on the above assert for now
    // assert_snapshot!(
    //     "escape_cancelled_stream",
    //     normalize_for_input_snapshot(session.screen_contents())
    // )
}

#[test]
#[cfg(target_os = "linux")]
fn test_ctrl_c_cancels_streaming() {
    // Use git_init to prevent "Snapshots disabled" from racing with "Working" status
    let config = SessionConfig::new().with_stream_until_cancel();
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    // Wait for the prompt to appear (indicated by the chevron character)
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Submit prompt
    session.send_str("testing!!!").unwrap();
    session.wait_for_text("testing!!!", TIMEOUT).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for streaming to start
    session
        .wait_for_text("Working", TIMEOUT)
        .expect("Conversation did not start");

    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Ctrl('c')).unwrap();
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Verify cancellation completed
    // (exact behavior depends on TUI implementation)
    session
        .wait_for_text(
            "Conversation interrupted - tell the model what to do differently",
            TIMEOUT,
        )
        .expect("No interrupt reported");

    // There are timing issues for when the "Streaming..." chunk shows up,
    // that make a snapshot here very flaky. Rely on the above assert for now
    // assert_snapshot!(
    //     "ctrl_c_cancelled_stream",
    //     normalize_for_input_snapshot(session.screen_contents())
    // )
}

// @current-session
#[test]
#[cfg(target_os = "linux")]
fn test_status_displays_token_usage_from_session_transcript() {
    // Create SessionConfig with mock-model (treated as Claude for discovery)
    let config = SessionConfig::new().with_mock_response("Test response");
    let mut session = TuiSession::spawn_with_config(24, 80, config).unwrap();

    // Wait for prompt to appear
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt did not appear");
    std::thread::sleep(TIMEOUT_INPUT);

    // Get NORI_HOME path and create Claude session transcript structure
    let nori_home = session
        .nori_home_path()
        .expect("nori_home should exist in test");

    // Mock Agent uses session ID "0" for the first session
    let session_id = "0";

    // Create Claude projects directory structure
    // Claude stores transcripts at ~/.claude/projects/<project_path>/<session_id>.jsonl
    // where project_path is cwd with / replaced by -
    let cwd = nori_home.clone(); // In tests, cwd == NORI_HOME
    let project_path = cwd.to_string_lossy().replace('/', "-");
    let claude_projects_dir = nori_home
        .join(".claude")
        .join("projects")
        .join(&project_path);
    std::fs::create_dir_all(&claude_projects_dir).expect("create claude projects dir");

    // Copy the Claude session fixture to the expected transcript path
    let transcript_path = claude_projects_dir.join(format!("{session_id}.jsonl"));
    let fixture_content = include_str!("../../acp/tests/fixtures/session-claude.jsonl");
    std::fs::write(&transcript_path, fixture_content).expect("write transcript fixture");

    // Send a prompt to establish the session
    session.send_str("test prompt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for response
    session
        .wait_for_text("Test response", TIMEOUT)
        .expect("Should receive mock response");

    // Wait for prompt to return
    session
        .wait_for_text("›", TIMEOUT)
        .expect("Prompt should return");
    std::thread::sleep(TIMEOUT_INPUT);

    // Send /status command
    session.send_str("/status").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for Token Usage to appear (async operation)
    session
        .wait_for_text("Token Usage", TIMEOUT)
        .expect("Should show Token Usage header after transcript parsing");

    // Give it a moment to fully render
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Verify token usage details appear in output
    let screen = session.screen_contents();

    assert!(
        screen.contains("input:"),
        "Should show input tokens label, got:\n{}",
        screen
    );
    assert!(
        screen.contains("output:"),
        "Should show output tokens label, got:\n{}",
        screen
    );
    assert!(
        screen.contains("total:"),
        "Should show total tokens label, got:\n{}",
        screen
    );
}
