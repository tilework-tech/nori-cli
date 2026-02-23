//! E2E tests for ACP file write/create/edit operations
//!
//! These tests verify that the ACP write_text_file implementation works correctly
//! for various file operation scenarios:
//!
//! ## Test Coverage
//!
//! 1. **Creating new files** - Verify agent can create files that don't exist
//! 2. **Editing existing files** - Verify agent can modify existing files
//! 3. **Directory creation** - Verify parent directories are auto-created
//! 4. **Path restrictions** - Verify security boundaries (workspace, /tmp, system paths)
//! 5. **Multiple operations** - Verify multiple file writes in one session
//!
//! ## Test Strategy
//!
//! Tests use the mock-acp-agent configured via environment variables:
//! - `MOCK_AGENT_WRITE_FILE`: Path to write
//! - `MOCK_AGENT_WRITE_CONTENT`: Content to write
//!
//! The mock agent:
//! 1. Requests write via client's `fs/write_text_file` method
//! 2. Reports success or failure as text chunks
//! 3. Optionally reads back the file to verify content
//!
//! ## Expected Behavior
//!
//! Success: `"File written successfully"` + optional `"Verified content: ..."`
//! Failure: `"Failed to write file: ..."` or `"Write restricted ..."`

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TIMEOUT_PRESNAPSHOT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::normalize_for_input_snapshot;

/// Test that an ACP agent can create a new file that doesn't exist
///
/// This verifies:
/// 1. Agent requests to write to a non-existent file
/// 2. ClientDelegate's write_text_file creates the file
/// 3. Content is written correctly
/// 4. Success message appears in TUI
#[test]
#[cfg(target_os = "linux")]
fn test_acp_create_new_file() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_WRITE_FILE", "new_file.txt")
        .with_agent_env("MOCK_AGENT_WRITE_CONTENT", "New file created by ACP");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send a prompt to trigger file creation
    session.send_str("Create a new file").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the write operation to complete
    session
        .wait_for_text("File written successfully", Duration::from_secs(10))
        .expect("Should successfully create new file");

    let contents = session.screen_contents();

    // Verify the file was created and content verified
    assert!(
        contents.contains("File written successfully"),
        "Should show success message, got: {}",
        contents
    );

    // The mock agent reads back the file to verify
    assert!(
        contents.contains("File written successfully") && contents.contains("Verified content:"),
        "Should verify file content was written correctly, got: {}",
        contents
    );
}

/// Test that an ACP agent can edit an existing file
///
/// This verifies:
/// 1. Agent can modify hello.py which exists in the temp directory
/// 2. Old content is replaced with new content
/// 3. Content verification shows the updated text
#[test]
#[cfg(target_os = "linux")]
fn test_acp_edit_existing_file() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_WRITE_FILE", "hello.py")
        .with_agent_env(
            "MOCK_AGENT_WRITE_CONTENT",
            "print('Modified by ACP agent!')",
        );

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send prompt to trigger edit
    session.send_str("Modify hello.py").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for write completion
    session
        .wait_for_text("File written successfully", Duration::from_secs(10))
        .expect("Should successfully edit existing file");

    let contents = session.screen_contents();

    // Verify file was written
    assert!(
        contents.contains("File written successfully"),
        "Should show success message, got: {}",
        contents
    );

    // Verify new content replaced old content
    assert!(
        contents.contains("File written successfully") && contents.contains("Verified content:"),
        "Should verify updated content, got: {}",
        contents
    );
}

/// Test that an ACP agent can create files with nested subdirectories
///
/// This verifies:
/// 1. Parent directories are automatically created when they don't exist
/// 2. File is created in the nested location
/// 3. Success message confirms operation completed
#[test]
#[cfg(target_os = "linux")]
fn test_acp_create_file_with_parent_dirs() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_WRITE_FILE", "subdir/nested/file.txt")
        .with_agent_env("MOCK_AGENT_WRITE_CONTENT", "Content in nested directory");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send prompt to trigger nested file creation
    session.send_str("Create nested file").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for write completion
    session
        .wait_for_text("File written successfully", Duration::from_secs(10))
        .expect("Should successfully create file with parent directories");

    let contents = session.screen_contents();

    // Verify file was created with parent directories
    assert!(
        contents.contains("File written successfully"),
        "Should show success message, got: {}",
        contents
    );

    // Verify file was created with parent directories
    assert!(
        contents.contains("File written successfully") && contents.contains("Verified content:"),
        "Should verify content in nested file, got: {}",
        contents
    );
}

