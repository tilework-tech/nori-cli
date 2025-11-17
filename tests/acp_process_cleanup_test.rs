use nori_cli::acp_runner::{AcpAgentConfig, AcpAgentRunner};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use tokio_util::sync::CancellationToken;

const MOCK_AGENT_COMMAND: &str = "target/debug/mock_acp_agent";

static TEST_GUARD: once_cell::sync::Lazy<Mutex<()>> = once_cell::sync::Lazy::new(|| Mutex::new(()));

fn acquire_test_guard<'a>() -> std::sync::MutexGuard<'a, ()> {
    TEST_GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn build_mock_agent() {
    let status = Command::new("cargo")
        .env("CARGO_TARGET_DIR", "target")
        .args(["build", "--manifest-path", "mock-acp-agent/Cargo.toml"])
        .status()
        .expect("Failed to build mock agent");
    assert!(
        status.success(),
        "Mock agent build failed with status {status:?}"
    );
}

fn mock_agent_config() -> AcpAgentConfig {
    AcpAgentConfig {
        name: "mock",
        command: MOCK_AGENT_COMMAND,
        args: vec![],
        install_url: "",
        install_command: None,
    }
}

/// Helper to check if a process exists using kill -0
fn process_exists(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[tokio::test]
async fn test_process_cleanup_on_runner_drop() {
    let _guard = acquire_test_guard();
    build_mock_agent();

    // Set env var to make mock agent stream continuously until cancelled
    // This prevents it from exiting naturally
    unsafe {
        std::env::set_var("MOCK_AGENT_STREAM_UNTIL_CANCEL", "1");
    }

    // Create a runner with the mock ACP agent
    let config = mock_agent_config();

    let mut runner = AcpAgentRunner::new(config, PathBuf::from("/tmp"));
    let cancel_token = CancellationToken::new();

    // Spawn a stream to start the agent process
    let _stream = runner
        .spawn_stream("test prompt".to_string(), cancel_token.clone())
        .await
        .expect("Failed to spawn stream");

    // Get the PID of the spawned process
    let pid = runner
        .agent_pid()
        .expect("Runner should have an agent process");

    // Give the agent a moment to start streaming
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Verify the process is still running (streaming continuously)
    assert!(
        process_exists(pid),
        "Process should be running and streaming"
    );

    // Drop the runner WITHOUT cancelling
    // The agent process is actively streaming and won't exit on its own
    drop(_stream);
    drop(runner);

    // Give the system a moment for cleanup
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Clean up env var
    unsafe {
        std::env::remove_var("MOCK_AGENT_STREAM_UNTIL_CANCEL");
    }

    // WITHOUT a Drop impl, the process will still be running (orphaned)
    // WITH a Drop impl, the process should be killed
    let still_running = process_exists(pid);

    assert!(
        !still_running,
        "Process PID {pid} should be terminated after runner drop, but it's still running. \
         This test EXPECTS to fail without a Drop implementation."
    );
}

#[tokio::test]
async fn test_process_cleanup_on_reuse() {
    let _guard = acquire_test_guard();
    build_mock_agent();

    let config = mock_agent_config();

    let mut runner = AcpAgentRunner::new(config, PathBuf::from("/tmp"));
    let cancel_token = CancellationToken::new();

    // Spawn first stream
    let _stream1 = runner
        .spawn_stream("first prompt".to_string(), cancel_token.clone())
        .await
        .expect("Failed to spawn first stream");

    // Get PID of first process
    let pid1 = runner
        .agent_pid()
        .expect("Runner should have first agent process");

    assert!(process_exists(pid1), "First process should be running");

    // Spawn second stream (should kill first process)
    let _stream2 = runner
        .spawn_stream("second prompt".to_string(), cancel_token.clone())
        .await
        .expect("Failed to spawn second stream");

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Verify first process is terminated
    assert!(
        !process_exists(pid1),
        "First process should be terminated when second stream is spawned"
    );

    // Verify a new process is running
    let pid2 = runner
        .agent_pid()
        .expect("Runner should have second agent process");

    assert!(process_exists(pid2), "New process should be running");
    assert_ne!(pid1, pid2, "Second process should have different PID");
}

#[tokio::test]
async fn test_process_cleanup_on_init_failure() {
    // This test will use a command that fails to initialize properly
    let config = AcpAgentConfig {
        name: "failing-agent",
        command: "echo",
        args: vec!["invalid".to_string()],
        install_url: "http://example.com",
        install_command: None,
    };

    let mut runner = AcpAgentRunner::new(config, PathBuf::from("/tmp"));
    let cancel_token = CancellationToken::new();

    // Try to spawn - this should fail during initialization
    let result = runner
        .spawn_stream("test prompt".to_string(), cancel_token.clone())
        .await;

    // Should fail
    assert!(result.is_err(), "Spawn should fail for invalid agent");

    // Verify no echo processes are left hanging
    // (echo exits immediately, but we're testing that the error path cleans up)
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // No specific assertion here since echo exits immediately,
    // but the test verifies the error path doesn't panic
}
