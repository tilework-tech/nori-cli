Agent Switching E2E Tests Implementation Plan

Goal: Create E2E tests that verify ACP agent subprocess lifecycle during agent switching - ensuring subprocesses are
spawned correctly, cleaned up properly, and that switching creates new subprocesses.

Architecture: The tests will use the existing tui-pty-e2e test infrastructure with a second mock agent registration
(mock-model-alt). The mock agent will emit its PID to stderr in a parseable format. Tests will capture PIDs, trigger
agent switches via TUI interactions, and verify subprocess lifecycle using /proc filesystem checks.

Tech Stack: Rust, tui-pty-e2e framework, mock-acp-agent, /proc filesystem for Linux process verification

---
Testing Plan

I will add E2E tests in a new file agent_switching.rs in the tui-pty-e2e/tests/ directory that:

1. Test subprocess spawning: Verify that starting with mock-model spawns a subprocess and the PID is logged
2. Test agent switch creates new subprocess: Switch from mock-model to mock-model-alt, verify different PIDs
3. Test old subprocess cleanup: After switching, verify old PID no longer exists in /proc
4. Test cleanup timing: Verify subprocess termination happens outside of prompt turns (not during streaming)
5. Test no subprocess reuse: Verify different agents use different subprocesses (not the same process serving multiple
agents)

The tests will:
- Parse stderr output from the ACP tracing logs for PID information
- Use /proc/{pid} existence checks to verify process lifecycle
- Use the mock agent's configurable environment variables to control behavior

NOTE: I will write all tests before I add any implementation behavior.

---
Phase 1: Registry Modification - Add Second Mock Agent

Step 1.1: Add mock-model-alt to ACP registry

File: /home/clifford/Documents/source/nori/cli/.worktrees/e2e-agent-switching-tests/codex-rs/acp/src/registry.rs

Add a new match arm in get_agent_config() (after line 113) for mock-model-alt:

"mock-model-alt" => {
    // Same binary as mock-model but different provider_slug
    // to ensure tests can distinguish between different agent configurations
    let exe_path = /* same resolution logic as mock-model */;
    Ok(AcpAgentConfig {
        provider_slug: "mock-acp-alt".to_string(),  // Different slug!
        command: exe_path.to_string_lossy().to_string(),
        args: vec![],
        provider_info: AcpProviderInfo {
            name: "Mock ACP Alt".to_string(),
            ..Default::default()
        },
    })
}

Key detail: The provider_slug MUST be different (mock-acp-alt vs mock-acp) - this is what distinguishes the agents and
prevents subprocess reuse optimization (if implemented later).

Step 1.2: Add unit test for new registry entry

File: /home/clifford/Documents/source/nori/cli/.worktrees/e2e-agent-switching-tests/codex-rs/acp/src/registry.rs

Add test in the mod tests block:

#[test]
fn test_get_mock_model_alt_config() {
    let config = get_agent_config("mock-model-alt")
        .expect("Should return config for mock-model-alt");

    assert_eq!(config.provider_slug, "mock-acp-alt");
    assert!(config.command.contains("mock_acp_agent"));
    assert_eq!(config.provider_info.name, "Mock ACP Alt");
}

Step 1.3: Run registry tests to verify

Command:
cd /home/clifford/Documents/source/nori/cli/.worktrees/e2e-agent-switching-tests/codex-rs && cargo test -p codex-acp
registry

Expected output: All registry tests pass including the new test_get_mock_model_alt_config.

---
Phase 2: Mock Agent PID Emission

Step 2.1: Add PID emission to mock agent

File: /home/clifford/Documents/source/nori/cli/.worktrees/e2e-agent-switching-tests/codex-rs/mock-acp-agent/src/main.rs

In the main() function (around line 461), after env_logger::init(), add:

// Emit PID for E2E test subprocess tracking
eprintln!("MOCK_AGENT_PID:{}", std::process::id());

This creates a parseable line in stderr that tests can grep for.

Step 2.2: Verify mock agent build and PID emission

