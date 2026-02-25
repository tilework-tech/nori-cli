use super::*;

/// Test that ToolCall with Execute kind generates command-mode parsed_cmd.
#[test]
fn test_tool_call_execute_generates_command_parsed_cmd() {
    let update = acp::SessionUpdate::ToolCall(
        acp::ToolCall::new(acp::ToolCallId::from("call-exec".to_string()), "Terminal")
            .kind(acp::ToolKind::Execute)
            .status(acp::ToolCallStatus::InProgress)
            .raw_input(serde_json::json!({"command": "cargo test"})),
    );

    let mut pending = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandBegin(begin) => {
            assert_eq!(begin.parsed_cmd.len(), 1);
            match &begin.parsed_cmd[0] {
                ParsedCommand::Unknown { cmd } => {
                    assert!(cmd.contains("cargo test"));
                }
                _ => panic!("Expected ParsedCommand::Unknown"),
            }
        }
        _ => panic!("Expected ExecCommandBegin event"),
    }
}

/// Test that ToolCallUpdate with Read kind generates exploring parsed_cmd in ExecCommandEnd.
#[test]
fn test_tool_call_update_read_generates_exploring_parsed_cmd() {
    let update = acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::from("call-read-end".to_string()),
        acp::ToolCallUpdateFields::new()
            .status(acp::ToolCallStatus::Completed)
            .title("Read File")
            .kind(acp::ToolKind::Read)
            .raw_input(serde_json::json!({"path": "Cargo.toml"})),
    ));

    let mut pending = std::collections::HashMap::new();
    let events = translate_session_update_to_events(&update, &mut pending);
    assert_eq!(events.len(), 1);

    match &events[0] {
        EventMsg::ExecCommandEnd(end) => {
            assert_eq!(end.parsed_cmd.len(), 1);
            match &end.parsed_cmd[0] {
                ParsedCommand::Read { name, .. } => {
                    assert_eq!(name, "Cargo.toml");
                }
                _ => panic!("Expected ParsedCommand::Read"),
            }
        }
        _ => panic!("Expected ExecCommandEnd event"),
    }
}

/// Test that authentication errors are correctly categorized
#[test]
fn test_categorize_acp_error_authentication() {
    // Test various authentication error patterns
    assert_eq!(
        categorize_acp_error("Authentication required"),
        AcpErrorCategory::Authentication
    );
    assert_eq!(
        categorize_acp_error("Error code -32000: not authenticated"),
        AcpErrorCategory::Authentication
    );
    assert_eq!(
        categorize_acp_error("Invalid API key"),
        AcpErrorCategory::Authentication
    );
    assert_eq!(
        categorize_acp_error("Unauthorized access"),
        AcpErrorCategory::Authentication
    );
    assert_eq!(
        categorize_acp_error("User not logged in"),
        AcpErrorCategory::Authentication
    );
}

/// Test that quota/rate limit errors are correctly categorized
#[test]
fn test_categorize_acp_error_quota() {
    assert_eq!(
        categorize_acp_error("Quota exceeded"),
        AcpErrorCategory::QuotaExceeded
    );
    assert_eq!(
        categorize_acp_error("Rate limit reached"),
        AcpErrorCategory::QuotaExceeded
    );
    assert_eq!(
        categorize_acp_error("Too many requests"),
        AcpErrorCategory::QuotaExceeded
    );
    assert_eq!(
        categorize_acp_error("HTTP 429: Too Many Requests"),
        AcpErrorCategory::QuotaExceeded
    );
}

/// Test that executable not found errors are correctly categorized
#[test]
fn test_categorize_acp_error_executable_not_found() {
    assert_eq!(
        categorize_acp_error("npx: command not found"),
        AcpErrorCategory::ExecutableNotFound
    );
    assert_eq!(
        categorize_acp_error("bunx: command not found"),
        AcpErrorCategory::ExecutableNotFound
    );
    assert_eq!(
        categorize_acp_error("No such file or directory: /usr/bin/claude"),
        AcpErrorCategory::ExecutableNotFound
    );
    assert_eq!(
        categorize_acp_error("command not found: gemini"),
        AcpErrorCategory::ExecutableNotFound
    );
}