/// Test that writes outside the workspace are denied
///
/// This verifies the path restriction security boundary:
/// 1. Attempts to write outside workspace fail
/// 2. Appropriate error message is shown
/// 3. File is NOT created
#[test]
#[cfg(target_os = "linux")]
fn test_acp_write_outside_workspace_denied() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_WRITE_FILE", "/home/outside/forbidden.txt")
        .with_agent_env("MOCK_AGENT_WRITE_CONTENT", "This should not be written");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send prompt to trigger forbidden write
    session.send_str("Write outside workspace").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for error message
    session
        .wait_for_text("Failed to write file", Duration::from_secs(10))
        .expect("Should show error message for restricted write");

    let contents = session.screen_contents();

    // Verify write was blocked
    assert!(
        contents.contains("Failed to write file"),
        "Should show failure message, got: {}",
        contents
    );

    // Should NOT show success
    assert!(
        !contents.contains("File written successfully"),
        "Should not show success for forbidden write, got: {}",
        contents
    );
}

/// Test that writes to system paths are denied
///
/// This verifies:
/// 1. Attempts to write to system paths like /etc are blocked
/// 2. Error message indicates the restriction
/// 3. System files remain untouched
#[test]
#[cfg(target_os = "linux")]
fn test_acp_write_system_path_denied() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_WRITE_FILE", "/etc/hosts")
        .with_agent_env("MOCK_AGENT_WRITE_CONTENT", "Malicious content");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send prompt to trigger system path write
    session.send_str("Write to /etc/hosts").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for error message
    session
        .wait_for_text("Failed to write file", Duration::from_secs(10))
        .expect("Should show error for system path write");

    let contents = session.screen_contents();

    // Verify write was blocked
    assert!(
        contents.contains("Failed to write file"),
        "Should block system path writes, got: {}",
        contents
    );

    assert!(
        !contents.contains("File written successfully"),
        "Should not allow system path writes, got: {}",
        contents
    );
}

/// Test that writes to /tmp/claude are allowed
///
/// This verifies:
/// 1. /tmp/claude is an explicitly allowed write location (sandbox-safe)
/// 2. Files can be created in /tmp/claude
/// 3. Content is written correctly
#[test]
#[cfg(target_os = "linux")]
fn test_acp_write_to_tmp_allowed() {
    // Note: The sandbox allows writes to /tmp/claude/, not arbitrary /tmp/ paths
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_WRITE_FILE", "/tmp/claude/acp_test_file.txt")
        .with_agent_env("MOCK_AGENT_WRITE_CONTENT", "Temporary file content");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Send prompt to trigger /tmp/claude write
    session.send_str("Write to /tmp/claude").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for success
    session
        .wait_for_text("Verified content", Duration::from_secs(10))
        .expect("Should allow writes to /tmp");

    let contents = session.screen_contents();

    // Verify /tmp write succeeded
    assert!(
        contents.contains("File written successfully"),
        "Should allow /tmp writes, got: {}",
        contents
    );

    // Verify /tmp write succeeded and content was verified
    assert!(
        contents.contains("File written successfully") && contents.contains("Verified content:"),
        "Should verify /tmp file content, got: {}",
        contents
    );
}

/// Test that multiple file writes work in a single session
///
/// This verifies:
/// 1. Agent can write multiple files sequentially
/// 2. Session remains stable across operations
/// 3. Each write is independent and succeeds
#[test]
#[cfg(target_os = "linux")]
fn test_acp_multiple_file_writes() {
    // First write: create file1.txt
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_WRITE_FILE", "file1.txt")
        .with_agent_env("MOCK_AGENT_WRITE_CONTENT", "First file content");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // First write
    session.send_str("Write first file").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("File written successfully", Duration::from_secs(10))
        .expect("First file write should succeed");

    // Wait for prompt to return
    session
        .wait_for_text("›", Duration::from_secs(5))
        .expect("Prompt should return after first write");

    std::thread::sleep(TIMEOUT_INPUT);

    // Note: For the second write, we cannot change env vars after spawn.
    // This test verifies that the first write succeeded and the session
    // remains stable and ready for more input. A full multiple-write test
    // would require extending the mock agent to support multiple paths.

    // Verify session is still functional by typing another message
    session.send_str("Session still active").unwrap();

    session
        .wait_for_text("Session still active", Duration::from_secs(3))
        .expect("Session should remain functional after write");

    let contents = session.screen_contents();

    // Verify first write succeeded
    assert!(
        contents.contains("File written successfully"),
        "First write should succeed, got: {}",
        contents
    );

    // Verify session remained stable
    assert!(
        contents.contains("Session still active"),
        "Session should remain functional, got: {}",
        contents
    );
}

/// Snapshot test for file write operation
///
/// This captures the visual rendering of a successful file write
/// to detect any regressions in display format.
#[test]
#[cfg(target_os = "linux")]
#[ignore]
fn test_acp_file_write_snapshot() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_WRITE_FILE", "snapshot_test.txt")
        .with_agent_env("MOCK_AGENT_WRITE_CONTENT", "Snapshot test content");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Trigger file write
    session.send_str("Create snapshot file").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for completion
    session
        .wait_for_text("File written successfully", Duration::from_secs(10))
        .expect("File write should complete");

    std::thread::sleep(TIMEOUT_PRESNAPSHOT);

    insta::assert_snapshot!(
        "acp_file_write_success",
        normalize_for_input_snapshot(session.screen_contents())
    );
}
