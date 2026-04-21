//! E2E tests for live authenticated ACP models
//!
//! These tests are marked with `#[ignore]` and require API credentials to run.
//! Run with: `cargo test --package tui-pty-e2e -- --ignored`
//!
//! Required environment variables:
//! - GEMINI_API_KEY for gemini-acp tests
//! - ANTHROPIC_API_KEY for claude-acp tests

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TuiSession;

/// Longer timeout for live API tests (network latency, model processing)
const LIVE_TIMEOUT: Duration = Duration::from_secs(30);
const LIVE_TIMEOUT_INPUT: Duration = Duration::from_millis(500);

/// Test gemini-acp with a real Gemini API connection
#[test]
#[cfg(target_os = "linux")]
#[ignore]
fn test_gemini_acp_live_response() {
    // Skip if API key not set
    if std::env::var("GEMINI_API_KEY").is_err() {
        eprintln!("Skipping test_gemini_acp_live_response: GEMINI_API_KEY not set");
        return;
    }

    let config = SessionConfig::new()
        .with_model("gemini-acp".to_owned())
        .with_config_toml(generate_live_config("gemini-acp"));

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn with gemini-acp");

    session
        .wait_for_text("? for shortcuts", LIVE_TIMEOUT)
        .expect("TUI should start successfully");

    // Send a simple prompt
    session.send_str("Say hello").unwrap();
    std::thread::sleep(LIVE_TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(LIVE_TIMEOUT_INPUT);

    // Wait for any response from the model (not a specific string, just any output)
    session
        .wait_for(
            |screen| {
                // Check that we got some response text after the prompt
                // The screen should contain more than just the prompt area
                screen.lines().count() > 5 && !screen.contains("Error")
            },
            LIVE_TIMEOUT,
        )
        .expect("Should receive a response from gemini-acp");

    eprintln!(
        "Gemini ACP test completed. Screen contents:\n{}",
        session.screen_contents()
    );
}

/// Test claude-acp with a real Claude API connection
#[test]
#[cfg(target_os = "linux")]
#[ignore]
fn test_claude_acp_live_response() {
    // Skip if API key not set
    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        eprintln!("Skipping test_claude_acp_live_response: ANTHROPIC_API_KEY not set");
        return;
    }

    let config = SessionConfig::new()
        .with_model("claude-acp".to_owned())
        .with_config_toml(generate_live_config("claude-acp"));

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn with claude-acp");

    session
        .wait_for_text("? for shortcuts", LIVE_TIMEOUT)
        .expect("TUI should start successfully");

    // Send a simple prompt
    session.send_str("Say hello").unwrap();
    std::thread::sleep(LIVE_TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(LIVE_TIMEOUT_INPUT);

    // Wait for any response from the model (not a specific string, just any output)
    session
        .wait_for(
            |screen| {
                // Check that we got some response text after the prompt
                // The screen should contain more than just the prompt area
                screen.lines().count() > 5 && !screen.contains("Error")
            },
            LIVE_TIMEOUT,
        )
        .expect("Should receive a response from claude-acp");

    eprintln!(
        "Claude ACP test completed. Screen contents:\n{}",
        session.screen_contents()
    );
}

/// Generate a config.toml for live ACP testing
fn generate_live_config(model: &str) -> String {
    format!(
        r#"model = "{model}"
model_provider = "live_acp_provider"

[model_providers.live_acp_provider]
name = "Live ACP provider for tests"
wire_api = "acp"
"#
    )
}