/// Test that initialization errors are correctly categorized
#[test]
fn test_categorize_acp_error_initialization() {
    assert_eq!(
        categorize_acp_error("ACP initialization failed"),
        AcpErrorCategory::Initialization
    );
    assert_eq!(
        categorize_acp_error("Protocol handshake error"),
        AcpErrorCategory::Initialization
    );
    assert_eq!(
        categorize_acp_error("Protocol version mismatch"),
        AcpErrorCategory::Initialization
    );
}

/// Test that unknown errors fall back to Unknown category
#[test]
fn test_categorize_acp_error_unknown() {
    assert_eq!(
        categorize_acp_error("Some random error message"),
        AcpErrorCategory::Unknown
    );
    assert_eq!(
        categorize_acp_error("Connection timeout"),
        AcpErrorCategory::Unknown
    );
    assert_eq!(
        categorize_acp_error("Unexpected end of input"),
        AcpErrorCategory::Unknown
    );
}

/// Test that error categorization is case-insensitive
#[test]
fn test_categorize_acp_error_case_insensitive() {
    assert_eq!(
        categorize_acp_error("AUTHENTICATION REQUIRED"),
        AcpErrorCategory::Authentication
    );
    assert_eq!(
        categorize_acp_error("QUOTA EXCEEDED"),
        AcpErrorCategory::QuotaExceeded
    );
    assert_eq!(
        categorize_acp_error("NPX: COMMAND NOT FOUND"),
        AcpErrorCategory::ExecutableNotFound
    );
}

/// Test that protocol "not found" errors are NOT classified as ExecutableNotFound.
/// These are legitimate ACP errors that should fall through to Unknown.
#[test]
fn test_protocol_not_found_is_not_executable_not_found() {
    // Resource not found is a protocol error, not a missing executable
    assert_ne!(
        categorize_acp_error("Resource not found: session-123"),
        AcpErrorCategory::ExecutableNotFound,
        "Protocol errors should not be ExecutableNotFound"
    );
    // Model not found is a business error, not a missing executable
    assert_ne!(
        categorize_acp_error("Model not found: gpt-999"),
        AcpErrorCategory::ExecutableNotFound,
        "Model errors should not be ExecutableNotFound"
    );
    // File not found (without "directory") should not trigger false positive
    assert_ne!(
        categorize_acp_error("File not found"),
        AcpErrorCategory::ExecutableNotFound,
        "Generic 'file not found' should not be ExecutableNotFound"
    );
}

/// Test that API server errors (500, 502, etc.) are categorized as ApiServerError
#[test]
fn test_categorize_acp_error_api_server_error() {
    // The exact error from the logs
    assert_eq!(
        categorize_acp_error(
            r#"Internal error: API Error: 500 {"type":"error","error":{"type":"api_error","message":"Internal server error"}}"#
        ),
        AcpErrorCategory::ApiServerError
    );

    // Various 5xx status codes
    assert_eq!(
        categorize_acp_error("HTTP 502: Bad Gateway"),
        AcpErrorCategory::ApiServerError
    );
    assert_eq!(
        categorize_acp_error("HTTP 503: Service Unavailable"),
        AcpErrorCategory::ApiServerError
    );
    assert_eq!(
        categorize_acp_error("HTTP 504: Gateway Timeout"),
        AcpErrorCategory::ApiServerError
    );

    // Generic patterns
    assert_eq!(
        categorize_acp_error("internal server error"),
        AcpErrorCategory::ApiServerError
    );
    assert_eq!(
        categorize_acp_error("api_error"),
        AcpErrorCategory::ApiServerError
    );
    assert_eq!(
        categorize_acp_error("server error occurred"),
        AcpErrorCategory::ApiServerError
    );
    assert_eq!(
        categorize_acp_error("API is overloaded"),
        AcpErrorCategory::ApiServerError
    );
}