Command:
cd /home/clifford/Documents/source/nori/cli/.worktrees/e2e-agent-switching-tests/codex-rs && cargo build -p
mock-acp-agent

Manual verification (optional):
./target/debug/mock_acp_agent 2>&1 | head -1
# Should output: MOCK_AGENT_PID:12345

---
Phase 3: E2E Test Infrastructure Helpers

Step 3.1: Add PID extraction helper to tui-pty-e2e

File: /home/clifford/Documents/source/nori/cli/.worktrees/e2e-agent-switching-tests/codex-rs/tui-pty-e2e/src/lib.rs

Add a helper function near the end of the file:

/// Extract agent PIDs from the ACP log file
/// Returns all PIDs found in MOCK_AGENT_PID:NNNN lines
pub fn extract_mock_agent_pids_from_log(log_path: &Path) -> Vec<u32> {
    std::fs::read_to_string(log_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            if let Some(pid_str) = line.strip_prefix("MOCK_AGENT_PID:") {
                pid_str.trim().parse().ok()
            } else {
                None
            }
        })
        .collect()
}

/// Check if a process with the given PID exists
pub fn process_exists(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid)).exists()
}

Step 3.2: Expose temp_dir log path in TuiSession

File: /home/clifford/Documents/source/nori/cli/.worktrees/e2e-agent-switching-tests/codex-rs/tui-pty-e2e/src/lib.rs

Add a public method to TuiSession:

impl TuiSession {
    /// Get the path to the ACP log file (if temp directory exists)
    pub fn acp_log_path(&self) -> Option<PathBuf> {
        self._temp_dir.as_ref().map(|d| d.path().join(".codex-acp.log"))
    }
}

---
Phase 4: E2E Tests for Agent Switching

Step 4.1: Create new test file

File: /home/clifford/Documents/source/nori/cli/.worktrees/e2e-agent-switching-tests/codex-rs/tui-pty-e2e/tests/agent_swi
tching.rs

Create the test file with module setup:

//! E2E tests for ACP agent switching subprocess lifecycle
//!
//! These tests verify that:
//! 1. Agent subprocesses are spawned with unique PIDs
//! 2. Switching agents spawns new subprocesses (different PIDs)
//! 3. Old subprocesses are cleaned up after switching
//! 4. Cleanup happens outside of prompt turns

use std::time::Duration;
use tui_pty_e2e::{
    SessionConfig, TuiSession, TIMEOUT, TIMEOUT_INPUT,
    extract_mock_agent_pids_from_log, process_exists,
};

Step 4.2: Write test - agent subprocess spawning

Add first test:

/// Test that starting with mock-model spawns a subprocess with a PID
#[test]
fn test_acp_agent_subprocess_spawned() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config)
        .expect("Failed to spawn TUI");

    // Wait for startup
    session.wait_for_text("›", TIMEOUT)
        .expect("TUI should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Check that a mock agent PID was logged
    if let Some(log_path) = session.acp_log_path() {
        let pids = extract_mock_agent_pids_from_log(&log_path);
        assert!(!pids.is_empty(), "Should have spawned at least one mock agent");

        // Verify the process exists
        let pid = pids[0];
        assert!(process_exists(pid), "Mock agent process {} should exist", pid);
    }
}

Step 4.3: Write test - agent switch creates new subprocess

/// Test that switching agents spawns a NEW subprocess with a DIFFERENT PID
#[test]
fn test_acp_agent_switch_creates_new_subprocess() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config)
        .expect("Failed to spawn TUI");

    // Wait for startup
    session.wait_for_text("›", TIMEOUT)
        .expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // Get initial PID
    let log_path = session.acp_log_path().expect("Should have log path");
    let initial_pids = extract_mock_agent_pids_from_log(&log_path);
    assert!(!initial_pids.is_empty(), "Should have initial PID");
    let initial_pid = initial_pids[0];

    // Type /new to start a new session (this triggers agent switch)
    session.send_str("/new").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(tui_pty_e2e::Key::Enter).unwrap();

    // Wait for new session to start
    session.wait_for_text("›", Duration::from_secs(10))
        .expect("New session should start");
    std::thread::sleep(Duration::from_millis(500));

    // Get PIDs after switch
    let post_switch_pids = extract_mock_agent_pids_from_log(&log_path);
    assert!(
        post_switch_pids.len() >= 2,
        "Should have at least 2 PIDs after switch, got: {:?}",
        post_switch_pids
    );

    let new_pid = post_switch_pids.last().unwrap();
    assert_ne!(
        initial_pid, *new_pid,
        "New session should have different PID: initial={}, new={}",
        initial_pid, new_pid
    );
}

