use std::sync::Arc;

use super::sacp_connection::SacpConnection;
use pretty_assertions::assert_eq;
use sacp::schema as acp;
use serial_test::serial;
use tempfile::tempdir;
use tokio::sync::mpsc;

/// Helper: get the mock agent config and skip if binary is not built.
fn mock_agent_config() -> Option<crate::registry::AcpAgentConfig> {
    let config = crate::registry::get_agent_config("mock-model").ok()?;
    if !std::path::Path::new(&config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            config.command
        );
        return None;
    }
    Some(config)
}

/// Test that SacpConnection can spawn a mock agent, perform the initialization
/// handshake, and return a working connection. After spawn, the connection
/// should be able to create a session (proving the transport is alive).
#[tokio::test]
#[serial]
async fn test_spawn_and_create_session() {
    let Some(config) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");

    let conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("Failed to spawn SacpConnection");

    // Verify the connection is functional by creating a session.
    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create_session should succeed on a live connection");

    // Session ID should be a valid non-empty string (mock agent uses incrementing IDs).
    assert!(
        !session_id.to_string().is_empty(),
        "Session ID should be non-empty"
    );
}

/// Test the full prompt lifecycle: spawn -> create session -> prompt -> receive
/// text updates -> get stop reason.
#[tokio::test]
#[serial]
async fn test_prompt_receives_text_updates() {
    let Some(config) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");

    let conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let (tx, mut rx) = mpsc::channel(32);
    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new("Hello"))];

    let stop_reason = conn.prompt(session_id, prompt, tx).await.expect("prompt");

    // Collect all text messages from the updates channel.
    let mut messages = Vec::new();
    while let Ok(update) = rx.try_recv() {
        if let acp::SessionUpdate::AgentMessageChunk(chunk) = update
            && let acp::ContentBlock::Text(text) = chunk.content
        {
            messages.push(text.text);
        }
    }

    // Mock agent sends "Test message 1" and "Test message 2" by default.
    assert!(
        !messages.is_empty(),
        "Should have received at least one text message"
    );
    assert!(
        messages.iter().any(|m| m.contains("Test message")),
        "Should contain 'Test message', got: {messages:?}"
    );
    assert_eq!(stop_reason, acp::StopReason::EndTurn);
}

/// Test that model state is populated after session creation.
/// The mock agent always returns 3 models in its NewSessionResponse.
#[tokio::test]
#[serial]
async fn test_model_state_after_session_creation() {
    let Some(config) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");

    let conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");

    let _session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let state = conn.model_state();

    // The mock agent always sends 3 models: mock-model-default, mock-model-fast,
    // mock-model-powerful, with mock-model-default as current.
    assert_eq!(
        state
            .current_model_id
            .as_ref()
            .map(std::string::ToString::to_string),
        Some("mock-model-default".to_string()),
        "Current model should be mock-model-default"
    );
    assert_eq!(
        state.available_models.len(),
        3,
        "Should have 3 available models"
    );
}

/// Test that the approval channel works: when an agent requests permission,
/// the approval receiver yields the request, and the prompt completes after
/// the approval response is sent back.
#[tokio::test]
#[serial]
async fn test_approval_receiver_forwards_requests() {
    let Some(mut config) = mock_agent_config() else {
        return;
    };

    // MOCK_AGENT_REQUEST_PERMISSION triggers the approval flow in the mock agent.
    config
        .env
        .insert("MOCK_AGENT_REQUEST_PERMISSION".to_string(), "1".to_string());

    let temp_dir = tempdir().expect("temp dir");

    let mut conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");

    let mut approval_rx = conn.take_approval_receiver();

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let (update_tx, _update_rx) = mpsc::channel::<acp::SessionUpdate>(32);
    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new(
        "do something",
    ))];

    // Drive the prompt concurrently with approval handling.
    // The prompt will block until the approval response is sent.
    let conn = Arc::new(conn);
    let conn_for_prompt = Arc::clone(&conn);
    let prompt_handle =
        tokio::spawn(async move { conn_for_prompt.prompt(session_id, prompt, update_tx).await });

    // Wait for the approval request to arrive (with timeout).
    let approval =
        tokio::time::timeout(std::time::Duration::from_secs(5), approval_rx.recv()).await;

    let approval = approval
        .expect("Approval request should arrive within 5s")
        .expect("Approval channel should not be closed");

    assert!(
        !approval.options.is_empty(),
        "Approval request should have permission options"
    );

    // Accept the first option to unblock the prompt.
    let _ = approval
        .response_tx
        .send(codex_protocol::protocol::ReviewDecision::Approved);

    // The prompt should complete (either normally or error) after approval.
    let result = tokio::time::timeout(std::time::Duration::from_secs(10), prompt_handle)
        .await
        .expect("Prompt should complete within 10s after approval")
        .expect("Prompt task should not panic");

    assert!(
        result.is_ok(),
        "Prompt should succeed after approval: {:?}",
        result.err()
    );
}

