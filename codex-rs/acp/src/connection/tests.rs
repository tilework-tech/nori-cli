use super::*;
use serial_test::serial;
use tempfile::tempdir;

/// Test that we can spawn an ACP connection and receive responses from the mock agent.
/// This is an integration test using the real mock-acp-agent binary.
#[tokio::test]
#[serial]
async fn test_spawn_connection_and_receive_response() {
    // Get the mock agent config
    let config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");

    // Check if mock agent binary exists
    if !std::path::Path::new(&config.command).exists() {
        // Skip test if binary not built
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            config.command
        );
        return;
    }

    let temp_dir = tempdir().expect("Failed to create temp dir");

    // Spawn connection
    let conn = AcpConnection::spawn(&config, temp_dir.path())
        .await
        .expect("Failed to spawn ACP connection");

    // Create session
    let session_id = conn
        .create_session(temp_dir.path())
        .await
        .expect("Failed to create session");

    // Send prompt and collect updates
    let (tx, mut rx) = mpsc::channel(32);
    let prompt = vec![acp::ContentBlock::Text(acp::TextContent::new("Hello"))];

    let stop_reason = conn
        .prompt(session_id, prompt, tx)
        .await
        .expect("Prompt failed");

    // Should have received responses
    let mut messages = Vec::new();
    while let Ok(update) = rx.try_recv() {
        if let acp::SessionUpdate::AgentMessageChunk(chunk) = update
            && let acp::ContentBlock::Text(text) = chunk.content
        {
            messages.push(text.text);
        }
    }

    // Mock agent sends "Test message 1" and "Test message 2"
    assert!(
        !messages.is_empty(),
        "Should have received at least one message"
    );
    assert!(
        messages.iter().any(|m| m.contains("Test message")),
        "Should contain test message, got: {messages:?}"
    );
    assert_eq!(stop_reason, acp::StopReason::EndTurn);
}

/// Test that read_text_file emits a ToolCall SessionUpdate event.
/// This enables TUI rendering of file read operations for agents like Gemini
/// that use client capability methods instead of session/update notifications.
#[tokio::test]
async fn test_read_text_file_emits_tool_call_event() {
    use acp::Client;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let test_file = temp_dir.path().join("test.txt");
    std::fs::write(&test_file, "test content").expect("Failed to write test file");

    // Create ClientDelegate with a session registered
    let (approval_tx, _approval_rx) = mpsc::channel(16);
    let delegate = ClientDelegate::new(temp_dir.path().to_path_buf(), approval_tx);

    // Register a session and capture updates
    let session_id = acp::SessionId::from("test-session-123".to_string());
    let (update_tx, mut update_rx) = mpsc::channel(32);
    delegate.register_session(session_id.clone(), update_tx);

    // Call read_text_file
    let request = acp::ReadTextFileRequest::new(session_id.clone(), test_file.clone());
    let response = delegate
        .read_text_file(request)
        .await
        .expect("read_text_file should succeed");

    // Verify the file was read
    assert_eq!(response.content, "test content");

    // Verify that a ToolCall SessionUpdate was emitted
    let update = update_rx
        .try_recv()
        .expect("Should have received a SessionUpdate");

    match update {
        acp::SessionUpdate::ToolCall(tool_call) => {
            assert_eq!(tool_call.status, acp::ToolCallStatus::Pending);
            assert!(
                tool_call.title.contains("read_text_file")
                    || tool_call.title.contains("Reading")
                    || tool_call.title.contains("test.txt"),
                "Title should indicate file read operation, got: {}",
                tool_call.title
            );
            assert_eq!(tool_call.kind, acp::ToolKind::Execute);
        }
        other => panic!("Expected ToolCall update, got: {other:?}"),
    }

    delegate.unregister_session(&session_id);
}

/// Test that write_text_file emits a ToolCall SessionUpdate event.
/// This enables TUI rendering of file write operations for agents like Gemini
/// that use client capability methods instead of session/update notifications.
#[tokio::test]
async fn test_write_text_file_emits_tool_call_event() {
    use acp::Client;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let test_file = temp_dir.path().join("output.txt");

    // Create ClientDelegate with a session registered
    let (approval_tx, _approval_rx) = mpsc::channel(16);
    let delegate = ClientDelegate::new(temp_dir.path().to_path_buf(), approval_tx);

    // Register a session and capture updates
    let session_id = acp::SessionId::from("test-session-456".to_string());
    let (update_tx, mut update_rx) = mpsc::channel(32);
    delegate.register_session(session_id.clone(), update_tx);

    // Call write_text_file
    let content = "Hello, world!";
    let request =
        acp::WriteTextFileRequest::new(session_id.clone(), test_file.clone(), content.to_string());
    let response = delegate
        .write_text_file(request)
        .await
        .expect("write_text_file should succeed");

    // Verify the response is valid
    assert_eq!(response, acp::WriteTextFileResponse::new());

    // Verify the file was written
    let written_content = std::fs::read_to_string(&test_file).expect("File should exist");
    assert_eq!(written_content, content);

    // Verify that a ToolCall SessionUpdate was emitted
    let update = update_rx
        .try_recv()
        .expect("Should have received a SessionUpdate");

    match update {
        acp::SessionUpdate::ToolCall(tool_call) => {
            assert_eq!(tool_call.status, acp::ToolCallStatus::Pending);
            assert!(
                tool_call.title.contains("write_text_file")
                    || tool_call.title.contains("Writing")
                    || tool_call.title.contains("output.txt"),
                "Title should indicate file write operation, got: {}",
                tool_call.title
            );
            assert_eq!(tool_call.kind, acp::ToolKind::Execute);
        }
        other => panic!("Expected ToolCall update, got: {other:?}"),
    }

    delegate.unregister_session(&session_id);
}

/// Test that notifications for unregistered sessions are forwarded to the
/// persistent listener instead of being silently dropped.
/// This covers the case where the ACP agent sends events between turns
/// (after unregister_session is called but before the next prompt registers).
#[tokio::test]
async fn test_persistent_listener_receives_inter_turn_notifications() {
    use acp::Client;

    let temp_dir = tempdir().expect("Failed to create temp dir");
    let (approval_tx, _approval_rx) = mpsc::channel(16);
    let delegate = ClientDelegate::new(temp_dir.path().to_path_buf(), approval_tx);

    // Set up a persistent listener
    let (persistent_tx, mut persistent_rx) = mpsc::channel(32);
    delegate.set_persistent_listener(persistent_tx);

    // Register a per-prompt session, then unregister it (simulating end-of-turn)
    let session_id = acp::SessionId::from("session-between-turns".to_string());
    let (prompt_tx, _prompt_rx) = mpsc::channel(32);
    delegate.register_session(session_id.clone(), prompt_tx);
    delegate.unregister_session(&session_id);

    // Send a notification for the now-unregistered session (inter-turn event)
    let update = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("background event")),
    ));
    let notification = acp::SessionNotification::new(session_id.clone(), update);
    delegate
        .session_notification(notification)
        .await
        .expect("session_notification should succeed");

    // The persistent listener should have received the notification
    let received = persistent_rx
        .try_recv()
        .expect("Persistent listener should receive inter-turn notification");

    match received {
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = chunk.content {
                assert_eq!(text.text, "background event");
            } else {
                panic!("Expected text content, got: {:?}", chunk.content);
            }
        }
        other => panic!("Expected AgentMessageChunk, got: {other:?}"),
    }
}
