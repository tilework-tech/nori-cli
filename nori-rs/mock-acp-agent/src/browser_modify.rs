use std::time::Duration;

use serde_json::json;

fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .new_agent()
}

/// Connect to Chrome via CDP and change document.title.
/// Returns Ok(new_title) on success.
pub fn modify_browser_title(cdp_port: i32, new_title: &str) -> Result<String, String> {
    let targets_url = format!("http://127.0.0.1:{cdp_port}/json");
    let agent = http_agent();

    let mut targets_body = String::new();
    for attempt in 0..10 {
        eprintln!("Mock agent: CDP target list attempt {attempt}");
        match agent.get(&targets_url).call() {
            Ok(mut resp) => {
                targets_body = resp
                    .body_mut()
                    .read_to_string()
                    .map_err(|e| format!("failed to read CDP targets body: {e}"))?;
                eprintln!("Mock agent: got targets: {targets_body}");
                break;
            }
            Err(e) => {
                if attempt == 9 {
                    return Err(format!("failed to list CDP targets after 10 attempts: {e}"));
                }
                eprintln!("Mock agent: CDP target list attempt {attempt} failed: {e}");
                std::thread::sleep(Duration::from_millis(500));
            }
        }
    }

    let targets: Vec<serde_json::Value> = serde_json::from_str(&targets_body)
        .map_err(|e| format!("invalid CDP targets JSON: {e}"))?;

    let page_target = targets
        .iter()
        .find(|t| t["type"].as_str() == Some("page"))
        .ok_or("no page target found in CDP /json")?;

    let ws_url = page_target["webSocketDebuggerUrl"]
        .as_str()
        .ok_or("page target missing webSocketDebuggerUrl")?;

    eprintln!("Mock agent: connecting to CDP WebSocket: {ws_url}");
    let (mut socket, _response) = tungstenite::connect(ws_url)
        .map_err(|e| format!("failed to connect to CDP WebSocket: {e}"))?;

    let cmd = json!({
        "id": 1,
        "method": "Runtime.evaluate",
        "params": {
            "expression": format!("document.title = '{new_title}'"),
            "returnByValue": true
        }
    });

    socket
        .send(tungstenite::Message::Text(cmd.to_string().into()))
        .map_err(|e| format!("failed to send CDP command: {e}"))?;

    let response = socket
        .read()
        .map_err(|e| format!("failed to read CDP response: {e}"))?;

    eprintln!("Mock agent: CDP response: {response}");

    socket.close(None).ok();

    Ok(new_title.to_string())
}