/// Test that errors containing both auth and server error patterns still categorize as auth
/// (auth check comes first in the chain)
#[test]
fn test_categorize_acp_error_auth_takes_priority_over_server_error() {
    assert_eq!(
        categorize_acp_error("500 authentication service unavailable"),
        AcpErrorCategory::Authentication
    );
}

/// Test that a wrapped 500 error (through anyhow .context()) is still categorized correctly
#[test]
fn test_api_server_error_through_anyhow_context() {
    let error_msg = r#"Internal error: API Error: 500 {"type":"error","error":{"type":"api_error","message":"Internal server error"}}"#;
    let inner = anyhow::anyhow!("{error_msg}");
    let wrapped: anyhow::Error = inner.context("ACP prompt failed");

    let error_string = format!("{wrapped:?}");
    let category = categorize_acp_error(&error_string);

    assert_eq!(
        category,
        AcpErrorCategory::ApiServerError,
        "Debug-formatted error chain should be categorized as ApiServerError"
    );
}

/// Test that enhanced_error_message for ApiServerError suggests retrying
#[test]
fn test_enhanced_error_message_api_server_error() {
    use crate::registry::AgentKind;

    let enhanced = enhanced_error_message(
        AcpErrorCategory::ApiServerError,
        "Internal error: API Error: 500",
        "Claude Code",
        AgentKind::ClaudeCode.auth_hint(),
        AgentKind::ClaudeCode.display_name(),
        AgentKind::ClaudeCode.npm_package(),
    );

    assert!(
        enhanced.contains("try again"),
        "ApiServerError message should suggest retrying, got: {enhanced}"
    );
    assert!(
        enhanced.contains("temporary") || enhanced.contains("server error"),
        "ApiServerError message should mention it's a server error, got: {enhanced}"
    );
}

/// Test that enhanced_error_message produces actionable auth error messages
#[test]
fn test_enhanced_error_message_auth() {
    use crate::registry::AgentKind;

    let auth_hint = AgentKind::ClaudeCode.auth_hint();
    let enhanced = enhanced_error_message(
        AcpErrorCategory::Authentication,
        "Authentication required",
        "Claude Code ACP",
        auth_hint,
        AgentKind::ClaudeCode.display_name(),
        AgentKind::ClaudeCode.npm_package(),
    );

    assert!(
        enhanced.contains("Authentication required"),
        "Should mention auth required, got: {enhanced}"
    );
    assert!(
        enhanced.contains("/login"),
        "Should include auth hint with '/login', got: {enhanced}"
    );
}

/// Test that enhanced_error_message produces actionable quota error messages
#[test]
fn test_enhanced_error_message_quota() {
    use crate::registry::AgentKind;

    let enhanced = enhanced_error_message(
        AcpErrorCategory::QuotaExceeded,
        "Rate limit exceeded",
        "Codex ACP",
        AgentKind::Codex.auth_hint(),
        AgentKind::Codex.display_name(),
        AgentKind::Codex.npm_package(),
    );

    assert!(
        enhanced.contains("Rate limit") || enhanced.contains("quota"),
        "Should mention rate limit or quota, got: {enhanced}"
    );
}

/// Test that enhanced_error_message produces actionable executable not found messages
#[test]
fn test_enhanced_error_message_executable_not_found() {
    use crate::registry::AgentKind;

    let enhanced = enhanced_error_message(
        AcpErrorCategory::ExecutableNotFound,
        "npx: command not found",
        "Gemini ACP",
        AgentKind::Gemini.auth_hint(),
        AgentKind::Gemini.display_name(),
        AgentKind::Gemini.npm_package(),
    );

    assert!(
        enhanced.contains("install") || enhanced.contains("npm"),
        "Should mention installation instructions, got: {enhanced}"
    );
}

