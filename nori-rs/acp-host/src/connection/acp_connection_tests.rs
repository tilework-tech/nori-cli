use std::sync::Arc;

use super::ConnectionEvent;
use super::DelegatedRequest;
use super::acp_connection::AcpConnection;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;
use serde_json::Value;
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
) -> DelegatedRequest {
    loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("Approval request should arrive within 5s")
            .expect("Event channel should not be closed");

        if let ConnectionEvent::DelegatedRequest(approval) = event {
            return approval;
        }
    }
}

async fn recv_agent_message_text(
    event_rx: &mut tokio::sync::mpsc::Receiver<ConnectionEvent>,
) -> String {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
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
    .expect("Agent message should arrive within 5s")
}

fn approve_request(approval: DelegatedRequest) {
    let acp::AgentRequest::RequestPermissionRequest(request) = approval.request else {
        panic!("expected a permission request");
    };
    let option_id = request
        .options
        .first()
        .expect("permission request should have an option")
        .option_id
        .clone();
    let response = acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Selected(
        acp::SelectedPermissionOutcome::new(option_id),
    ));
    let _ = approval
        .response_tx
        .send(Ok(acp::ClientResponse::RequestPermissionResponse(response)));
}

async fn drive_logged_prompt(
    config: &crate::registry::AcpAgentConfig,
    cwd: &std::path::Path,
    proxy: nori_config::AcpProxyConfig,
) {
    let conn = AcpConnection::spawn(config, cwd, proxy)
        .await
        .expect("spawn logged connection");
    let session_id = conn
        .create_session(cwd, vec![])
        .await
        .expect("create session");
    conn.prompt(
        session_id,
        vec![acp::ContentBlock::Text(acp::TextContent::new("Hello"))],
    )
    .await
    .expect("prompt");
    conn.shutdown().await;
}

fn read_wire_log(path: &std::path::Path) -> Vec<Value> {
    let content = std::fs::read_to_string(path).expect("read wire log");
    content
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("wire log line is json"))
        .collect()
}

