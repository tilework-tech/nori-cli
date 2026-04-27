use std::sync::Arc;

use super::ApprovalRequest;
use super::ConnectionEvent;
use super::sacp_connection::SacpConnection;
use agent_client_protocol_schema as acp;
use pretty_assertions::assert_eq;
use serial_test::serial;
use tempfile::tempdir;

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

async fn recv_approval_request(
    event_rx: &mut tokio::sync::mpsc::Receiver<ConnectionEvent>,
) -> ApprovalRequest {
    loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("Approval request should arrive within 5s")
            .expect("Event channel should not be closed");

        if let ConnectionEvent::ApprovalRequest(approval) = event {
            return approval;
        }
    }
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

    let mut conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");

    let mut event_rx = conn.take_event_receiver();

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new("Hello"))];

    let stop_reason = conn.prompt(session_id, prompt).await.expect("prompt");

    // Collect all text messages from the ordered event inbox.
    let mut messages = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        if let ConnectionEvent::SessionUpdate(acp::SessionUpdate::AgentMessageChunk(chunk)) = event
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

/// Test that the connection exposes one ordered event inbox for prompt updates.
#[tokio::test]
#[serial]
async fn test_event_receiver_forwards_session_updates() {
    let Some(config) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");

    let mut conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");
    let mut event_rx = conn.take_event_receiver();

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new("Hello"))];
    let stop_reason = conn.prompt(session_id, prompt).await.expect("prompt");

    let mut messages = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        if let ConnectionEvent::SessionUpdate(acp::SessionUpdate::AgentMessageChunk(chunk)) = event
            && let acp::ContentBlock::Text(text) = chunk.content
        {
            messages.push(text.text);
        }
    }

    assert_eq!(stop_reason, acp::StopReason::EndTurn);
    assert!(
        messages.iter().any(|m| m.contains("Test message")),
        "Should contain prompt text updates from the ordered inbox, got: {messages:?}"
    );
}

#[tokio::test]
#[serial]
async fn test_tool_call_prompt_delivers_final_text_update() {
    use std::time::Duration;

    let Some(mut config) = mock_agent_config() else {
        return;
    };
    config
        .env
        .insert("MOCK_AGENT_SEND_TOOL_CALL".to_string(), "1".to_string());

    let temp_dir = tempdir().expect("temp dir");

    let mut conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");

    let mut event_rx = conn.take_event_receiver();

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new(
        "Do a tool call",
    ))];

    let stop_reason = conn.prompt(session_id, prompt).await.expect("prompt");

    let mut saw_final_text = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), event_rx.recv()).await {
            Ok(Some(ConnectionEvent::SessionUpdate(acp::SessionUpdate::AgentMessageChunk(
                chunk,
            )))) => {
                if let acp::ContentBlock::Text(text) = chunk.content
                    && text.text.contains("Tool call completed successfully.")
                {
                    saw_final_text = true;
                    break;
                }
            }
            Ok(Some(_)) => continue,
            Ok(None) => break,
            Err(_) => continue,
        }
    }

    assert_eq!(stop_reason, acp::StopReason::EndTurn);
    assert!(
        saw_final_text,
        "expected tool-call prompt to deliver final assistant text update"
    );
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

/// Test that approval requests flow through the ordered event inbox and the
/// prompt completes after the approval response is sent back.
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

    let mut event_rx = conn.take_event_receiver();

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new(
        "do something",
    ))];

    // Drive the prompt concurrently with approval handling.
    // The prompt will block until the approval response is sent.
    let conn = Arc::new(conn);
    let conn_for_prompt = Arc::clone(&conn);
    let prompt_handle =
        tokio::spawn(async move { conn_for_prompt.prompt(session_id, prompt).await });

    let approval = recv_approval_request(&mut event_rx).await;

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