/// Test that enhanced_error_message passes through unknown errors
#[test]
fn test_enhanced_error_message_unknown() {
    use crate::registry::AgentKind;

    let original_error = "Some random error";
    let enhanced = enhanced_error_message(
        AcpErrorCategory::Unknown,
        original_error,
        "Mock ACP",
        AgentKind::ClaudeCode.auth_hint(),
        AgentKind::ClaudeCode.display_name(),
        AgentKind::ClaudeCode.npm_package(),
    );

    assert_eq!(
        enhanced, original_error,
        "Unknown errors should pass through unchanged"
    );
}

/// Integration test: Mock agent auth failure produces actionable error message.
///
/// This test uses the real mock-acp-agent binary with MOCK_AGENT_REQUIRE_AUTH=true
/// to simulate an authentication failure and verify the error message is actionable.
#[tokio::test]
#[serial]
async fn test_mock_agent_auth_failure_produces_actionable_error() {
    // Get the mock agent config to check if the binary exists
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");

    // Check if mock agent binary exists
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    // Set the environment variable to trigger auth failure
    // SAFETY: This is a test that manipulates environment variables.
    // It's safe because this test runs in isolation and we clean up after.
    unsafe {
        std::env::set_var("MOCK_AGENT_REQUIRE_AUTH", "true");
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (event_tx, _event_rx) = mpsc::channel(32);

    let config = AcpBackendConfig {
        agent: "mock-model".to_string(),
        cwd: temp_dir.path().to_path_buf(),
        approval_policy: AskForApproval::Never,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        notify: None,
        os_notifications: crate::config::OsNotifications::Disabled,
        nori_home: temp_dir.path().to_path_buf(),
        history_persistence: crate::config::HistoryPersistence::SaveAll,
        cli_version: "test".to_string(),
        notify_after_idle: crate::config::NotifyAfterIdle::FiveSeconds,
        auto_worktree: crate::config::AutoWorktree::Off,
        auto_worktree_repo_root: None,
        session_start_hooks: vec![],
        session_end_hooks: vec![],
        pre_user_prompt_hooks: vec![],
        post_user_prompt_hooks: vec![],
        pre_tool_call_hooks: vec![],
        post_tool_call_hooks: vec![],
        pre_agent_response_hooks: vec![],
        post_agent_response_hooks: vec![],
        async_session_start_hooks: vec![],
        async_session_end_hooks: vec![],
        async_pre_user_prompt_hooks: vec![],
        async_post_user_prompt_hooks: vec![],
        async_pre_tool_call_hooks: vec![],
        async_post_tool_call_hooks: vec![],
        async_pre_agent_response_hooks: vec![],
        async_post_agent_response_hooks: vec![],
        script_timeout: std::time::Duration::from_secs(30),
        default_model: None,
    };

    let result = AcpBackend::spawn(&config, event_tx).await;

    // Clean up env var
    // SAFETY: Cleaning up the environment variable we set above.
    unsafe {
        std::env::remove_var("MOCK_AGENT_REQUIRE_AUTH");
    }

    // Verify spawn failed
    let error_message = match result {
        Ok(_) => {
            panic!("Expected spawn to fail with auth error, but it succeeded");
        }
        Err(e) => e.to_string(),
    };

    // Verify error message is actionable - should mention auth and provide instructions
    // The mock agent returns error code -32000 which should be categorized as auth
    assert!(
        error_message.contains("Authentication")
            || error_message.contains("auth")
            || error_message.contains("login"),
        "Error message should mention authentication or provide login instructions, got: {error_message}"
    );
}

/// Test that updating the approval policy via watch channel dynamically changes
/// the approval handler's behavior. This verifies that `/approvals` command
/// selecting "full access" makes it equivalent to `--yolo`.
#[tokio::test]
async fn test_approval_policy_dynamic_update() {
    use codex_protocol::approvals::ExecApprovalRequestEvent;
    use tokio::sync::oneshot;
    use tokio::sync::watch;

    // Create channels for the test
    let (approval_tx, approval_rx) = mpsc::channel::<ApprovalRequest>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let pending_approvals = Arc::new(Mutex::new(Vec::<ApprovalRequest>::new()));
    let user_notifier = Arc::new(codex_core::UserNotifier::new(None, false));
    let cwd = PathBuf::from("/tmp/test");

    // Create watch channel starting with OnRequest policy (requires approval)
    let (policy_tx, policy_rx) = watch::channel(AskForApproval::OnRequest);

    // Spawn the approval handler with the watch receiver
    tokio::spawn(AcpBackend::run_approval_handler(
        approval_rx,
        event_tx.clone(),
        Arc::clone(&pending_approvals),
        Arc::clone(&user_notifier),
        cwd.clone(),
        policy_rx,
    ));

    // Create a mock approval request
    let (response_tx1, mut response_rx1) = oneshot::channel();
    let request1 = ApprovalRequest {
        event: ApprovalEventType::Exec(ExecApprovalRequestEvent {
            call_id: "call-1".to_string(),
            turn_id: String::new(),
            command: vec!["ls".to_string()],
            cwd: cwd.clone(),
            reason: None,
            risk: None,
            parsed_cmd: vec![],
        }),
        options: vec![],
        response_tx: response_tx1,
    };

    // Send first request - should be forwarded to TUI (not auto-approved)
    approval_tx.send(request1).await.unwrap();

    // Give the handler time to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Should have received an approval request event in the TUI
    let event = event_rx.try_recv();
    assert!(
        event.is_ok(),
        "Should have received approval request event for OnRequest policy"
    );
    if let Ok(Event {
        msg: EventMsg::ExecApprovalRequest(req),
        ..
    }) = event
    {
        assert_eq!(req.call_id, "call-1");
    } else {
        panic!("Expected ExecApprovalRequest event");
    }

    // The request should be pending (not auto-approved)
    assert!(
        response_rx1.try_recv().is_err(),
        "Request should not be auto-approved with OnRequest policy"
    );

    // Now update the policy to Never (yolo mode)
    policy_tx.send(AskForApproval::Never).unwrap();

    // Give the handler time to see the policy change
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Send second request - should be auto-approved
    let (response_tx2, mut response_rx2) = oneshot::channel();
    let request2 = ApprovalRequest {
        event: ApprovalEventType::Exec(ExecApprovalRequestEvent {
            call_id: "call-2".to_string(),
            turn_id: String::new(),
            command: vec!["echo".to_string(), "hello".to_string()],
            cwd: cwd.clone(),
            reason: None,
            risk: None,
            parsed_cmd: vec![],
        }),
        options: vec![],
        response_tx: response_tx2,
    };

    approval_tx.send(request2).await.unwrap();

    // Give the handler time to process
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // Should NOT have received another approval request event (auto-approved)
    let event2 = event_rx.try_recv();
    assert!(
        event2.is_err(),
        "Should NOT receive approval request event when policy is Never (yolo mode)"
    );

    // The request should have been auto-approved
    let decision = response_rx2.try_recv();
    assert!(
        matches!(decision, Ok(ReviewDecision::Approved)),
        "Request should be auto-approved with Never policy, got: {decision:?}"
    );
}

/// Test that Op::Compact sends the summarization prompt to the agent and emits
/// the expected events: TaskStarted, agent message streaming, ContextCompacted,
/// Warning, and TaskComplete.
///
/// This test uses the mock agent to simulate the compact flow.
#[tokio::test]
#[serial]
async fn test_compact_sends_summarization_prompt_and_emits_events() {
    use std::time::Duration;

    // Get the mock agent config to check if the binary exists
    let mock_config =
        crate::registry::get_agent_config("mock-model").expect("mock-model should be registered");

    // Check if mock agent binary exists
    if !std::path::Path::new(&mock_config.command).exists() {
        eprintln!(
            "Skipping test: mock_acp_agent not found at {}",
            mock_config.command
        );
        return;
    }

    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let (event_tx, mut event_rx) = mpsc::channel(64);

    let config = AcpBackendConfig {
        agent: "mock-model".to_string(),
        cwd: temp_dir.path().to_path_buf(),
        approval_policy: AskForApproval::Never,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        notify: None,
        os_notifications: crate::config::OsNotifications::Disabled,
        nori_home: temp_dir.path().to_path_buf(),
        history_persistence: crate::config::HistoryPersistence::SaveAll,
        cli_version: "test".to_string(),
        notify_after_idle: crate::config::NotifyAfterIdle::FiveSeconds,
        auto_worktree: crate::config::AutoWorktree::Off,
        auto_worktree_repo_root: None,
        session_start_hooks: vec![],
        session_end_hooks: vec![],
        pre_user_prompt_hooks: vec![],
        post_user_prompt_hooks: vec![],
        pre_tool_call_hooks: vec![],
        post_tool_call_hooks: vec![],
        pre_agent_response_hooks: vec![],
        post_agent_response_hooks: vec![],
        async_session_start_hooks: vec![],
        async_session_end_hooks: vec![],
        async_pre_user_prompt_hooks: vec![],
        async_post_user_prompt_hooks: vec![],
        async_pre_tool_call_hooks: vec![],
        async_post_tool_call_hooks: vec![],
        async_pre_agent_response_hooks: vec![],
        async_post_agent_response_hooks: vec![],
        script_timeout: std::time::Duration::from_secs(30),
        default_model: None,
    };

    let backend = AcpBackend::spawn(&config, event_tx)
        .await
        .expect("Failed to spawn ACP backend");

    // Drain the SessionConfigured event
    let _ = tokio::time::timeout(Duration::from_secs(2), event_rx.recv())
        .await
        .expect("Should receive SessionConfigured event");

    // Submit the Compact operation
    let _id = backend
        .submit(Op::Compact)
        .await
        .expect("Failed to submit Op::Compact");

    // Collect events with a timeout
    let mut events = Vec::new();
    let timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match tokio::time::timeout(Duration::from_millis(500), event_rx.recv()).await {
            Ok(Some(event)) => {
                events.push(event);
                // Check if we got TaskComplete, which signals the end
                if matches!(
                    events.last().map(|e| &e.msg),
                    Some(EventMsg::TaskComplete(_))
                ) {
                    break;
                }
            }
            Ok(None) => break, // Channel closed
            Err(_) => {
                // Timeout on recv - check if we have enough events
                if events
                    .iter()
                    .any(|e| matches!(e.msg, EventMsg::TaskComplete(_)))
                {
                    break;
                }
            }
        }
    }

    // Verify we got the expected events
    let has_task_started = events
        .iter()
        .any(|e| matches!(e.msg, EventMsg::TaskStarted(_)));
    let has_context_compacted = events
        .iter()
        .any(|e| matches!(e.msg, EventMsg::ContextCompacted(_)));
    let has_warning = events.iter().any(|e| matches!(e.msg, EventMsg::Warning(_)));
    let has_task_complete = events
        .iter()
        .any(|e| matches!(e.msg, EventMsg::TaskComplete(_)));

    assert!(
        has_task_started,
        "Expected TaskStarted event. Events received: {events:?}"
    );
    assert!(
        has_context_compacted,
        "Expected ContextCompacted event. Events received: {events:?}"
    );
    assert!(
        has_warning,
        "Expected Warning event about long conversations. Events received: {events:?}"
    );
    assert!(
        has_task_complete,
        "Expected TaskComplete event. Events received: {events:?}"
    );
}
