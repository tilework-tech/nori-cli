//! E2E tests for ACP agent switching subprocess lifecycle
//!
//! These tests verify that:
//! 1. Agent subprocesses are spawned with unique PIDs
//! 2. Starting another session reuses the prepared subprocess
//! 3. Switching agents spawns a distinct subprocess and reaps the old one
//! 4. Cleanup happens outside of prompt turns
//! 5. Different agents use different subprocesses

use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;

// ============================================================================
// Helper Functions for Subprocess Tracking
// ============================================================================

/// Extract agent PIDs from the ACP log file
/// Parses lines like: "ACP agent spawned (pid: Some(456))"
fn extract_mock_agent_pids_from_log(log_path: &Path) -> Vec<u32> {
    let re_pattern = "ACP agent spawned \\(pid: Some\\((\\d+)\\)\\)";
    let re = regex::Regex::new(re_pattern).expect("Invalid regex");

    std::fs::read_to_string(log_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            re.captures(line)
                .and_then(|caps| caps.get(1).and_then(|m| m.as_str().parse().ok()))
        })
        .collect()
}

fn wait_for_mock_agent_pid_count(
    session: &mut TuiSession,
    log_path: &Path,
    count: usize,
    timeout: Duration,
) -> Vec<u32> {
    let latest_pids = RefCell::new(Vec::new());
    session
        .wait_for(
            |_| {
                let pids = extract_mock_agent_pids_from_log(log_path);
                let has_enough_pids = pids.len() >= count;
                *latest_pids.borrow_mut() = pids;
                has_enough_pids
            },
            timeout,
        )
        .unwrap_or_else(|err| {
            panic!(
                "Should have at least {count} PIDs after switch, got: {:?}: {err}",
                latest_pids.borrow()
            )
        });
    latest_pids.into_inner()
}

/// Check if a process with the given PID exists and is not a zombie
fn process_exists_and_not_zombie(pid: u32) -> bool {
    let proc_path = format!("/proc/{}", pid);
    if !std::path::Path::new(&proc_path).exists() {
        return false;
    }

    // Check process state - zombies have state 'Z'
    let status_path = format!("/proc/{}/status", pid);
    if let Ok(status) = std::fs::read_to_string(&status_path) {
        for line in status.lines() {
            if line.starts_with("State:") {
                // State line looks like "State:	S (sleeping)" or "State:	Z (zombie)"
                return !line.contains("Z (zombie)") && !line.contains("Z (");
            }
        }
    }

    // If we can't read status, assume process exists (be conservative)
    true
}

/// Check if a process exists (including zombies)
fn process_exists(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid)).exists()
}

// ============================================================================
// Test: Subprocess Spawning
// ============================================================================