/// Test that session updates and approval requests share one ordered inbox.
#[tokio::test]
#[serial]
async fn test_event_receiver_preserves_update_then_approval_order() {
    let Some(mut config) = mock_agent_config() else {
        return;
    };
    config
        .env
        .insert("MOCK_AGENT_REQUEST_PERMISSION".to_string(), "1".to_string());

    let temp_dir = tempdir().expect("temp dir");

    let mut conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");
    let mut event_rx = conn.take_event_receiver();

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new(
        "do something",
    ))];

    let conn = Arc::new(conn);
    let conn_for_prompt = Arc::clone(&conn);
    let prompt_handle =
        tokio::spawn(async move { conn_for_prompt.prompt(session_id, prompt).await });

    let first_event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
        .await
        .expect("expected first event within timeout")
        .expect("event channel should stay open");

    let approval = loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("expected approval event within timeout")
            .expect("event channel should stay open");

        if let ConnectionEvent::ApprovalRequest(approval) = event {
            break approval;
        }
    };

    assert!(
        matches!(
            first_event,
            ConnectionEvent::SessionUpdate(acp::SessionUpdate::AgentMessageChunk(_))
        ),
        "expected prompt updates to remain ordered ahead of the approval request"
    );
    assert!(
        !approval.options.is_empty(),
        "Approval request should have permission options"
    );

    let _ = approval
        .response_tx
        .send(codex_protocol::protocol::ReviewDecision::Approved);

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

    let mut conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");

    let mut event_rx = conn.take_event_receiver();

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new("check env"))];

    conn.prompt(session_id, prompt).await.expect("prompt");

    let mut messages = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        if let ConnectionEvent::SessionUpdate(acp::SessionUpdate::AgentMessageChunk(chunk)) = event
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

    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new("stream"))];

    // Start a prompt that will stream forever
    let conn = Arc::new(conn);
    let conn_for_prompt = Arc::clone(&conn);
    let prompt_handle = tokio::spawn(async move {
        let _ = conn_for_prompt.prompt(session_id, prompt).await;
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

    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new("stream"))];

    // Start the prompt in a background task
    let conn_for_prompt = Arc::clone(&conn);
    let sid = session_id.clone();
    let prompt_task = tokio::spawn(async move { conn_for_prompt.prompt(sid, prompt).await });

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
        .expect("Prompt task should not panic");
    let stop_reason = result.expect("Prompt should not error after cancel");

    assert_eq!(
        stop_reason,
        acp::StopReason::Cancelled,
        "Stop reason should be Cancelled after cancel"
    );
}

/// Test that a new prompt still receives updates after the previous prompt
/// was cancelled. This covers the user-visible regression where the old
/// prompt's stale cleanup could break the new prompt's update channel.
#[tokio::test]
#[serial]
async fn test_sequential_prompt_after_cancel_receives_response() {
    let Some(mut config) = mock_agent_config() else {
        return;
    };
    config.env.insert(
        "MOCK_AGENT_STREAM_UNTIL_CANCEL".to_string(),
        "1".to_string(),
    );

    let temp_dir = tempdir().expect("temp dir");

    let mut conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");
    let mut event_rx = conn.take_event_receiver();

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");
    let conn = Arc::new(conn);

    let prompt1 = vec![acp::ContentBlock::Text(acp::TextContent::new("hello"))];
    let conn_for_prompt1 = Arc::clone(&conn);
    let session_id_for_prompt1 = session_id.clone();
    let prompt1_task = tokio::spawn(async move {
        conn_for_prompt1
            .prompt(session_id_for_prompt1, prompt1)
            .await
    });

    let first_update = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
        .await
        .expect("Prompt 1 should start streaming within 5s")
        .expect("Event channel should stay open");
    assert!(
        matches!(
            first_update,
            ConnectionEvent::SessionUpdate(acp::SessionUpdate::AgentMessageChunk(_))
        ),
        "Prompt 1 should receive a streamed agent message before cancel"
    );

    conn.cancel(&session_id)
        .await
        .expect("prompt 1 cancel should succeed");

    let stop_reason_1 = tokio::time::timeout(std::time::Duration::from_secs(5), prompt1_task)
        .await
        .expect("Prompt 1 should complete within 5s after cancel")
        .expect("Prompt 1 task should not panic")
        .expect("Prompt 1 should not error after cancel");
    assert_eq!(stop_reason_1, acp::StopReason::Cancelled);

    while tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv())
        .await
        .is_ok()
    {}

    let prompt2 = vec![acp::ContentBlock::Text(acp::TextContent::new(
        "hello again",
    ))];
    let conn_for_prompt2 = Arc::clone(&conn);
    let session_id_for_prompt2 = session_id.clone();
    let prompt2_task = tokio::spawn(async move {
        conn_for_prompt2
            .prompt(session_id_for_prompt2, prompt2)
            .await
    });

    let second_update = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
        .await
        .expect("Prompt 2 should start streaming within 5s")
        .expect("Event channel should stay open");
    let second_text = match second_update {
        ConnectionEvent::SessionUpdate(acp::SessionUpdate::AgentMessageChunk(chunk)) => {
            match chunk.content {
                acp::ContentBlock::Text(text) => text.text,
                other => panic!("Prompt 2 should receive text content, got: {other:?}"),
            }
        }
        other => panic!("Prompt 2 should receive an agent text chunk, got: {other:?}"),
    };
    assert!(
        !second_text.is_empty(),
        "Prompt 2 should receive non-empty text updates after cancel"
    );

    conn.cancel(&session_id)
        .await
        .expect("prompt 2 cancel should succeed");

    let stop_reason_2 = tokio::time::timeout(std::time::Duration::from_secs(5), prompt2_task)
        .await
        .expect("Prompt 2 should complete within 5s after cancel")
        .expect("Prompt 2 task should not panic")
        .expect("Prompt 2 should not error after cancel");
    assert_eq!(stop_reason_2, acp::StopReason::Cancelled);
}

