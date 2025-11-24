use serial_test::serial;
use std::fs;
use tempfile::TempDir;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

/// Comprehensive test that verifies all tracing functionality
/// This must be a single test because the global subscriber can only be set once
#[test]
#[serial]
fn test_file_tracing_comprehensive() {
    // Create a temporary directory for the test
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let log_file_path = temp_dir.path().join(".codex-acp.log");

    // Test 1: First initialization should succeed
    let result1 = codex_acp::init_file_tracing(&log_file_path);
    assert!(result1.is_ok(), "First initialization should succeed");

    // Test 2: Emit test log events and verify they appear in file
    debug!("This is a debug message");
    info!("This is an info message");
    warn!("This is a warning message");
    error!("This is an error message");
    tracing::trace!("This is a trace message that should not appear");

    // Give async logger time to flush
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Verify log file exists
    assert!(
        log_file_path.exists(),
        "Log file should exist at {:?}",
        log_file_path
    );

    // Read and verify log file contents
    let contents = fs::read_to_string(&log_file_path).expect("Failed to read log file");

    // Test 3: Verify that DEBUG and above appear in the file
    assert!(
        contents.contains("This is a debug message"),
        "Log file should contain debug message"
    );
    assert!(
        contents.contains("This is an info message"),
        "Log file should contain info message"
    );
    assert!(
        contents.contains("This is a warning message"),
        "Log file should contain warning message"
    );
    assert!(
        contents.contains("This is an error message"),
        "Log file should contain error message"
    );

    // Test 4: Verify TRACE is filtered out
    assert!(
        !contents.contains("This is a trace message"),
        "Log file should NOT contain trace message (filtered out)"
    );

    // Test 5: Second initialization should fail (global subscriber already set)
    let result2 = codex_acp::init_file_tracing(&log_file_path);
    assert!(
        result2.is_err(),
        "Second initialization should return error"
    );
}
