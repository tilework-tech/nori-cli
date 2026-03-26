//! E2E tests for /mcp slash command in ACP mode.

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

/// Test that /mcp shows configured MCP servers in ACP mode.
///
/// Configures a stdio-based MCP server in config.toml, launches the TUI
/// in ACP mode, types `/mcp`, and verifies the server info appears.
#[test]
#[cfg(target_os = "linux")]
fn test_mcp_command_shows_configured_servers() {
    let config_toml = r#"
[mcp_servers.test-server]
command = "echo"
args = ["hello"]
"#;

    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_config_toml(config_toml);

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Type /mcp and submit
    session.send_str("/mcp").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // The output should show the configured server name
    session
        .wait_for_text("test-server", TIMEOUT)
        .expect("/mcp should display configured MCP server name");

    let contents = session.screen_contents();
    assert!(
        contents.contains("MCP Tools"),
        "/mcp output should contain 'MCP Tools' header, got:\n{contents}"
    );
    assert!(
        contents.contains("echo"),
        "/mcp output should show the server command, got:\n{contents}"
    );
}

/// Test that /mcp with no servers configured shows appropriate message.
#[test]
#[cfg(target_os = "linux")]
fn test_mcp_command_no_servers_shows_empty_message() {
    let config = SessionConfig::new().with_model("mock-model".to_owned());

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn in ACP mode");

    // Wait for startup
    session
        .wait_for_text("›", TIMEOUT)
        .expect("ACP mode should start");

    std::thread::sleep(TIMEOUT_INPUT);

    // Type /mcp and submit
    session.send_str("/mcp").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Should show the "no servers" message (handled entirely in TUI, no backend needed)
    session
        .wait_for_text("No MCP servers configured", TIMEOUT)
        .expect("/mcp with no servers should show empty message");
}