/// Test that AcpConnection can spawn a mock agent, perform the initialization
/// handshake, and return a working connection. After spawn, the connection
/// should be able to create a session (proving the transport is alive).
#[tokio::test]
#[serial]
async fn test_spawn_and_create_session() {
    let Some(config) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");

    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("Failed to spawn AcpConnection");

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

#[tokio::test]
#[serial]
async fn test_proxy_logging_records_one_wire_log_per_child_subprocess() {
    let Some(config) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");
    let proxy = nori_config::AcpProxyConfig {
        enabled: true,
        log_dir: temp_dir.path().join("acp-wire"),
    };

    drive_logged_prompt(&config, temp_dir.path(), proxy.clone()).await;
    drive_logged_prompt(&config, temp_dir.path(), proxy.clone()).await;

    let mut log_paths = std::fs::read_dir(&proxy.log_dir)
        .expect("wire log dir exists")
        .map(|entry| entry.expect("wire log entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect::<Vec<_>>();
    log_paths.sort();

    assert_eq!(
        log_paths.len(),
        2,
        "expected one JSONL wire log per spawned ACP child subprocess"
    );

    for path in log_paths {
        let records = read_wire_log(&path);
        assert!(
            records.iter().any(|record| {
                record["direction"] == "client_to_agent"
                    && record["message"]["method"] == "initialize"
            }),
            "expected client initialize request in {path:?}: {records:#?}"
        );
        assert!(
            records.iter().any(|record| {
                record["direction"] == "client_to_agent"
                    && record["message"]["method"] == "session/prompt"
            }),
            "expected client prompt request in {path:?}: {records:#?}"
        );
        assert!(
            records.iter().any(|record| {
                record["direction"] == "agent_to_client"
                    && record["message"]["method"] == "session/update"
            }),
            "expected agent session/update notification in {path:?}: {records:#?}"
        );
    }
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

    let mut conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
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

    let mut conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
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

    let mut conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
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

#[tokio::test]
#[serial]
async fn test_session_config_options_after_session_creation() {
    let Some(config) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");

    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("spawn");

    let _session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let config_options = conn.config_options();
    let option_names = config_options
        .iter()
        .map(|option| option.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(option_names, vec!["Model", "Thought Level"]);
}

#[tokio::test]
#[serial]
async fn test_set_session_config_option_replaces_connection_state() {
    let Some(config) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");

    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("spawn");

    let session_id = conn
        .create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    conn.set_config_option(&session_id, "model", "mock-model-fast")
        .await
        .expect("set config option");

    let config_options = conn.config_options();
    let option_names = config_options
        .iter()
        .map(|option| option.name.as_str())
        .collect::<Vec<_>>();

    assert_eq!(option_names, vec!["Model", "Speed"]);
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

    let mut conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
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

    approve_request(approval);

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

    let mut conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
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

    let mut saw_agent_message = false;
    let approval = loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(5), event_rx.recv())
            .await
            .expect("expected approval event within timeout")
            .expect("event channel should stay open");

        match event {
            ConnectionEvent::SessionUpdate(acp::SessionUpdate::AgentMessageChunk(_)) => {
                saw_agent_message = true;
            }
            ConnectionEvent::DelegatedRequest(approval) => break approval,
            ConnectionEvent::Acp(_)
            | ConnectionEvent::SessionClosed
            | ConnectionEvent::SessionUpdate(_)
            | ConnectionEvent::ChildExited { .. } => {}
        }
    };

    assert!(
        saw_agent_message,
        "expected prompt updates to remain ordered ahead of the approval request"
    );
    approve_request(approval);

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

    let mut conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
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

/// Test that dropping AcpConnection kills the agent subprocess.
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

    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
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

    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
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

    let mut conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
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

    let first_update = recv_agent_message_text(&mut event_rx).await;
    assert!(!first_update.is_empty());

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

    let second_text = recv_agent_message_text(&mut event_rx).await;
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

// ============================================================================
// Child lifecycle: graceful teardown, exit detection, stderr surfacing
// ============================================================================

/// Write an executable shell script into `dir` and return an agent config
/// that spawns it.
fn script_agent_config(dir: &std::path::Path, body: &str) -> crate::registry::AcpAgentConfig {
    let script = dir.join("script-agent.sh");
    std::fs::write(&script, body).expect("write script agent");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod script agent");
    }
    crate::registry::AcpAgentConfig {
        agent: crate::registry::AgentKind::ClaudeCode, // placeholder, like custom agents
        provider_slug: "script-agent".to_string(),
        command: script.to_string_lossy().to_string(),
        args: vec![],
        env: std::collections::HashMap::new(),
        provider_info: crate::registry::AcpProviderInfo::default(),
        auth_hint: "run: script-agent login".to_string(),
        display_name: "Script Agent".to_string(),
        install_hint: "none".to_string(),
    }
}

/// Shutdown must close the child's stdin and wait for it to exit on its own —
/// not SIGKILL the process group. The script writes a marker after the mock
/// agent exits (which it does on stdin EOF); a killed group never writes it.
#[tokio::test]
async fn test_shutdown_closes_stdin_and_waits_for_child_exit() {
    let Some(mock) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");
    let marker = temp_dir.path().join("released");
    let config = script_agent_config(
        temp_dir.path(),
        &format!(
            "#!/bin/sh\n'{mock}'\necho done > '{marker}'\n",
            mock = mock.command,
            marker = marker.display(),
        ),
    );

    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("spawn script agent");
    conn.create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    conn.shutdown().await;

    // shutdown waited for the child, so the post-EOF marker must exist
    // (allow a brief fs delay).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !marker.exists() && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        marker.exists(),
        "child must get stdin EOF and finish its cleanup before being reaped; \
         a missing marker means shutdown killed the process group immediately"
    );
}

/// A child that ignores stdin EOF is killed after the grace period — shutdown
/// must wait out the full grace first (not kill instantly), must not hang
/// forever, and the child must actually die.
#[tokio::test]
#[cfg(target_os = "linux")]
async fn test_shutdown_kills_child_that_outlives_grace() {
    let Some(mock) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");
    let pidfile = temp_dir.path().join("pid");
    let config = script_agent_config(
        temp_dir.path(),
        &format!(
            "#!/bin/sh\necho $$ > '{pidfile}'\n'{mock}'\nsleep 600\n",
            mock = mock.command,
            pidfile = pidfile.display(),
        ),
    );

    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("spawn script agent");
    conn.create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");

    let start = std::time::Instant::now();
    conn.shutdown_with_grace(std::time::Duration::from_millis(300))
        .await;
    assert!(
        start.elapsed() >= std::time::Duration::from_millis(300),
        "shutdown must give the child the full grace period before killing, took {:?}",
        start.elapsed()
    );
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "shutdown must not wait for a hung child beyond the grace period"
    );

    let pid: i32 = std::fs::read_to_string(&pidfile)
        .expect("script should have written its pid")
        .trim()
        .parse()
        .expect("pid parses");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let proc_path = format!("/proc/{pid}");
    while std::path::Path::new(&proc_path).exists() && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        !std::path::Path::new(&proc_path).exists(),
        "the hung child (pid {pid}) must be killed after the grace period"
    );
}