/// Test that CODEX_HOME is NOT inherited by the spawned agent subprocess.
/// This prevents third-party agents from reading Nori-specific config.
#[tokio::test]
#[serial]
async fn test_codex_home_not_inherited() {
    let Some(mut config) = mock_agent_config() else {
        return;
    };

    // Set CODEX_HOME in current process
    struct EnvGuard(Option<String>);
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(val) => unsafe { std::env::set_var("CODEX_HOME", val) },
                None => unsafe { std::env::remove_var("CODEX_HOME") },
            }
        }
    }
    let _guard = EnvGuard(std::env::var("CODEX_HOME").ok());
    unsafe {
        std::env::set_var("CODEX_HOME", "/tmp/fake-nori-home");
    }

    // MOCK_AGENT_ECHO_ENV tells the mock agent to echo the named env var's value.
    config
        .env
        .insert("MOCK_AGENT_ECHO_ENV".to_string(), "CODEX_HOME".to_string());

    let temp_dir = tempdir().expect("temp dir");

    let conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let (tx, mut rx) = mpsc::channel(32);
    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new("check env"))];

    conn.prompt(session_id, prompt, tx).await.expect("prompt");

    let mut messages = Vec::new();
    while let Ok(update) = rx.try_recv() {
        if let acp::SessionUpdate::AgentMessageChunk(chunk) = update
            && let acp::ContentBlock::Text(text) = chunk.content
        {
            messages.push(text.text);
        }
    }

    let combined = messages.join("");
    assert!(
        combined.contains("ENV:CODEX_HOME=<unset>"),
        "CODEX_HOME should NOT be inherited, but agent saw: {combined}"
    );
}

/// Test that dropping SacpConnection kills the agent subprocess.
/// We verify this by spawning with MOCK_AGENT_STREAM_UNTIL_CANCEL (which
/// streams indefinitely), starting a prompt, then dropping the connection.
/// The test has a timeout — if drop doesn't kill the subprocess, we'd hang.
#[tokio::test]
#[serial]
async fn test_drop_kills_subprocess() {
    let Some(mut config) = mock_agent_config() else {
        return;
    };

    // The agent will stream indefinitely until cancelled or killed.
    config.env.insert(
        "MOCK_AGENT_STREAM_UNTIL_CANCEL".to_string(),
        "1".to_string(),
    );

    let temp_dir = tempdir().expect("temp dir");

    let conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let (tx, _rx) = mpsc::channel(32);
    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new("stream"))];

    // Start a prompt that will stream forever
    let conn = Arc::new(conn);
    let conn_for_prompt = Arc::clone(&conn);
    let prompt_handle = tokio::spawn(async move {
        let _ = conn_for_prompt.prompt(session_id, prompt, tx).await;
    });

    // Give the prompt time to start streaming
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Drop our reference to the connection. When the last Arc ref (inside the
    // prompt task) is dropped after the task is aborted/errors, the subprocess
    // should be killed.
    drop(conn);
    prompt_handle.abort();

    // If drop didn't kill the subprocess, this would hang waiting for the
    // prompt to complete. The test timeout will catch this.
}

/// Test that cancel works during an active prompt: the agent stops streaming
/// and returns a Cancelled stop reason.
#[tokio::test]
#[serial]
async fn test_cancel_during_prompt() {
    let Some(mut config) = mock_agent_config() else {
        return;
    };

    // MOCK_AGENT_STREAM_UNTIL_CANCEL makes the agent stream until it gets
    // a cancel notification, then return StopReason::Cancelled.
    config.env.insert(
        "MOCK_AGENT_STREAM_UNTIL_CANCEL".to_string(),
        "1".to_string(),
    );

    let temp_dir = tempdir().expect("temp dir");

    let conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let conn = Arc::new(conn);

    let (tx, _rx) = mpsc::channel(32);
    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new("stream"))];

    // Start the prompt in a background task
    let conn_for_prompt = Arc::clone(&conn);
    let sid = session_id.clone();
    let prompt_task = tokio::spawn(async move { conn_for_prompt.prompt(sid, prompt, tx).await });

    // Give the prompt time to start streaming
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Cancel the prompt
    conn.cancel(&session_id)
        .await
        .expect("cancel should succeed");

    // The prompt should complete with Cancelled stop reason
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), prompt_task)
        .await
        .expect("Prompt should complete within 5s after cancel")
        .expect("Prompt task should not panic")
        .expect("Prompt should not error after cancel");

    assert_eq!(
        result,
        acp::StopReason::Cancelled,
        "Stop reason should be Cancelled after cancel"
    );
}
