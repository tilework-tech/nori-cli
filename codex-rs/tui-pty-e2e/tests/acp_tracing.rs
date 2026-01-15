//! E2E tests for ACP wire protocol tracing via sacp-tee
//!
//! These tests verify that:
//! 1. ACP tracing can be enabled via SessionConfig
//! 2. sacp-tee logs all JSON-RPC messages between client and agent
//! 3. The wire protocol log structure is consistent and correct

use std::time::Duration;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

#[test]
fn test_acp_tracing_enabled_creates_log_file() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_acp_trace_enabled(true);

    let mut session = TuiSession::spawn_with_config(24, 80, config)
        .expect("Failed to spawn session with ACP tracing");

    // Wait for TUI to start
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Verify log file was created
    let log_path = session
        .acp_trace_log_path()
        .expect("ACP trace log path should be available");

    assert!(
        log_path.exists(),
        "ACP trace log file should exist at {}",
        log_path.display()
    );
}

#[test]
fn test_acp_tracing_logs_wire_protocol_messages() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_acp_trace_enabled(true)
        .with_mock_response("Test response from mock agent");

    let mut session = TuiSession::spawn_with_config(24, 80, config)
        .expect("Failed to spawn session with ACP tracing");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt to trigger wire protocol communication
    session.send_str("Test prompt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(tui_pty_e2e::Key::Enter).unwrap();

    // Wait for response from mock agent
    session
        .wait_for_text("Test response from mock agent", TIMEOUT)
        .expect("Should receive response from mock agent");

    // Read the log file
    let log_path = session
        .acp_trace_log_path()
        .expect("ACP trace log path should be available");

    let log_content = std::fs::read_to_string(&log_path).expect("Should be able to read log file");

    // Verify the log contains expected JSON-RPC messages with direction markers
    assert!(
        log_content.contains(r#"→ {"jsonrpc":"2.0","id":0,"method":"initialize""#),
        "Log should contain initialize request, got:\n{}",
        log_content
    );

    assert!(
        log_content.contains(r#"← {"jsonrpc":"2.0","id":0,"result":"#),
        "Log should contain initialize response, got:\n{}",
        log_content
    );

    assert!(
        log_content.contains(r#"→ {"jsonrpc":"2.0","id":1,"method":"session/new""#),
        "Log should contain session/new request, got:\n{}",
        log_content
    );

    assert!(
        log_content.contains(r#"→ {"jsonrpc":"2.0","id":2,"method":"session/prompt""#)
            || log_content.contains(r#"→ {"jsonrpc":"2.0","method":"session/prompt""#),
        "Log should contain session/prompt request, got:\n{}",
        log_content
    );
}

#[test]
fn test_acp_tracing_disabled_by_default() {
    let config = SessionConfig::new().with_model("mock-model".to_owned());
    // Note: NOT calling .with_acp_trace_enabled(true)

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn session");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt
    session.send_str("Test prompt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(tui_pty_e2e::Key::Enter).unwrap();

    // Wait a bit for any potential log file creation
    std::thread::sleep(Duration::from_millis(500));

    // Verify NO log file was created
    assert!(
        session.acp_trace_log_path().is_none(),
        "ACP trace log should not exist when tracing is disabled"
    );
}
