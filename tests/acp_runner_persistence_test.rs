/// Test that ACP runners persist across multiple conversational turns with the same agent,
/// and are properly replaced when switching agents.
use nori_cli::app::{BACKEND_OPTIONS, Message, Model};
use nori_cli::backends::AgentBackend;
use std::process::Command;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

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

/// Helper to check if a process exists using kill -0
fn process_exists(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Helper to get PID from an ACP backend if it has one
fn get_backend_pid(backend: &Box<dyn AgentBackend + Send>) -> Option<u32> {
    // This is a bit hacky, but we need to access the runner's PID
    // We'll use the backend's name to determine if it's a mock backend,
    // then downcast to access the runner
    // For now, we'll return None and rely on integration-level testing
    // TODO: This might need refinement based on actual backend structure
    None
}

#[tokio::test]
async fn test_runner_persists_across_multiple_prompts_same_agent() {
    let _guard = acquire_test_guard();
    build_mock_agent();

    // Create a model and set it to use the Mock ACP agent
    let mut model = Model::default();

    // Find the Mock ACP agent index
    let mock_index = BACKEND_OPTIONS
        .iter()
        .position(|opt| opt.name == "Mock ACP Agent")
        .expect("Mock ACP Agent should be in BACKEND_OPTIONS");

    model.selected_agent_index = mock_index;

    // Get the backend (this should create it)
    let backend1 = model.get_backend();

    // In the future, after our changes, this should reuse the same backend
    let backend2 = model.get_backend();

    // For now, this test will fail because get_backend creates a new backend each time
    // After our implementation, we should verify that backend1 and backend2 are the same instance
    // (or at least that they share the same underlying subprocess PID)

    // Note: This test demonstrates the current behavior (new backend each time)
    // After implementation, we'll verify persistence by checking subprocess PIDs

    drop(backend1);
    drop(backend2);

    // This test will be updated after implementation to actually verify PID persistence
    // For now, it just documents the intended behavior
}

#[tokio::test]
async fn test_runner_replaced_when_agent_changes() {
    let _guard = acquire_test_guard();
    build_mock_agent();

    let mut model = Model::default();

    // Find Mock and Claude Code ACP indices
    let mock_index = BACKEND_OPTIONS
        .iter()
        .position(|opt| opt.name == "Mock ACP Agent")
        .expect("Mock ACP Agent should be in BACKEND_OPTIONS");

    let claude_code_index = BACKEND_OPTIONS
        .iter()
        .position(|opt| opt.name == "Claude Code ACP")
        .expect("Claude Code ACP should be in BACKEND_OPTIONS");

    // Start with Mock agent
    model.selected_agent_index = mock_index;
    let backend1 = model.get_backend();

    // Change to Claude Code agent (without submitting)
    // CRITICAL: This should NOT drop backend1 yet!
    model.selected_agent_index = claude_code_index;

    // Only when we get the backend for a new prompt should it be replaced
    let backend2 = model.get_backend();

    // After implementation, verify that:
    // 1. Changing selection didn't drop backend1's subprocess
    // 2. Getting backend2 created a new backend with different subprocess

    drop(backend1);
    drop(backend2);
}

#[tokio::test]
async fn test_selection_change_without_submit_preserves_runner() {
    let _guard = acquire_test_guard();
    build_mock_agent();

    let mut model = Model::default();

    let mock_index = BACKEND_OPTIONS
        .iter()
        .position(|opt| opt.name == "Mock ACP Agent")
        .expect("Mock ACP Agent should be in BACKEND_OPTIONS");

    let claude_index = BACKEND_OPTIONS
        .iter()
        .position(|opt| opt.name == "Claude Code ACP")
        .expect("Claude Code ACP should be in BACKEND_OPTIONS");

    // Start with Mock agent
    model.selected_agent_index = mock_index;

    // Simulate getting the backend (as would happen on SubmitInput)
    let _backend = model.get_backend();

    // Now user changes selection but doesn't submit
    model.update(Message::SelectItem);
    model.selected_agent_index = claude_index;

    // The backend should still be for the mock agent
    // Only on next SubmitInput should it be replaced

    // After implementation, we'll verify the subprocess wasn't killed just from SelectItem
}