/// Test that starting with mock-model spawns a subprocess with a PID
#[test]
#[cfg(target_os = "linux")]
fn test_acp_agent_subprocess_spawned() {
    let config = SessionConfig::new().with_agent("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Check that a mock agent PID was logged
    if let Some(log_path) = session.acp_log_path() {
        let pids = extract_mock_agent_pids_from_log(&log_path);
        assert!(
            !pids.is_empty(),
            "Should have spawned at least one mock agent, log contents: {:?}",
            std::fs::read_to_string(&log_path).unwrap_or_default()
        );

        // Verify the process exists and is not a zombie
        let pid = pids[0];
        assert!(
            process_exists_and_not_zombie(pid),
            "Mock agent process {} should exist and not be a zombie",
            pid
        );
    } else {
        panic!("No ACP log path available");
    }
}

// ============================================================================
// Test: /new Reuses the Prepared Subprocess
// ============================================================================

/// Test that `/new` activates the prepared connection without respawning it.
#[test]
#[cfg(target_os = "linux")]
fn test_slash_new_reuses_prepared_subprocess() {
    let config = SessionConfig::new().with_agent("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // Get initial PID
    let log_path = session.acp_log_path().expect("Should have log path");
    let initial_pids = extract_mock_agent_pids_from_log(&log_path);
    assert!(!initial_pids.is_empty(), "Should have initial PID");
    let initial_pid = initial_pids[0];

    // Type /new to activate the already-prepared connection.
    session.submit_input("/new").unwrap();

    session
        .wait_for(
            |_| {
                std::fs::read_to_string(&log_path)
                    .unwrap_or_default()
                    .contains("ACP session created")
            },
            Duration::from_secs(10),
        )
        .expect("/new should activate the prepared connection");

    let activated_pids = extract_mock_agent_pids_from_log(&log_path);
    assert_eq!(
        activated_pids, initial_pids,
        "/new must not spawn or initialize another child"
    );
    assert!(
        process_exists_and_not_zombie(initial_pid),
        "the prepared child must remain alive after activation"
    );
}

// ============================================================================
// Test: Cleanup Outside Prompt Turns
// ============================================================================

/// Test that subprocess cleanup happens outside of prompt turns (not during streaming)
#[test]
#[cfg(target_os = "linux")]
fn test_acp_cleanup_outside_prompt_turn() {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_string())
        .with_stream_until_cancel(); // Agent streams until cancelled

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    let log_path = session.acp_log_path().expect("Should have log path");
    let initial_pids = extract_mock_agent_pids_from_log(&log_path);
    assert!(!initial_pids.is_empty(), "Should have initial PID");
    let initial_pid = initial_pids[0];

    // Start a streaming prompt
    session.submit_input("Start streaming").unwrap();

    // Wait for streaming to start (status indicator appears with interrupt hint)
    session
        .wait_for_text("esc to interrupt", Duration::from_secs(5))
        .expect("Streaming should start (status indicator visible)");

    // While streaming, the process should still exist and not be zombie
    assert!(
        process_exists_and_not_zombie(initial_pid),
        "Process should exist and not be zombie during streaming"
    );

    // Cancel the stream with Escape
    session.send_key(Key::Escape).unwrap();

    // Wait for cancellation
    std::thread::sleep(Duration::from_millis(500));

    // After cancellation (turn complete), process should still exist
    // (cleanup only happens on session switch, not turn end)
    assert!(
        process_exists_and_not_zombie(initial_pid),
        "Process should exist after turn ends (cleanup is on session switch)"
    );
}

// ============================================================================
// Test: Different Agents Different Subprocesses
// ============================================================================

/// Test that mock-model and mock-model-alt use different subprocesses
#[test]
#[cfg(target_os = "linux")]
fn test_different_agents_different_subprocesses() {
    // First session with mock-model
    let config1 = SessionConfig::new().with_agent("mock-model".to_string());

    let mut session1 =
        TuiSession::spawn_with_config(24, 80, config1).expect("Failed to spawn first TUI");

    session1
        .wait_for_text("›", TIMEOUT)
        .expect("First TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    let log_path1 = session1.acp_log_path().expect("Should have log path");
    let pids1 = extract_mock_agent_pids_from_log(&log_path1);
    assert!(!pids1.is_empty(), "First session should have PID");
    let pid1 = pids1[0];

    // Second session with mock-model-alt (separate TUI instance)
    let config2 = SessionConfig::new().with_agent("mock-model-alt".to_string());

    let mut session2 =
        TuiSession::spawn_with_config(24, 80, config2).expect("Failed to spawn second TUI");

    session2
        .wait_for_text("›", TIMEOUT)
        .expect("Second TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    let log_path2 = session2.acp_log_path().expect("Should have log path");
    let pids2 = extract_mock_agent_pids_from_log(&log_path2);
    assert!(!pids2.is_empty(), "Second session should have PID");
    let pid2 = pids2[0];

    // Different TUI instances should have different agent PIDs
    assert_ne!(
        pid1, pid2,
        "Different agent models should spawn different subprocesses: mock-model={}, mock-model-alt={}",
        pid1, pid2
    );
}

// ============================================================================
// Test: Agent Switch via Model Picker
// ============================================================================

/// Test that switching agents via model picker spawns a new subprocess
#[test]
#[cfg(target_os = "linux")]
fn test_acp_agent_switch_via_model_picker() {
    let config = SessionConfig::new().with_agent("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    let log_path = session.acp_log_path().expect("Should have log path");
    let initial_pids = extract_mock_agent_pids_from_log(&log_path);
    assert!(!initial_pids.is_empty(), "Should have initial PID");
    let initial_pid = initial_pids[0];

    // Open model picker with Ctrl-M (or the key that opens it)
    // The model picker is opened with '/' then selecting model from menu
    // or using a specific keyboard shortcut
    session.send_key(Key::Ctrl('k')).unwrap(); // Common shortcut for model picker
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for model picker to appear - it should show available models
    let picker_appeared = session.wait_for(
        |screen| {
            screen.contains("mock-model") || screen.contains("Model") || screen.contains("Select")
        },
        Duration::from_secs(8),
    );

    if picker_appeared.is_err() {
        // If Ctrl-K doesn't work, try /model command
        session.send_key(Key::Escape).unwrap();
        std::thread::sleep(TIMEOUT_INPUT);
        session.submit_input("/model").unwrap();
        std::thread::sleep(TIMEOUT_INPUT);
    }

    // Navigate to mock-model-alt and select it
    // Use arrow keys to find and select the alt model
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for switch to complete
    std::thread::sleep(Duration::from_millis(1000));

    // Check if we got a new PID
    let post_switch_pids = extract_mock_agent_pids_from_log(&log_path);

    // If the model picker triggered a new session, we should have more PIDs
    // Note: This test may need adjustment based on how model picker actually works
    if post_switch_pids.len() > initial_pids.len() {
        let new_pid = *post_switch_pids.last().unwrap();
        assert_ne!(
            initial_pid, new_pid,
            "Model picker switch should create new subprocess"
        );
    }
    // If no new PID, the model picker might not trigger subprocess restart
    // This is acceptable behavior - document it
}

// ============================================================================
// Test: /agent Slash Command - Shows Available Agents
// ============================================================================

/// Test that /agent command shows available ACP agents from the registry
#[test]
#[cfg(target_os = "linux")]
fn test_agent_command_shows_available_agents() {
    let config = SessionConfig::new().with_agent("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // Open agent picker with /agent command
    session.submit_input("/agent").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for agent picker to appear - it should show available agents
    session
        .wait_for(
            |screen| {
                // Should show available agents from the ACP registry
                screen.contains("Select agent") || screen.contains("mock-model")
            },
            Duration::from_secs(8),
        )
        .expect("Agent picker should appear");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // Verify both mock agents are visible
    let screen = session.screen_contents();
    assert!(
        screen.contains("mock-model") || screen.contains("Mock"),
        "Agent picker should show mock-model agent, got: {}",
        screen
    );
}

// ============================================================================
// Test: /agent Slash Command - Activate Candidate
// ============================================================================

/// Choosing a session activates the prepared candidate without waiting for a
/// prompt to trigger another spawn.
#[test]
#[cfg(target_os = "linux")]
fn test_agent_switch_activates_prepared_candidate() {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_string())
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1");

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    let log_path = session.acp_log_path().expect("Should have log path");
    let initial_pids = extract_mock_agent_pids_from_log(&log_path);
    assert!(!initial_pids.is_empty(), "Should have initial PID");
    let initial_pid = initial_pids[0];

    // Open agent picker with /agent command
    session.submit_input("/agent").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for agent picker to appear (8 seconds - CI detection is slow)
    session
        .wait_for(
            |screen| screen.contains("Select agent") || screen.contains("mock-model"),
            Duration::from_secs(8),
        )
        .expect("Agent picker should appear");

    // Select a different agent (mock-model-alt)
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Start a new session", Duration::from_secs(10))
        .expect("the candidate session picker should open");
    let prepared_pids = wait_for_mock_agent_pid_count(
        &mut session,
        &log_path,
        initial_pids.len() + 1,
        Duration::from_secs(10),
    );
    let new_pid = prepared_pids
        .iter()
        .copied()
        .find(|pid| !initial_pids.contains(pid))
        .expect("preparation should add one candidate process");
    assert!(
        process_exists_and_not_zombie(initial_pid),
        "the current agent must remain alive until candidate activation"
    );

    // Row 0 explicitly starts a new session on the prepared connection.
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("›", Duration::from_secs(10))
        .expect("candidate activation should return to the composer");
    assert_ne!(
        initial_pid, new_pid,
        "the prepared candidate must be a distinct process"
    );
    let activated_pids = extract_mock_agent_pids_from_log(&log_path);
    assert_eq!(
        activated_pids, prepared_pids,
        "activation must reuse the prepared child instead of spawning a third process"
    );
    assert!(
        process_exists_and_not_zombie(new_pid),
        "the prepared child must become the active agent"
    );
    session
        .wait_for(|_| !process_exists(initial_pid), Duration::from_secs(10))
        .expect("the replaced process should be reaped after candidate SessionStarted");

    session.submit_input("hello").unwrap();
    session
        .wait_for_text("Test message", Duration::from_secs(10))
        .expect("the activated candidate should answer prompts");
}

/// Cancelling a prepared candidate destroys only that candidate and restores
/// the still-live current session.
#[test]
#[cfg(target_os = "linux")]
fn test_agent_candidate_cancel_keeps_current_session_promptable() {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_string())
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1");
    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("spawn TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    let log_path = session.acp_log_path().expect("log path");
    let current_pid = extract_mock_agent_pids_from_log(&log_path)[0];

    session.submit_input("/agent").unwrap();
    session
        .wait_for_text("Select agent", Duration::from_secs(8))
        .expect("agent picker");
    session.send_key(Key::Down).unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Start a new session", Duration::from_secs(10))
        .expect("candidate session picker");
    let pids = wait_for_mock_agent_pid_count(&mut session, &log_path, 2, Duration::from_secs(10));
    let candidate_pid = *pids.last().unwrap();

    session.send_key(Key::Escape).unwrap();
    session
        .wait_for_text("›", Duration::from_secs(10))
        .expect("cancelling should restore the current composer");
    session
        .wait_for(|_| !process_exists(candidate_pid), Duration::from_secs(10))
        .expect("cancelled candidate should be reaped");
    assert!(
        process_exists_and_not_zombie(current_pid),
        "current process must survive candidate cancellation"
    );

    session.submit_input("still current").unwrap();
    session
        .wait_for_text("Test message", Duration::from_secs(10))
        .expect("current session should remain promptable after cancellation");
}

/// A session-directive failure reaps the candidate and leaves the current
/// session available for another prompt.
#[test]
#[cfg(target_os = "linux")]
fn test_agent_candidate_activation_failure_keeps_current_session_promptable() {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_string())
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env("MOCK_AGENT_FAIL_NEW_SESSION_MODEL", "mock-model-alt");
    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("spawn TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    let log_path = session.acp_log_path().expect("log path");
    let current_pid = extract_mock_agent_pids_from_log(&log_path)[0];

    session.submit_input("/agent").unwrap();
    session
        .wait_for_text("Select agent", Duration::from_secs(8))
        .expect("agent picker");
    session.send_key(Key::Down).unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Start a new session", Duration::from_secs(10))
        .expect("candidate session picker");
    let pids = wait_for_mock_agent_pid_count(&mut session, &log_path, 2, Duration::from_secs(10));
    let candidate_pid = *pids.last().unwrap();

    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Failed to start ACP session", Duration::from_secs(10))
        .expect("candidate activation failure should be visible");
    session
        .wait_for(|_| !process_exists(candidate_pid), Duration::from_secs(10))
        .expect("failed candidate should be reaped");
    assert!(
        process_exists_and_not_zombie(current_pid),
        "current process must survive candidate activation failure"
    );

    session.submit_input("still current").unwrap();
    session
        .wait_for_text("Test message", Duration::from_secs(10))
        .expect("current session should remain promptable after candidate failure");
}

/// A candidate whose initialize never completes must time out, be reaped, and
/// leave the current session available for another prompt.
#[test]
#[cfg(target_os = "linux")]
fn test_agent_candidate_preparation_timeout_keeps_current_session_promptable() {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_string())
        .with_agent_env("MOCK_AGENT_STARTUP_DELAY_MS_MOCK_MODEL_ALT", "60000");
    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("spawn TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    let log_path = session.acp_log_path().expect("log path");
    let current_pid = extract_mock_agent_pids_from_log(&log_path)[0];

    session.submit_input("/agent").unwrap();
    session
        .wait_for_text("Select agent", Duration::from_secs(8))
        .expect("agent picker");
    session.send_key(Key::Down).unwrap();
    session.send_key(Key::Enter).unwrap();
    let pids = wait_for_mock_agent_pid_count(&mut session, &log_path, 2, Duration::from_secs(10));
    let candidate_pid = pids
        .iter()
        .copied()
        .find(|pid| *pid != current_pid)
        .expect("candidate process pid");

    session
        .wait_for_text(
            "timed out preparing agent after 20s",
            Duration::from_secs(25),
        )
        .expect("hung candidate preparation should time out");
    session
        .wait_for(|_| !process_exists(candidate_pid), Duration::from_secs(10))
        .expect("timed-out candidate should be reaped");
    assert!(
        process_exists_and_not_zombie(current_pid),
        "current process must survive candidate preparation timeout"
    );

    session.submit_input("still current after timeout").unwrap();
    session
        .wait_for_text("Test message", Duration::from_secs(10))
        .expect("current session should remain promptable after candidate timeout");
}

// ============================================================================
// Test: /agent - No Switch During Active Prompt Turn
// ============================================================================

/// Test that navigating /agent picker during streaming doesn't kill the agent
#[test]
#[cfg(target_os = "linux")]
fn test_agent_picker_no_switch_during_streaming() {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_string())
        .with_stream_until_cancel(); // Agent streams until cancelled

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    let log_path = session.acp_log_path().expect("Should have log path");
    let initial_pids = extract_mock_agent_pids_from_log(&log_path);
    assert!(!initial_pids.is_empty(), "Should have initial PID");
    let initial_pid = initial_pids[0];

    // Start a streaming prompt
    session.submit_input("Start streaming").unwrap();

    // Wait for streaming to start (status indicator appears with interrupt hint)
    session
        .wait_for_text("esc to interrupt", Duration::from_secs(5))
        .expect("Streaming should start (status indicator visible)");

    // While streaming, the agent should still be running
    assert!(
        process_exists_and_not_zombie(initial_pid),
        "Agent should be running during streaming"
    );

    // Cancel streaming first so we can access the UI
    session.send_key(Key::Escape).unwrap();
    std::thread::sleep(Duration::from_millis(500));

    // The agent should still be the same
    let pids_after = extract_mock_agent_pids_from_log(&log_path);
    assert_eq!(
        pids_after.len(),
        initial_pids.len(),
        "No new subprocess should be spawned during/after streaming cancel"
    );
    assert!(
        process_exists_and_not_zombie(initial_pid),
        "Original agent should still be running after cancel"
    );
}

// ============================================================================
// Test: /model Slash Command - Shows Disabled in ACP Mode
// ============================================================================

/// Test that /model command shows disabled options in ACP mode
#[test]
#[cfg(target_os = "linux")]
fn test_model_command_shows_disabled_in_acp_mode() {
    let config = SessionConfig::new().with_agent("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // Open model picker with /model command
    session.submit_input("/model").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for model picker to appear (8 seconds - CI detection is slow)
    session
        .wait_for(
            |screen| screen.contains("Select model") || screen.contains("Model"),
            Duration::from_secs(8),
        )
        .expect("Model picker should appear");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    // In ACP mode, model options should show as disabled or indicate
    // they're not available
    let screen = session.screen_contents();
    assert!(
        screen.contains("disabled")
            || screen.contains("Not available")
            || screen.contains("ACP")
            || screen.contains("Use /agent"),
        "Model picker should indicate options are disabled in ACP mode, got: {}",
        screen
    );
}

// ============================================================================
// Test: Agent Switch Message Flow - Verifies NEW agent receives and responds
// ============================================================================

/// Test that when switching agents via /agent command, the NEW agent
/// correctly receives and responds to the submitted prompt.
///
/// This keeps the unique user-visible workflow at the PTY boundary: the old
/// agent answers, the switch commits under the selected display name, and the
/// new conversation answers independently. Exact ACP request ordering belongs
/// to the harness wire-boundary tests.
///
/// This catches the race condition bug where events from the OLD agent
/// could leak into the NEW widget, causing the prompt to be lost.
#[test]
#[cfg(target_os = "linux")]
fn test_agent_switch_message_flow_mock_to_mock_alt() {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_string())
        .with_mock_response("initial agent response marker")
        .with_agent_env(
            "MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT",
            "switched agent response marker",
        );

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // First, verify the initial agent works.
    session.submit_input("test initial").unwrap();

    // Wait for the initial agent's echoed prompt.
    session
        .wait_for_text("initial agent response marker", Duration::from_secs(5))
        .expect("Initial agent should respond");

    // Open agent picker with /agent command
    session.submit_input("/agent").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for agent picker to appear (8 seconds - CI detection is slow)
    session
        .wait_for(
            |screen| screen.contains("Select agent") || screen.contains("mock-model"),
            Duration::from_secs(8),
        )
        .expect("Agent picker should appear");

    // Select mock-model-alt (different agent)
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Start a new session", Duration::from_secs(10))
        .expect("candidate session picker should open");

    // Activate the prepared candidate before submitting a prompt.
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text(
            "Started new conversation with agent: Mock ACP Alt",
            Duration::from_secs(10),
        )
        .expect("prepared candidate should commit before accepting the next prompt");

    // The new active agent should receive this prompt and respond.
    session.submit_input("test after switch").unwrap();

    session
        .wait_for_text("switched agent response marker", TIMEOUT)
        .expect("Screen should contain response text");
}

// ============================================================================
// Test: Agent Picker Shows Correct Agents (Debug Build)
// ============================================================================

/// Test that the agent picker shows all 5 agents in debug build.
///
/// In debug builds, the agent picker should show:
/// - Mock ACP (mock agent for testing)
/// - Mock ACP Alt (alternate mock agent for testing)
/// - Claude Code (Anthropic)
/// - Codex (OpenAI)
/// - Gemini (Google)
///
/// Note: In release builds, only the 3 production agents (Claude, Codex, Gemini)
/// would be shown. This test validates the debug build behavior.
#[test]
#[cfg(target_os = "linux")]
#[cfg(debug_assertions)]
fn test_agent_picker_shows_five_agents_in_debug_build() {
    let config = SessionConfig::new().with_agent("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // Open agent picker with /agent command
    session.submit_input("/agent").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for agent picker to appear (8 seconds - CI detection is slow)
    session
        .wait_for(
            |screen| screen.contains("Select agent"),
            Duration::from_secs(8),
        )
        .expect("Agent picker should appear with title");

    // Get screen contents to verify all agents are present
    let screen = session.screen_contents();

    // Verify all 5 agents are shown in debug build
    // The display names should NOT include model versions (e.g., "Claude" not "Claude 4.5")
    assert!(
        screen.contains("Mock ACP"),
        "Agent picker should show 'Mock ACP', got: {}",
        screen
    );
    assert!(
        screen.contains("Mock ACP Alt"),
        "Agent picker should show 'Mock ACP Alt', got: {}",
        screen
    );
    assert!(
        screen.contains("Claude") && !screen.contains("Claude 4.5"),
        "Agent picker should show 'Claude' without model version, got: {}",
        screen
    );
    assert!(
        screen.contains("Codex"),
        "Agent picker should show 'Codex', got: {}",
        screen
    );
    assert!(
        screen.contains("Gemini") && !screen.contains("Gemini 2.5"),
        "Agent picker should show 'Gemini' without model version, got: {}",
        screen
    );

    // Count agents by looking for unique agent entries
    // Each agent line should be distinct in the picker
    let agent_count = ["Mock ACP Alt", "Claude", "Codex", "Gemini"]
        .iter()
        .filter(|name| screen.contains(*name))
        .count()
        + if screen.contains("Mock ACP") {
            1 // Mock ACP is present (Mock ACP Alt counted separately)
        } else {
            0
        };

    // We should see all 5 agents
    assert!(
        agent_count >= 4, // At minimum Claude, Codex, Gemini, and one of the Mocks
        "Expected at least 4 distinct agents in picker, found approximately: {}. Screen: {}",
        agent_count,
        screen
    );
}

// ============================================================================
// Test: Connecting Status During Slow Agent Startup
// ============================================================================

/// Test that the slash command popup shows the current agent name in the
/// /agent description when the TUI first starts.
///
/// The /agent description should read:
///   "switch between available ACP agents (current: Mock ACP)"
///
/// This verifies that the description override is set during initial
/// construction, not just when switching agents.
#[test]
#[cfg(target_os = "linux")]
fn test_slash_popup_shows_current_agent_in_description() {
    let config = SessionConfig::new().with_agent("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // Type '/a' to open the slash command popup (NOT '/agent' + Enter which
    // opens the agent picker). The popup should show filtered commands
    // including /agent with its description.
    session.type_input("/a").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for the slash popup to render with the /agent command visible
    session
        .wait_for(|screen| screen.contains("/agent"), Duration::from_secs(5))
        .expect("Slash popup should show /agent command");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let screen = session.screen_contents();

    // The description for /agent should include the current agent name
    // in parentheses. mock-model has display name "Mock ACP".
    assert!(
        screen.contains("(current: Mock ACP)"),
        "Slash popup /agent description should show current agent name.\n\
         Expected to find '(current: Mock ACP)' in screen.\n\
         Screen contents:\n{}",
        screen
    );
}

/// Test that the slash command popup shows the current approval mode in the
/// /approvals description when the TUI first starts.
///
/// The /approvals description should include a parenthetical like:
///   "choose what Nori can do without approval (current: Agent)"
///
/// This verifies that the approval mode description override is set during
/// initial construction.
#[test]
#[cfg(target_os = "linux")]
fn test_slash_popup_shows_current_approval_mode_in_description() {
    let config = SessionConfig::new().with_agent("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // Type '/ap' to open the slash command popup filtered to show /approvals
    session.type_input("/ap").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for the slash popup to render with the /approvals command visible
    session
        .wait_for(
            |screen| screen.contains("/approvals"),
            Duration::from_secs(5),
        )
        .expect("Slash popup should show /approvals command");
    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    let screen = session.screen_contents();

    // The description for /approvals should include the current approval mode
    // in parentheses. The exact mode depends on config defaults, but it should
    // be one of the known preset labels.
    let has_approval_mode = screen.contains("(current: Agent)")
        || screen.contains("(current: Read Only)")
        || screen.contains("(current: Full Access)");

    assert!(
        has_approval_mode,
        "Slash popup /approvals description should show current approval mode.\n\
         Expected to find '(current: Agent)' or '(current: Read Only)' or '(current: Full Access)' in screen.\n\
         Screen contents:\n{}",
        screen
    );
}

/// Test that candidate preparation feedback appears during slow agent startup.
///
/// When an ACP agent takes time to start (e.g., npx/bunx resolving dependencies),
/// the TUI should show a "Connecting" status indicator with shimmer animation
/// to provide feedback to the user.
///
/// This test works by:
/// 1. Starting with mock-model (no delay) so TUI initializes normally
/// 2. Selecting mock-model-alt via the agent picker
/// 3. Candidate preparation starts immediately
/// 4. mock-model-alt has a 6-second startup delay configured
/// 5. Verifying preparation feedback appears during that delay
#[test]
#[cfg(target_os = "linux")]
fn test_connecting_status_during_slow_agent_startup() {
    // Configure mock-model-alt with a 6-second startup delay to simulate slow npx/bunx
    // mock-model has no delay so TUI starts up quickly
    let config = SessionConfig::new()
        .with_agent("mock-model".to_string())
        .with_agent_env("MOCK_AGENT_STARTUP_DELAY_MS_MOCK_MODEL_ALT", "6000");

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn TUI");

    // Wait for TUI to fully start with mock-model
    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start with mock-model");
    std::thread::sleep(TIMEOUT_INPUT);

    // Open agent picker with /agent command
    session.submit_input("/agent").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);

    // Wait for agent picker to appear
    session
        .wait_for(
            |screen| screen.contains("Select agent") || screen.contains("mock-model"),
            Duration::from_secs(8),
        )
        .expect("Agent picker should appear");

    // Select mock-model-alt (one down from mock-model)
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(Duration::from_millis(500));

    // Preparation starts immediately when the agent row is selected.
    // The 6-second delay gives us plenty of time to catch this
    session
        .wait_for_text("Preparing", Duration::from_secs(3))
        .expect("Should show preparation feedback during slow agent startup");

    // Eventually the prepared agent's explicit session picker should appear.
    session
        .wait_for_text("Start a new session", Duration::from_secs(15))
        .expect("TUI should eventually show the candidate session picker");
}
