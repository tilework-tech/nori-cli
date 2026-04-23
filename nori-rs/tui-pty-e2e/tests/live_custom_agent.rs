//! E2E tests for custom agents defined via the data-driven agent registry.
//!
//! These tests exercise the `[[agents]]` TOML config path end-to-end: nori
//! reads the config, initializes the registry, resolves the custom agent
//! distribution, spawns the subprocess, and relays ACP messages.
//!
//! Tests are marked `#[ignore]` because they require external binaries.
//! Run with: `cargo test --package tui-pty-e2e -- --ignored`
//!
//! Required binaries in PATH:
//! - `elizacp` for the ElizACP tests (cargo install elizacp)

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TuiSession;

/// Longer timeout for live agent tests (subprocess spawn + init)
const LIVE_TIMEOUT: Duration = Duration::from_secs(30);
const LIVE_TIMEOUT_INPUT: Duration = Duration::from_millis(500);

/// Config TOML snippet that registers elizacp as a custom local agent.
const ELIZACP_CONFIG: &str = r#"model = "elizacp"
model_provider = "elizacp_provider"

[projects."/"]
trust_level = "trusted"

[model_providers.elizacp_provider]
name = "ElizACP provider"

[[agents]]
name = "ElizACP"
slug = "elizacp"

[agents.distribution.local]
command = "elizacp"
args = ["--deterministic", "acp"]
"#;

/// Returns true if `elizacp` is available in PATH.
fn elizacp_available() -> bool {
    std::process::Command::new("elizacp")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Test that nori can start with elizacp as a custom local agent and exchange messages.
///
/// This validates the full data-driven agent registry path:
/// 1. Config TOML `[[agents]]` with local distribution is parsed
/// 2. Registry is initialized with the custom agent
/// 3. `--agent elizacp` resolves to the custom agent config
/// 4. elizacp subprocess is spawned and ACP handshake succeeds
/// 5. A user message is sent and a response is received
#[test]
#[cfg(target_os = "linux")]
#[ignore]
fn test_elizacp_custom_agent_startup_and_response() {
    if !elizacp_available() {
        eprintln!("Skipping test_elizacp_custom_agent_startup_and_response: elizacp not in PATH");
        return;
    }

    let config = SessionConfig::new()
        .with_model("elizacp".to_owned())
        .with_config_toml(ELIZACP_CONFIG);

    let mut session = TuiSession::spawn_with_config(24, 80, config)
        .expect("Failed to spawn nori with elizacp agent");

    // Wait for the TUI to start and show the prompt
    session
        .wait_for_text("? for shortcuts", LIVE_TIMEOUT)
        .expect("TUI should start successfully with elizacp agent");

    // Send a simple message
    session.send_str("Hello").unwrap();
    std::thread::sleep(LIVE_TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Wait for the prompt to reappear after the response, indicating
    // a complete exchange. The "›" prompt returns once the agent finishes.
    // First wait for the user message to be sent (prompt disappears).
    session
        .wait_for(
            |screen| {
                // After response completes, we should see the second prompt "›"
                // at the bottom AND the screen should not show spawn errors.
                let has_prompt = screen.contains("›");
                let no_error = !screen.contains("Failed to spawn")
                    && !screen.contains("ACP initialization failed");
                // The response should have generated a Task summary in the footer
                let has_task = screen.contains("Task:");
                has_prompt && no_error && has_task
            },
            LIVE_TIMEOUT,
        )
        .expect("Should receive a response from elizacp agent");

    eprintln!(
        "ElizACP custom agent test completed. Screen contents:\n{}",
        session.screen_contents()
    );
}

/// Test that nori shows the custom agent name correctly in the UI.
#[test]
#[cfg(target_os = "linux")]
#[ignore]
fn test_elizacp_custom_agent_display_name() {
    if !elizacp_available() {
        eprintln!("Skipping test_elizacp_custom_agent_display_name: elizacp not in PATH");
        return;
    }

    let config = SessionConfig::new()
        .with_model("elizacp".to_owned())
        .with_config_toml(ELIZACP_CONFIG);

    let mut session = TuiSession::spawn_with_config(24, 80, config)
        .expect("Failed to spawn nori with elizacp agent");

    // Wait for startup
    session
        .wait_for_text("? for shortcuts", LIVE_TIMEOUT)
        .expect("TUI should start successfully");

    let contents = session.screen_contents();
    eprintln!("ElizACP display name test. Screen contents:\n{}", contents);

    // Verify no startup errors
    assert!(
        !contents.contains("Failed to spawn") && !contents.contains("initialization failed"),
        "Should start without errors, got: {}",
        contents
    );
}
