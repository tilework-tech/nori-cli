use serial_test::serial;
use std::fs;
use tempfile::TempDir;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

/// Comprehensive test that verifies rolling file tracing functionality
/// This must be a single test because the global subscriber can only be set once
#[test]
#[serial]
fn test_rolling_file_tracing_comprehensive() {
    // Create a temporary directory for the test
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let log_dir = temp_dir.path().join("logs");

    // Test 1: First initialization should succeed
    let result1 = nori_acp::init_rolling_file_tracing(&log_dir, "nori-acp");
    assert!(result1.is_ok(), "First initialization should succeed");

    let debug_enabled = tracing::enabled!(tracing::Level::DEBUG);
    let info_enabled = tracing::enabled!(tracing::Level::INFO);
    let warn_enabled = tracing::enabled!(tracing::Level::WARN);
    let error_enabled = tracing::enabled!(tracing::Level::ERROR);
    let trace_enabled = tracing::enabled!(tracing::Level::TRACE);

    // Test 2: Emit test log events and verify they appear in file
    debug!("This is a debug message");
    info!("This is an info message");
    warn!("This is a warning message");
    error!("This is an error message");
    tracing::trace!("This is a trace message");

    // Give async logger time to flush
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Verify log directory exists
    assert!(
        log_dir.exists(),
        "Log directory should exist at {log_dir:?}"
    );

    // Find the log file (rolling files have date suffix like nori-acp.2024-01-15)
    let log_files: Vec<_> = fs::read_dir(&log_dir)
        .expect("Failed to read log directory")
        .filter_map(std::result::Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("nori-acp"))
        .collect();

    assert!(
        !log_files.is_empty(),
        "Log directory should contain at least one log file"
    );

    // Read and verify log file contents
    let log_file_path = log_files[0].path();
    let contents = fs::read_to_string(&log_file_path).expect("Failed to read log file");

    // Test 3: Captured messages should match the active subscriber filter.
    assert_eq!(
        contents.contains("This is a debug message"),
        debug_enabled,
        "debug message capture should match active filter"
    );
    assert_eq!(
        contents.contains("This is an info message"),
        info_enabled,
        "info message capture should match active filter"
    );
    assert_eq!(
        contents.contains("This is a warning message"),
        warn_enabled,
        "warning message capture should match active filter"
    );
    assert_eq!(
        contents.contains("This is an error message"),
        error_enabled,
        "error message capture should match active filter"
    );
    assert_eq!(
        contents.contains("This is a trace message"),
        trace_enabled,
        "trace message capture should match active filter"
    );

    // Test 5: Second initialization should fail (global subscriber already set)
    let result2 = nori_acp::init_rolling_file_tracing(&log_dir, "nori-acp");
    assert!(
        result2.is_err(),
        "Second initialization should return error"
    );

    // Also verify legacy function fails (same global subscriber constraint)
    let legacy_path = temp_dir.path().join("legacy.log");
    let result3 = nori_acp::init_file_tracing(&legacy_path);
    assert!(
        result3.is_err(),
        "Legacy initialization should also fail when subscriber already set"
    );
}
