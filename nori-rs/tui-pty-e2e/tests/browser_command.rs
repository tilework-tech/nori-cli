//! E2E test for the /browser command.
//!
//! This test verifies the full browser integration flow:
//! 1. Nori TUI starts
//! 2. `/browser` launches Chrome with CDP enabled
//! 3. The agent receives CDP connection details
//! 4. The agent connects to the browser via CDP and modifies the page
//! 5. The modification is verified via CDP from the test process
//!
//! Requires Chrome/Chromium installed and a display server (X11/Wayland).

use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

/// Verify Chrome is reachable and the page title was modified.
fn verify_browser_title(cdp_port: u16, expected_title: &str) -> Result<(), String> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .new_agent();

    let targets_url = format!("http://127.0.0.1:{cdp_port}/json");
    let targets_body: String = agent
        .get(&targets_url)
        .call()
        .map_err(|e| format!("failed to list CDP targets: {e}"))?
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("failed to read CDP targets body: {e}"))?;

    let targets: Vec<serde_json::Value> =
        serde_json::from_str(&targets_body).map_err(|e| format!("invalid CDP JSON: {e}"))?;

    let page_target = targets
        .iter()
        .find(|t| t["type"].as_str() == Some("page"))
        .ok_or("no page target found")?;

    let ws_url = page_target["webSocketDebuggerUrl"]
        .as_str()
        .ok_or("page target missing webSocketDebuggerUrl")?;

    let (mut socket, _response) = tungstenite::connect(ws_url)
        .map_err(|e| format!("failed to connect to CDP WebSocket: {e}"))?;

    let cmd = serde_json::json!({
        "id": 1,
        "method": "Runtime.evaluate",
        "params": {
            "expression": "document.title",
            "returnByValue": true
        }
    });

    socket
        .send(tungstenite::Message::Text(cmd.to_string().into()))
        .map_err(|e| format!("failed to send CDP command: {e}"))?;

    let response = socket
        .read()
        .map_err(|e| format!("failed to read CDP response: {e}"))?;

    socket.close(None).ok();

    let resp: serde_json::Value = serde_json::from_str(response.to_text().unwrap_or("{}"))
        .map_err(|e| format!("invalid CDP response JSON: {e}"))?;

    let actual_title = resp["result"]["result"]["value"]
        .as_str()
        .unwrap_or("<missing>");

    if actual_title == expected_title {
        Ok(())
    } else {
        Err(format!(
            "expected title '{expected_title}', got '{actual_title}'"
        ))
    }
}

/// Extract the CDP port from the TUI screen contents.
/// Looks for "CDP endpoint: http://127.0.0.1:PORT" in the browser session prompt.
fn extract_cdp_port_from_screen(screen: &str) -> Option<u16> {
    for line in screen.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("CDP endpoint: http://127.0.0.1:")
            && let Ok(port) = rest.trim().parse::<u16>() {
                return Some(port);
            }
    }
    None
}

/// Full end-to-end test: start app → launch browser → agent modifies page → verify HTML changed.
///
/// This test is ignored by default because it requires Chrome and a display server.
/// Run with: `cargo test -p tui-pty-e2e -- --ignored browser`
#[test]
#[ignore]
#[cfg(target_os = "linux")]
fn browser_command_agent_modifies_page() {
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_BROWSER_MODIFY", "1");

    let mut session = TuiSession::spawn_with_config(30, 120, config).expect("Failed to spawn nori");

    // 1. Confirm the nori session loads
    session
        .wait_for_text("›", TIMEOUT)
        .expect("nori should start and show prompt");

    std::thread::sleep(TIMEOUT_INPUT);

    // 2. Send /browser command
    session.send_str("/browser").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // 3. Wait for the browser session prompt to appear on screen.
    //    This contains "CDP endpoint: http://127.0.0.1:PORT" which proves
    //    Chrome launched and the agent received connection details.
    session
        .wait_for_text("CDP endpoint:", Duration::from_secs(30))
        .expect("browser session prompt should appear with CDP details");

    // Extract the CDP port from the screen
    let screen = session.screen_contents();
    let cdp_port = extract_cdp_port_from_screen(&screen)
        .unwrap_or_else(|| panic!("could not extract CDP port from screen:\n{screen}"));

    eprintln!("E2E test: detected CDP port {cdp_port}");

    // 4. Wait for the agent to modify the browser.
    // The mock agent (MOCK_AGENT_BROWSER_MODIFY) receives the browser session message,
    // connects to CDP, and changes document.title to "NORI_BROWSER_TEST".
    session
        .wait_for_text(
            "BROWSER_MODIFIED:title=NORI_BROWSER_TEST",
            Duration::from_secs(30),
        )
        .expect("agent should modify the browser and report success");

    // 5. Independently verify the HTML was actually modified via CDP
    verify_browser_title(cdp_port, "NORI_BROWSER_TEST").expect("browser title should be modified");

    eprintln!("E2E test: browser title verified as 'NORI_BROWSER_TEST'");
}