Step 4.4: Write test - old subprocess cleanup

/// Test that the old subprocess is cleaned up after switching
#[test]
fn test_acp_agent_old_subprocess_cleanup() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_string());

    let mut session = TuiSession::spawn_with_config(24, 80, config)
        .expect("Failed to spawn TUI");

    session.wait_for_text("›", TIMEOUT).expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    let log_path = session.acp_log_path().expect("Should have log path");
    let initial_pids = extract_mock_agent_pids_from_log(&log_path);
    let initial_pid = initial_pids[0];

    // Verify initial process exists
    assert!(process_exists(initial_pid), "Initial process should exist");

    // Trigger session switch
    session.send_str("/new").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(tui_pty_e2e::Key::Enter).unwrap();

    // Wait for new session
    session.wait_for_text("›", Duration::from_secs(10))
        .expect("New session should start");

    // Give cleanup time to happen
    std::thread::sleep(Duration::from_millis(1000));

    // Old process should be gone
    assert!(
        !process_exists(initial_pid),
        "Old subprocess {} should be cleaned up after switch",
        initial_pid
    );
}

Step 4.5: Write test - cleanup outside prompt turns

/// Test that subprocess cleanup happens outside of prompt turns
/// (not during streaming)
#[test]
fn test_acp_cleanup_outside_prompt_turn() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_string())
        .with_stream_until_cancel();  // Agent streams until cancelled

    let mut session = TuiSession::spawn_with_config(24, 80, config)
        .expect("Failed to spawn TUI");

    session.wait_for_text("›", TIMEOUT).expect("TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    let log_path = session.acp_log_path().expect("Should have log path");
    let initial_pids = extract_mock_agent_pids_from_log(&log_path);
    let initial_pid = initial_pids[0];

    // Start a streaming prompt
    session.send_str("Start streaming").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(tui_pty_e2e::Key::Enter).unwrap();

    // Wait for streaming to start
    session.wait_for_text("Streaming", Duration::from_secs(5))
        .expect("Streaming should start");

    // While streaming, the process should still exist
    assert!(
        process_exists(initial_pid),
        "Process should exist during streaming"
    );

    // Cancel the stream with Escape
    session.send_key(tui_pty_e2e::Key::Escape).unwrap();

    // Wait for cancellation
    std::thread::sleep(Duration::from_millis(500));

    // After cancellation (turn complete), process should still exist
    // (cleanup only happens on session switch, not turn end)
    assert!(
        process_exists(initial_pid),
        "Process should exist after turn ends (cleanup is on session switch)"
    );
}

Step 4.6: Write test - different agents don't share subprocess