/// An unexpected child death must surface through the connection event
/// stream with the child's recent stderr — not leave the session silently
/// hung.
#[tokio::test]
async fn test_child_exit_emits_event_with_stderr_tail() {
    let Some(mock) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");
    let trigger = temp_dir.path().join("die");
    let config = script_agent_config(
        temp_dir.path(),
        &format!(
            "#!/bin/sh\n\
             ( while [ ! -f '{trigger}' ]; do sleep 0.1; done; \
               echo 'simulated crash' >&2; kill -9 $$ ) &\n\
             exec '{mock}'\n",
            trigger = trigger.display(),
            mock = mock.command,
        ),
    );

    let mut conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("spawn script agent");
    conn.create_session(temp_dir.path(), vec![])
        .await
        .expect("create session");
    std::fs::write(&trigger, "now").expect("write crash trigger");

    let mut event_rx = conn.take_event_receiver();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let exited = loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let event = tokio::time::timeout(remaining, event_rx.recv())
            .await
            .expect("child exit should surface as an event before the timeout")
            .expect("event channel closed without reporting the child exit");
        match event {
            ConnectionEvent::ChildExited {
                status,
                stderr_tail,
            } => {
                break (status, stderr_tail);
            }
            ConnectionEvent::Acp(_)
            | ConnectionEvent::SessionClosed
            | ConnectionEvent::SessionUpdate(_)
            | ConnectionEvent::DelegatedRequest(_) => {}
        }
    };

    let (status, stderr_tail) = exited;
    assert_eq!(status, None, "a SIGKILLed child has no exit code");
    assert!(
        stderr_tail.contains("simulated crash"),
        "the event must carry the child's recent stderr, got: {stderr_tail:?}"
    );
}

/// When the child exits during spawn/initialize (e.g. unauthenticated), the
/// spawn error must include the child's stderr so the real cause is visible
/// and categorizable.
#[tokio::test]
async fn test_spawn_failure_surfaces_child_stderr() {
    let temp_dir = tempdir().expect("temp dir");
    let config = script_agent_config(
        temp_dir.path(),
        "#!/bin/sh\necho 'Error: not authenticated - run: nori-handroll login' >&2\nexit 1\n",
    );

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        AcpConnection::spawn(
            &config,
            temp_dir.path(),
            nori_config::AcpProxyConfig::disabled(),
        ),
    )
    .await
    .expect("spawn must fail fast when the child exits immediately, not hang");

    let err = result.err().expect("spawn should fail for a dead child");
    let err_text = format!("{err:?}");
    assert!(
        err_text.contains("not authenticated"),
        "spawn error must include the child's stderr, got: {err_text}"
    );
    assert_eq!(
        crate::categorize_acp_error(&err_text),
        crate::AcpErrorCategory::Authentication,
        "the surfaced stderr must drive categorization (auth, not init failure)"
    );
}

