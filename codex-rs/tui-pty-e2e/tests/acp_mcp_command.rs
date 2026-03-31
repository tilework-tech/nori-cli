//! E2E tests for /mcp slash command in ACP mode.

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

/// Test that /mcp opens the MCP server management picker with configured servers.
#[test]
#[cfg(target_os = "linux")]
fn test_mcp_command_opens_picker_with_servers() {
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

    // The picker should open showing the server name and "Add new..."
    session
        .wait_for_text("MCP Servers", TIMEOUT)
        .expect("/mcp should open the MCP server picker");

    let contents = session.screen_contents();
    assert!(
        contents.contains("Add new..."),
        "/mcp picker should show 'Add new...' option, got:\n{contents}"
    );
    assert!(
        contents.contains("test-server"),
        "/mcp picker should show configured server name, got:\n{contents}"
    );
}

/// Test that /mcp with no servers configured opens picker with just "Add new...".
#[test]
#[cfg(target_os = "linux")]
fn test_mcp_command_no_servers_opens_picker() {
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

    // The picker should open with just "Add new..."
    session
        .wait_for_text("MCP Servers", TIMEOUT)
        .expect("/mcp should open the MCP server picker even with no servers");

    let contents = session.screen_contents();
    assert!(
        contents.contains("Add new..."),
        "/mcp picker should show 'Add new...' option, got:\n{contents}"
    );
}