/// Test that mock-model and mock-model-alt use different subprocesses
#[test]
fn test_different_agents_different_subprocesses() {
    // First session with mock-model
    let config1 = SessionConfig::new()
        .with_model("mock-model".to_string());

    let mut session1 = TuiSession::spawn_with_config(24, 80, config1)
        .expect("Failed to spawn first TUI");

    session1.wait_for_text("›", TIMEOUT).expect("First TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    let log_path1 = session1.acp_log_path().expect("Should have log path");
    let pids1 = extract_mock_agent_pids_from_log(&log_path1);
    let pid1 = pids1[0];

    // Second session with mock-model-alt (separate TUI instance)
    let config2 = SessionConfig::new()
        .with_model("mock-model-alt".to_string());

    let mut session2 = TuiSession::spawn_with_config(24, 80, config2)
        .expect("Failed to spawn second TUI");

    session2.wait_for_text("›", TIMEOUT).expect("Second TUI should start");
    std::thread::sleep(TIMEOUT_INPUT);

    let log_path2 = session2.acp_log_path().expect("Should have log path");
    let pids2 = extract_mock_agent_pids_from_log(&log_path2);
    let pid2 = pids2[0];

    // Different TUI instances should have different agent PIDs
    assert_ne!(
        pid1, pid2,
        "Different agent models should spawn different subprocesses"
    );
}

Step 4.7: Run the tests

Command:
cd /home/clifford/Documents/source/nori/cli/.worktrees/e2e-agent-switching-tests/codex-rs && cargo test -p tui-pty-e2e
--test agent_switching -- --test-threads=1

Note: Use --test-threads=1 to avoid race conditions with subprocess management.

---
Edge Cases to Address

1. Race condition in PID logging: The PID might not be written to log immediately. Tests include sleep calls to allow
for async log flushing.
2. Process cleanup timing: Old subprocess cleanup relies on Drop semantics which are deterministic but may have small
delays. Tests include reasonable timeouts.
3. Zombie processes: If the parent doesn't properly wait() on child, zombie processes may remain. The /proc/{pid} check
will still find zombies - may need to check /proc/{pid}/status for "Z" state if this is a concern.
4. Log file truncation: When session restarts, the log file may be overwritten. The current design appends PIDs so this
should work, but tests should handle the case where logs are truncated.
5. Platform-specific: /proc filesystem is Linux-specific. Tests should be marked #[cfg(target_os = "linux")] or have
fallback behavior for macOS.

---
Questions Requiring Clarity

1. Should we verify zombie state? Current implementation only checks if /proc/{pid} exists. Should we also verify the
process is not a zombie by checking /proc/{pid}/status?
2. Should mock-model-alt have different behavior? Currently planned to use the same binary with different provider_slug.
 Should the alt model respond differently to distinguish it in tests?
3. Is /new the only way to switch agents? The model picker might also trigger agent switches - should we test that path
too?
4. Test parallelism: Should we allow parallel test execution or keep --test-threads=1 for safety?

---
Testing Details: The tests verify BEHAVIOR (subprocess spawning, cleanup, isolation) rather than implementation details.
 They use observable side effects (PID existence via /proc, log file contents) to verify the subprocess lifecycle.

Implementation Details:
- Add mock-model-alt entry to ACP registry with provider_slug = "mock-acp-alt"
- Mock agent emits MOCK_AGENT_PID:{pid} to stderr on startup
- Helper functions in tui-pty-e2e extract PIDs from logs
- Helper function checks process existence via /proc/{pid}
- Five E2E tests cover: spawning, switching, cleanup, timing, and isolation
- Tests use existing tui-pty-e2e infrastructure with SessionConfig
- Tests marked Linux-only due to /proc dependency

Questions:
1. Should zombie process detection be added (check /proc/{pid}/status for "Z" state)?
2. Should mock-model-alt have distinct behavior beyond different provider_slug?
3. Should we test model picker triggered agent switches in addition to /new command?

---

Key points:

Summary:
1. Registry change: Add mock-model-alt with different provider_slug
2. PID tracking: Mock agent emits MOCK_AGENT_PID:{pid} to stderr
3. 5 E2E tests covering subprocess spawning, switching, cleanup, timing, and isolation
4. Linux-specific: Uses /proc filesystem for process verification

Questions that input:

User answered Claude's questions:
⎿  · Should we verify zombie process state (check /proc/{pid}/status for 'Z') in addition to process existence? → Yes, check for zombies
   · Should mock-model-alt have different observable behavior to distinguish it in tests? → Same behavior, different slug
   · Should we also test agent switching via model picker (in addition to /new command)? → Both /new and model picker

Incorporating these requirements:
1. ✅ Add zombie process detection via /proc/{pid}/status
2. ✅ Keep mock-model-alt with same behavior, different slug
3. ✅ Add tests for model picker switching in addition to /new