/// `list_sessions` drains the agent's `session/list` pages without projecting
/// ACP-owned session data into a host-owned mirror.
#[tokio::test]
#[serial]
async fn test_list_sessions_maps_agent_session_info() {
    let Some(config) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");

    // SAFETY: serialized test; the variable is removed before any assertion
    // that could unwind, and `#[serial]` keeps env mutation isolated.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_LIST", "1");
    }

    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("Failed to spawn AcpConnection");

    assert!(
        conn.capabilities().session_capabilities.list.is_some(),
        "mock should advertise session/list when env-gated on"
    );

    let summaries = conn
        .list_sessions(temp_dir.path())
        .await
        .expect("list_sessions should succeed against a live agent");

    // SAFETY: cleaning up the env var we set above.
    unsafe {
        std::env::remove_var("MOCK_AGENT_SUPPORT_SESSION_LIST");
    }

    let expected = vec![
        acp::SessionInfo::new("mock-session-1", temp_dir.path())
            .title("First mock session")
            .updated_at("2026-01-02T03:04:05Z"),
        acp::SessionInfo::new("mock-session-2", temp_dir.path()),
    ];
    assert_eq!(summaries, expected);
}

/// A session's ACP `_meta` extension payload survives the trip from the agent's
/// `session/list` response through the schema-native result. The env-gated mock attaches
/// `_meta = {"nori": {"origin": "cloud"}}` to its first row only; the CLI must
/// surface that object (and must not leak it onto other rows) so the resume
/// picker can distinguish cloud sessions without the legacy cwd == "/"
/// sentinel.
#[tokio::test]
#[serial]
async fn test_list_sessions_preserves_session_meta() {
    let Some(config) = mock_agent_config() else {
        return;
    };
    let temp_dir = tempdir().expect("temp dir");

    // SAFETY: serialized test; both variables are removed before any assertion
    // that could unwind, and `#[serial]` keeps env mutation isolated.
    unsafe {
        std::env::set_var("MOCK_AGENT_SUPPORT_SESSION_LIST", "1");
        std::env::set_var(
            "MOCK_AGENT_LIST_SESSIONS_META",
            r#"{"nori":{"origin":"cloud"}}"#,
        );
    }

    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("Failed to spawn AcpConnection");

    let summaries = conn
        .list_sessions(temp_dir.path())
        .await
        .expect("list_sessions should succeed against a live agent");

    // SAFETY: cleaning up the env vars we set above.
    unsafe {
        std::env::remove_var("MOCK_AGENT_SUPPORT_SESSION_LIST");
        std::env::remove_var("MOCK_AGENT_LIST_SESSIONS_META");
    }

    let meta = serde_json::json!({"nori": {"origin": "cloud"}})
        .as_object()
        .cloned()
        .expect("object metadata");
    let expected = vec![
        acp::SessionInfo::new("mock-session-1", temp_dir.path())
            .title("First mock session")
            .updated_at("2026-01-02T03:04:05Z")
            .meta(meta),
        acp::SessionInfo::new("mock-session-2", temp_dir.path()),
    ];
    assert_eq!(summaries, expected);
}

