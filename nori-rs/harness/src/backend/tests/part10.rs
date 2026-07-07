//! Tests for the mid-turn `session/prompt` error path
//! (`AcpBackend::send_prompt_error`). Unlike spawn/resume/close failures,
//! which already run through `enhance_agent_error`'s clean
//! `AcpErrorDetails { category, detail }` formatting, a failed user prompt
//! previously formatted the raw anyhow chain with `format!("{err:#}")` —
//! which walked into `acp::Error`'s `Display` impl and appended the *entire*
//! pretty-printed `data` JSON blob whenever the structured error carried one,
//! not just the clean `detail` string. These tests drive a real prompt
//! failure end-to-end through the mock agent and guard that the resulting
//! `EventMsg::Error` message stays clean.

use super::*;

/// Whether the mock agent binary is available; tests skip quietly otherwise
/// (same convention as the other backend test parts).
fn mock_agent_available() -> bool {
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");
    if std::path::Path::new(&mock_config.command).exists() {
        return true;
    }
    eprintln!(
        "Skipping test: mock_acp_agent not found at {}",
        mock_config.command
    );
    false
}

/// A failed `session/prompt` carrying `error.data.detail` (JSON-RPC -32010,
/// "agent unreachable") must surface `detail` to the user verbatim — not the
/// raw pretty-printed JSON blob the agent sent alongside it (mock agent's
/// `MOCK_AGENT_PROMPT_FAIL_JSON`, mock-acp-agent/src/main.rs).
#[tokio::test]
#[serial]
async fn prompt_failure_with_structured_detail_shows_clean_message_not_raw_json() {
    use std::time::Duration;

    if !mock_agent_available() {
        return;
    }

    let _fail_guard = EnvGuard::set("MOCK_AGENT_PROMPT_FAIL_JSON", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "trigger a structured prompt failure".to_string(),
            }],
        })
        .await
        .expect("Failed to submit user input");

    let mut error_message = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match recv_backend_control(&mut backend_event_rx, Duration::from_millis(500)).await {
            Some(event) => {
                if let EventMsg::Error(err) = event.msg {
                    error_message = Some(err.message);
                    break;
                }
            }
            None => continue,
        }
    }
    let message = error_message.expect("Should receive an EventMsg::Error for the failed prompt");

    assert!(
        message.contains("connection reset by broker"),
        "the agent-supplied error.data.detail must reach the user verbatim, got: {message}"
    );
    assert!(
        !message.contains("\"detail\""),
        "the raw `detail` JSON key must not leak into the user-facing message, got: {message}"
    );
    assert!(
        !message.contains('{'),
        "no raw JSON data blob should be embedded in the user-facing message, got: {message}"
    );
    assert!(
        message.contains("backing service is unreachable"),
        "the category-specific human copy for AgentUnreachable must still be present, got: {message}"
    );
}

/// A failed `session/prompt` whose `error.data` carries no `detail` field —
/// only unrelated noise — must still avoid dumping the raw JSON blob. The fix
/// can't just substitute `detail` when present; it must stop walking the
/// anyhow chain's `Display` impl altogether (mock agent's
/// `MOCK_AGENT_PROMPT_FAIL_JSON_NO_DETAIL`, mock-acp-agent/src/main.rs).
#[tokio::test]
#[serial]
async fn prompt_failure_with_structured_data_but_no_detail_shows_clean_message() {
    use std::time::Duration;

    if !mock_agent_available() {
        return;
    }

    let _fail_guard = EnvGuard::set("MOCK_AGENT_PROMPT_FAIL_JSON_NO_DETAIL", "1");

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (backend_event_tx, mut backend_event_rx) = mpsc::channel(64);
    let config = build_test_config(temp_dir.path());

    let backend = AcpBackend::spawn(&config, backend_event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    let _ = recv_backend_control(&mut backend_event_rx, Duration::from_secs(5))
        .await
        .expect("Should receive SessionConfigured event");

    backend
        .submit(Op::UserInput {
            items: vec![codex_protocol::user_input::UserInput::Text {
                text: "trigger a structured prompt failure without detail".to_string(),
            }],
        })
        .await
        .expect("Failed to submit user input");

    let mut error_message = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match recv_backend_control(&mut backend_event_rx, Duration::from_millis(500)).await {
            Some(event) => {
                if let EventMsg::Error(err) = event.msg {
                    error_message = Some(err.message);
                    break;
                }
            }
            None => continue,
        }
    }
    let message = error_message.expect("Should receive an EventMsg::Error for the failed prompt");

    assert!(
        !message.contains('{'),
        "no raw JSON data blob should be embedded in the user-facing message, got: {message}"
    );
    assert!(
        !message.contains("retry_after_ms"),
        "unrelated noise fields from error.data must not leak into the message, got: {message}"
    );
    assert!(
        message.contains("backing service is unreachable"),
        "the category-specific human copy for AgentUnreachable must still be present, got: {message}"
    );
    assert!(
        message.contains("ACP prompt failed"),
        "the top-level error Display (the acp-host `.context(\"ACP prompt failed\")`) must still be present, got: {message}"
    );
}