/// Test that an immediate empty end_turn after a cancelled prompt does not
/// consume the next logical prompt turn. The connection should absorb that
/// stale terminal response and keep working until the user's follow-up prompt
/// receives real streamed content.
#[tokio::test]
#[serial]
async fn test_prompt_after_cancel_absorbs_empty_end_turn_tail() {
    let Some(mut config) = mock_agent_config() else {
        return;
    };
    config.env.insert(
        "MOCK_AGENT_STREAM_UNTIL_CANCEL".to_string(),
        "1".to_string(),
    );
    config.env.insert(
        "MOCK_AGENT_CANCEL_TAIL_EMPTY_END_TURNS".to_string(),
        "2".to_string(),
    );
    config.env.insert(
        "MOCK_AGENT_CANCEL_TAIL_FOLLOW_UP_RESPONSE".to_string(),
        "Recovered after cancel tail".to_string(),
    );

    let temp_dir = tempdir().expect("temp dir");

    let mut conn = SacpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("spawn");
    let mut event_rx = conn.take_event_receiver();

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");
    let conn = Arc::new(conn);

    let prompt1 = vec![acp::ContentBlock::Text(acp::TextContent::new("hello"))];
    let conn_for_prompt1 = Arc::clone(&conn);
    let session_id_for_prompt1 = session_id.clone();
    let prompt1_task = tokio::spawn(async move {
        conn_for_prompt1
            .prompt(session_id_for_prompt1, prompt1)
            .await
    });

    let first_update = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
        .await
        .expect("Prompt 1 should start streaming within 5s")
        .expect("Event channel should stay open");
    assert!(
        matches!(
            first_update,
            ConnectionEvent::SessionUpdate(acp::SessionUpdate::AgentMessageChunk(_))
        ),
        "Prompt 1 should receive a streamed agent message before cancel"
    );

    conn.cancel(&session_id)
        .await
        .expect("prompt 1 cancel should succeed");

    let stop_reason_1 = tokio::time::timeout(std::time::Duration::from_secs(5), prompt1_task)
        .await
        .expect("Prompt 1 should complete within 5s after cancel")
        .expect("Prompt 1 task should not panic")
        .expect("Prompt 1 should not error after cancel");
    assert_eq!(stop_reason_1, acp::StopReason::Cancelled);

    while tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv())
        .await
        .is_ok()
    {}

    let prompt2 = vec![acp::ContentBlock::Text(acp::TextContent::new(
        "what have you finished?",
    ))];
    let conn_for_prompt2 = Arc::clone(&conn);
    let session_id_for_prompt2 = session_id.clone();
    let prompt2_task = tokio::spawn(async move {
        conn_for_prompt2
            .prompt(session_id_for_prompt2, prompt2)
            .await
    });

    let second_update = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let event = event_rx
                .recv()
                .await
                .expect("Event channel should stay open");
            if let ConnectionEvent::SessionUpdate(acp::SessionUpdate::AgentMessageChunk(chunk)) =
                event
                && let acp::ContentBlock::Text(text) = chunk.content
            {
                return text.text;
            }
        }
    })
    .await
    .expect("Prompt 2 should receive streamed text after the stale end_turn tail is absorbed");

    assert!(
        second_update.contains("Recovered after cancel tail"),
        "Prompt 2 should receive its real response after the stale cancel tail, got: {second_update:?}"
    );

    let stop_reason_2 = tokio::time::timeout(std::time::Duration::from_secs(5), prompt2_task)
        .await
        .expect("Prompt 2 should complete within 5s")
        .expect("Prompt 2 task should not panic")
        .expect("Prompt 2 should not error");
    assert_eq!(stop_reason_2, acp::StopReason::EndTurn);
}