/// `resume_session` reattaches to an agent-side session over ACP
/// `session/resume` — the reattach path for agents (like nori cloud) that
/// advertise `sessionCapabilities.resume` with `loadSession: false`.
/// `MOCK_AGENT_FAIL_NEW_SESSION_FROM=0` makes any silent `session/new`
/// fallback fail loudly.
#[tokio::test]
async fn test_resume_session_reattaches_over_session_resume() {
    let Some(mut config) = mock_agent_config() else {
        return;
    };
    config.env.insert(
        "MOCK_AGENT_SUPPORT_SESSION_RESUME".to_string(),
        "1".to_string(),
    );
    config.env.insert(
        "MOCK_AGENT_FAIL_NEW_SESSION_FROM".to_string(),
        "0".to_string(),
    );

    let temp_dir = tempdir().expect("temp dir");
    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("Failed to spawn AcpConnection");

    assert!(
        conn.capabilities().session_capabilities.resume.is_some(),
        "mock should advertise session/resume when env-gated on"
    );

    let session_id = conn
        .resume_session("mock-session-1", temp_dir.path(), Vec::new())
        .await
        .expect("resume_session should succeed against a live agent");
    assert_eq!(session_id.to_string(), "mock-session-1");
}

/// A structured `session/resume` failure must stay categorizable: the agent's
/// error code (-32002 resource_not_found) and `data.detail` survive the real
/// stdio transport and the anyhow chain so callers can show a precise message.
#[tokio::test]
async fn test_resume_session_failure_categorizes_as_session_not_found() {
    let Some(mut config) = mock_agent_config() else {
        return;
    };
    config.env.insert(
        "MOCK_AGENT_SUPPORT_SESSION_RESUME".to_string(),
        "1".to_string(),
    );
    config.env.insert(
        "MOCK_AGENT_RESUME_SESSION_FAIL".to_string(),
        "1".to_string(),
    );

    let temp_dir = tempdir().expect("temp dir");
    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("Failed to spawn AcpConnection");

    let err = conn
        .resume_session("gone-session", temp_dir.path(), Vec::new())
        .await
        .expect_err("resume_session should fail when the agent rejects the id");
    let details = crate::categorize_acp_error_chain(&err);
    assert_eq!(
        (details.category, details.detail.as_deref()),
        (
            crate::AcpErrorCategory::SessionNotFound,
            Some("the session is no longer claimed")
        )
    );
}

/// `close_session` releases an agent-side session over ACP `session/close`.
#[tokio::test]
async fn test_close_session_round_trips() {
    let Some(mut config) = mock_agent_config() else {
        return;
    };
    config.env.insert(
        "MOCK_AGENT_SUPPORT_SESSION_CLOSE".to_string(),
        "1".to_string(),
    );

    let temp_dir = tempdir().expect("temp dir");
    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("Failed to spawn AcpConnection");

    assert!(
        conn.capabilities().session_capabilities.close.is_some(),
        "mock should advertise session/close when env-gated on"
    );

    conn.close_session("mock-session-1")
        .await
        .expect("close_session should succeed against a live agent");
}

/// A structured `session/close` failure propagates — proving the request
/// actually crosses the process boundary (a no-op `close_session` could not
/// observe the agent-side error).
#[tokio::test]
async fn test_close_session_failure_categorizes_as_session_not_found() {
    let Some(mut config) = mock_agent_config() else {
        return;
    };
    config.env.insert(
        "MOCK_AGENT_SUPPORT_SESSION_CLOSE".to_string(),
        "1".to_string(),
    );
    config
        .env
        .insert("MOCK_AGENT_CLOSE_SESSION_FAIL".to_string(), "1".to_string());

    let temp_dir = tempdir().expect("temp dir");
    let conn = AcpConnection::spawn(
        &config,
        temp_dir.path(),
        nori_config::AcpProxyConfig::disabled(),
    )
    .await
    .expect("Failed to spawn AcpConnection");

    let err = conn
        .close_session("gone-session")
        .await
        .expect_err("close_session should fail when the agent rejects the id");
    assert_eq!(
        crate::categorize_acp_error_chain(&err).category,
        crate::AcpErrorCategory::SessionNotFound
    );
}
