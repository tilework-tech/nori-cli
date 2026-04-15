use super::*;

async fn send_seed_tool_call(
    persistent_tx: &mpsc::Sender<agent_client_protocol_schema::SessionUpdate>,
    call_id: &str,
) {
    persistent_tx
        .send(agent_client_protocol_schema::SessionUpdate::ToolCall(
            agent_client_protocol_schema::ToolCall::new(call_id.to_string(), "Terminal"),
        ))
        .await
        .expect("send seed tool call");
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
        initial_context: None,
        session_context: None,
        mcp_servers: std::collections::HashMap::new(),
        mcp_oauth_credentials_store_mode: codex_rmcp_client::OAuthCredentialsStoreMode::default(),
    };

    let result = spawn_test_backend(&config, event_tx, None).await;

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
#[serial]
async fn test_approval_policy_dynamic_update() {
    use agent_client_protocol_schema as acp;
    use codex_protocol::approvals::ExecApprovalRequestEvent;
    use tokio::sync::oneshot;
    use tokio::sync::watch;

    // Create channels for the test
    let (approval_tx, approval_rx) = mpsc::channel::<ApprovalRequest>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);
    let pending_approvals = Arc::new(Mutex::new(Vec::<PendingApprovalRequest>::new()));
    let user_notifier = Arc::new(codex_core::UserNotifier::new(None, false));
    let cwd = PathBuf::from("/tmp/test");

    // Create watch channel starting with OnRequest policy (requires approval)
    let (policy_tx, policy_rx) = watch::channel(AskForApproval::OnRequest);

    // Spawn the approval handler with the watch receiver
    spawn_test_approval_handler(
        approval_rx,
        event_tx.clone(),
        Some(client_event_tx),
        Arc::clone(&pending_approvals),
        Arc::clone(&user_notifier),
        policy_rx,
    );

    // Create a mock approval request
    let (response_tx1, mut response_rx1) = oneshot::channel();
    let request1 = ApprovalRequest {
        request_id: "perm-1".to_string(),
        event: ApprovalEventType::Exec(ExecApprovalRequestEvent {
            call_id: "call-1".to_string(),
            turn_id: String::new(),
            command: vec!["ls".to_string()],
            cwd: cwd.clone(),
            reason: None,
            risk: None,
            parsed_cmd: vec![],
        }),
        acp_request: acp::RequestPermissionRequest::new(
            "session-1",
            acp::ToolCallUpdate::new("call-1", acp::ToolCallUpdateFields::new()),
            vec![],
        ),
        options: vec![],
        response_tx: response_tx1,
    };

    // Send first request - should be forwarded to TUI (not auto-approved)
    approval_tx.send(request1).await.unwrap();

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("approval request timeout")
            .expect("approval request missing");
    if let nori_protocol::ClientEvent::ApprovalRequest(req) = client_event {
        assert_eq!(req.call_id, "call-1");
    } else {
        panic!("Expected normalized ApprovalRequest event");
    }

    // The request should be pending (not auto-approved)
    assert!(
        response_rx1.try_recv().is_err(),
        "Request should not be auto-approved with OnRequest policy"
    );
    if let Ok(event) = event_rx.try_recv() {
        assert!(
            !matches!(
                event.msg,
                EventMsg::ExecApprovalRequest(_) | EventMsg::ApplyPatchApprovalRequest(_)
            ),
            "OnRequest policy should not emit legacy approval events",
        );
    }

    // Now update the policy to Never (yolo mode)
    policy_tx.send(AskForApproval::Never).unwrap();

    // Send second request - should be auto-approved
    let (response_tx2, mut response_rx2) = oneshot::channel();
    let request2 = ApprovalRequest {
        request_id: "perm-2".to_string(),
        event: ApprovalEventType::Exec(ExecApprovalRequestEvent {
            call_id: "call-2".to_string(),
            turn_id: String::new(),
            command: vec!["echo".to_string(), "hello".to_string()],
            cwd: cwd.clone(),
            reason: None,
            risk: None,
            parsed_cmd: vec![],
        }),
        acp_request: acp::RequestPermissionRequest::new(
            "session-1",
            acp::ToolCallUpdate::new("call-2", acp::ToolCallUpdateFields::new()),
            vec![],
        ),
        options: vec![],
        response_tx: response_tx2,
    };

    approval_tx.send(request2).await.unwrap();

    // The request should have been auto-approved
    let decision = tokio::time::timeout(std::time::Duration::from_secs(1), &mut response_rx2)
        .await
        .expect("auto-approval timeout")
        .expect("auto-approval response channel closed");
    assert!(
        matches!(decision, ReviewDecision::Approved),
        "Request should be auto-approved with Never policy, got: {decision:?}"
    );

    let client_event = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        client_event_rx.recv(),
    )
    .await;
    assert!(
        client_event.is_err(),
        "Should NOT receive approval request event when policy is Never (yolo mode)"
    );
}

#[tokio::test]
#[serial]
async fn test_patch_approval_emits_normalized_client_event() {
    use agent_client_protocol_schema as acp;
    use tokio::sync::oneshot;
    use tokio::sync::watch;

    let (approval_tx, approval_rx) = mpsc::channel::<ApprovalRequest>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);
    let pending_approvals = Arc::new(Mutex::new(Vec::<PendingApprovalRequest>::new()));
    let user_notifier = Arc::new(codex_core::UserNotifier::new(None, false));
    let (_policy_tx, policy_rx) = watch::channel(AskForApproval::OnRequest);

    spawn_test_approval_handler(
        approval_rx,
        event_tx,
        Some(client_event_tx),
        Arc::clone(&pending_approvals),
        Arc::clone(&user_notifier),
        policy_rx,
    );

    let mut changes = HashMap::new();
    changes.insert(
        PathBuf::from("README.md"),
        codex_protocol::protocol::FileChange::Add {
            content: "hello\nworld\n".into(),
        },
    );
    let tool_call = acp::ToolCallUpdate::new(
        "call-patch",
        acp::ToolCallUpdateFields::new()
            .title("Write README.md")
            .kind(acp::ToolKind::Edit)
            .content(vec![acp::ToolCallContent::Diff(acp::Diff::new(
                PathBuf::from("README.md"),
                "hello\nworld\n",
            ))]),
    );
    let (response_tx, _response_rx) = oneshot::channel();
    approval_tx
        .send(ApprovalRequest {
            request_id: "perm-patch".to_string(),
            event: ApprovalEventType::Patch(
                codex_protocol::approvals::ApplyPatchApprovalRequestEvent {
                    call_id: "call-patch".into(),
                    turn_id: String::new(),
                    changes,
                    reason: Some("Write README.md (2 lines)".into()),
                    grant_root: None,
                },
            ),
            acp_request: acp::RequestPermissionRequest::new("session-1", tool_call, vec![]),
            options: vec![],
            response_tx,
        })
        .await
        .expect("send approval request");

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("client event timeout")
            .expect("client event missing");
    let nori_protocol::ClientEvent::ApprovalRequest(req) = client_event else {
        panic!("expected normalized approval request");
    };
    assert_eq!(req.call_id, "call-patch");
    assert_eq!(req.title, "Write README.md");
    assert_eq!(req.kind, nori_protocol::ToolKind::Edit);
    let nori_protocol::ApprovalSubject::ToolSnapshot(snapshot) = req.subject;
    assert_eq!(snapshot.call_id, "call-patch");
    assert_eq!(snapshot.title, "Write README.md");
    assert_eq!(snapshot.kind, nori_protocol::ToolKind::Edit);
    assert_eq!(snapshot.phase, nori_protocol::ToolPhase::PendingApproval);
    assert_eq!(
        snapshot.invocation,
        Some(nori_protocol::Invocation::FileChanges {
            changes: vec![nori_protocol::FileChange {
                path: PathBuf::from("README.md"),
                old_text: None,
                new_text: "hello\nworld\n".into(),
            }],
        })
    );
    assert_eq!(
        snapshot.artifacts,
        vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
            path: PathBuf::from("README.md"),
            old_text: None,
            new_text: "hello\nworld\n".into(),
        })]
    );
    assert!(snapshot.owner_request_id.is_some());

    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
    if let Ok(Some(event)) = event {
        assert!(
            !matches!(
                event.msg,
                EventMsg::ExecApprovalRequest(_) | EventMsg::ApplyPatchApprovalRequest(_)
            ),
            "edit approvals should not be emitted on the legacy event channel once the client-event path exists",
        );
    }
}

#[tokio::test]
#[serial]
async fn test_exec_approval_emits_normalized_client_event() {
    use agent_client_protocol_schema as acp;
    use tokio::sync::oneshot;
    use tokio::sync::watch;

    let (approval_tx, approval_rx) = mpsc::channel::<ApprovalRequest>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);
    let pending_approvals = Arc::new(Mutex::new(Vec::<PendingApprovalRequest>::new()));
    let user_notifier = Arc::new(codex_core::UserNotifier::new(None, false));
    let (_policy_tx, policy_rx) = watch::channel(AskForApproval::OnRequest);

    spawn_test_approval_handler(
        approval_rx,
        event_tx,
        Some(client_event_tx),
        Arc::clone(&pending_approvals),
        Arc::clone(&user_notifier),
        policy_rx,
    );

    let tool_call = acp::ToolCallUpdate::new(
        "call-exec-approve",
        acp::ToolCallUpdateFields::new()
            .title("Terminal")
            .kind(acp::ToolKind::Execute)
            .raw_input(serde_json::json!({"command": "git status"})),
    );
    let (response_tx, _response_rx) = oneshot::channel();
    approval_tx
        .send(ApprovalRequest {
            request_id: "perm-exec".to_string(),
            event: ApprovalEventType::Exec(codex_protocol::approvals::ExecApprovalRequestEvent {
                call_id: "call-exec-approve".into(),
                turn_id: String::new(),
                command: vec!["bash".into(), "-lc".into(), "git status".into()],
                cwd: PathBuf::from("/tmp/test"),
                reason: Some("Execute: git status".into()),
                risk: None,
                parsed_cmd: vec![],
            }),
            acp_request: acp::RequestPermissionRequest::new("session-1", tool_call, vec![]),
            options: vec![],
            response_tx,
        })
        .await
        .expect("send approval request");

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("client event timeout")
            .expect("client event missing");
    let nori_protocol::ClientEvent::ApprovalRequest(req) = client_event else {
        panic!("expected normalized approval request");
    };
    assert_eq!(req.call_id, "call-exec-approve");
    assert_eq!(req.title, "Terminal");
    assert_eq!(req.kind, nori_protocol::ToolKind::Execute);
    let nori_protocol::ApprovalSubject::ToolSnapshot(snapshot) = req.subject;
    assert_eq!(snapshot.call_id, "call-exec-approve");
    assert_eq!(snapshot.title, "Terminal");
    assert_eq!(snapshot.kind, nori_protocol::ToolKind::Execute);
    assert_eq!(snapshot.phase, nori_protocol::ToolPhase::PendingApproval);
    assert_eq!(
        snapshot.invocation,
        Some(nori_protocol::Invocation::Command {
            command: "git status".into(),
        })
    );
    assert_eq!(
        snapshot.raw_input,
        Some(serde_json::json!({"command": "git status"}))
    );
    assert!(snapshot.owner_request_id.is_some());

    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
    if let Ok(Some(event)) = event {
        assert!(
            !matches!(
                event.msg,
                EventMsg::ExecApprovalRequest(_) | EventMsg::ApplyPatchApprovalRequest(_)
            ),
            "execute approvals should not be emitted on the legacy event channel once the client-event path exists",
        );
    }
}

#[tokio::test]
#[serial]
async fn test_exec_approval_with_never_policy_does_not_emit_normalized_client_event() {
    use agent_client_protocol_schema as acp;
    use pretty_assertions::assert_eq;
    use tokio::sync::oneshot;
    use tokio::sync::watch;

    let (approval_tx, approval_rx) = mpsc::channel::<ApprovalRequest>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);
    let pending_approvals = Arc::new(Mutex::new(Vec::<PendingApprovalRequest>::new()));
    let user_notifier = Arc::new(codex_core::UserNotifier::new(None, false));
    let (_policy_tx, policy_rx) = watch::channel(AskForApproval::Never);

    spawn_test_approval_handler(
        approval_rx,
        event_tx,
        Some(client_event_tx),
        Arc::clone(&pending_approvals),
        Arc::clone(&user_notifier),
        policy_rx,
    );

    let tool_call = acp::ToolCallUpdate::new(
        "call-auto-approved",
        acp::ToolCallUpdateFields::new()
            .title("Terminal")
            .kind(acp::ToolKind::Execute)
            .raw_input(serde_json::json!({"command": "git status"})),
    );
    let (response_tx, response_rx) = oneshot::channel();
    approval_tx
        .send(ApprovalRequest {
            request_id: "perm-auto-approved".to_string(),
            event: ApprovalEventType::Exec(codex_protocol::approvals::ExecApprovalRequestEvent {
                call_id: "call-auto-approved".into(),
                turn_id: String::new(),
                command: vec!["bash".into(), "-lc".into(), "git status".into()],
                cwd: PathBuf::from("/tmp/test"),
                reason: Some("Execute: git status".into()),
                risk: None,
                parsed_cmd: vec![],
            }),
            acp_request: acp::RequestPermissionRequest::new("session-1", tool_call, vec![]),
            options: vec![],
            response_tx,
        })
        .await
        .expect("send approval request");

    let decision = tokio::time::timeout(std::time::Duration::from_secs(1), response_rx)
        .await
        .expect("approval response timeout")
        .expect("approval response missing");
    assert_eq!(decision, ReviewDecision::Approved);

    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
    if let Ok(Some(event)) = event {
        assert!(
            !matches!(
                event.msg,
                EventMsg::ExecApprovalRequest(_) | EventMsg::ApplyPatchApprovalRequest(_)
            ),
            "auto-approved execute approvals should not be emitted on the legacy event channel",
        );
    }

    let client_event = tokio::time::timeout(
        std::time::Duration::from_millis(100),
        client_event_rx.recv(),
    )
    .await;
    assert!(
        client_event.is_err(),
        "auto-approved execute approvals should not emit normalized approval events",
    );
}

#[tokio::test]
#[serial]
async fn test_completed_edit_update_emits_normalized_tool_snapshot() {
    use agent_client_protocol_schema as acp;
    use pretty_assertions::assert_eq;

    let (persistent_tx, persistent_rx) = mpsc::channel::<acp::SessionUpdate>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);

    spawn_test_persistent_relay(persistent_rx, event_tx, Some(client_event_tx));
    send_seed_tool_call(&persistent_tx, "call-edit-complete").await;

    persistent_tx
        .send(acp::SessionUpdate::ToolCallUpdate(
            acp::ToolCallUpdate::new(
                "call-edit-complete",
                acp::ToolCallUpdateFields::new()
                    .title("Write README.md")
                    .kind(acp::ToolKind::Edit)
                    .status(acp::ToolCallStatus::Completed)
                    .content(vec![acp::Diff::new("README.md", "hello\nworld\n").into()]),
            ),
        ))
        .await
        .expect("send tool call update");

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("client event timeout")
            .expect("client event missing");
    let nori_protocol::ClientEvent::ToolSnapshot(snapshot) = client_event else {
        panic!("expected tool snapshot");
    };
    assert!(snapshot.owner_request_id.is_some());
    assert_eq!(
        snapshot,
        nori_protocol::ToolSnapshot {
            call_id: "call-edit-complete".into(),
            title: "Write README.md".into(),
            kind: nori_protocol::ToolKind::Edit,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::FileChanges {
                changes: vec![nori_protocol::FileChange {
                    path: PathBuf::from("README.md"),
                    old_text: None,
                    new_text: "hello\nworld\n".into(),
                }],
            }),
            artifacts: vec![nori_protocol::Artifact::Diff(nori_protocol::FileChange {
                path: PathBuf::from("README.md"),
                old_text: None,
                new_text: "hello\nworld\n".into(),
            })],
            raw_input: None,
            raw_output: None,
            owner_request_id: snapshot.owner_request_id.clone(),
        }
    );

    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
    assert!(
        event.is_err(),
        "completed edit snapshots should not emit the legacy patch event once the client-event path exists",
    );
}

#[tokio::test]
#[serial]
async fn test_unknown_tool_call_update_still_emits_normalized_tool_snapshot() {
    use agent_client_protocol_schema as acp;
    use pretty_assertions::assert_eq;

    let (persistent_tx, persistent_rx) = mpsc::channel::<acp::SessionUpdate>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);

    spawn_test_persistent_relay(persistent_rx, event_tx, Some(client_event_tx));

    persistent_tx
        .send(acp::SessionUpdate::ToolCallUpdate(
            acp::ToolCallUpdate::new(
                "call-exec-orphan",
                acp::ToolCallUpdateFields::new()
                    .title("Terminal")
                    .kind(acp::ToolKind::Execute)
                    .status(acp::ToolCallStatus::Completed)
                    .raw_input(serde_json::json!({
                        "command": "git status",
                    }))
                    .raw_output(serde_json::json!({
                        "stdout": "On branch spec\n",
                    })),
            ),
        ))
        .await
        .expect("send tool call update");

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("client event timeout")
            .expect("client event missing");
    let nori_protocol::ClientEvent::ToolSnapshot(snapshot) = client_event else {
        panic!("expected tool snapshot");
    };
    assert!(snapshot.owner_request_id.is_some());
    assert_eq!(
        snapshot,
        nori_protocol::ToolSnapshot {
            call_id: "call-exec-orphan".into(),
            title: "Terminal".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "git status".into(),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "On branch spec\n".into(),
            }],
            raw_input: Some(serde_json::json!({
                "command": "git status",
            })),
            raw_output: Some(serde_json::json!({
                "stdout": "On branch spec\n",
            })),
            owner_request_id: snapshot.owner_request_id.clone(),
        }
    );

    let warnings =
        tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
    assert!(
        warnings.is_err(),
        "unknown tool updates during an active request should stay visible without falling back to control-plane warnings",
    );
}

#[tokio::test]
#[serial]
async fn test_out_of_phase_tool_call_update_still_emits_normalized_tool_snapshot() {
    use agent_client_protocol_schema as acp;
    use pretty_assertions::assert_eq;

    let (persistent_tx, persistent_rx) = mpsc::channel::<acp::SessionUpdate>(16);
    let (event_tx, _event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);

    spawn_test_idle_persistent_relay(persistent_rx, event_tx, Some(client_event_tx));

    persistent_tx
        .send(acp::SessionUpdate::ToolCallUpdate(
            acp::ToolCallUpdate::new(
                "call-read-orphan",
                acp::ToolCallUpdateFields::new()
                    .title("Read Cargo.toml")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Completed)
                    .raw_input(serde_json::json!({
                        "path": "Cargo.toml",
                    })),
            ),
        ))
        .await
        .expect("send tool call update");

    let warning_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("warning timeout")
            .expect("warning missing");
    let nori_protocol::ClientEvent::Warning(warning) = warning_event else {
        panic!("expected warning");
    };
    assert!(
        warning
            .message
            .contains("Received request-owned content update while no request is active"),
        "unexpected warning: {warning:?}"
    );

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("client event timeout")
            .expect("client event missing");
    let nori_protocol::ClientEvent::ToolSnapshot(snapshot) = client_event else {
        panic!("expected tool snapshot");
    };
    assert_eq!(
        snapshot,
        nori_protocol::ToolSnapshot {
            call_id: "call-read-orphan".into(),
            title: "Read Cargo.toml".into(),
            kind: nori_protocol::ToolKind::Read,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Read {
                path: PathBuf::from("Cargo.toml"),
            }),
            artifacts: vec![],
            raw_input: Some(serde_json::json!({
                "path": "Cargo.toml",
            })),
            raw_output: None,
            owner_request_id: None,
        }
    );
}

#[tokio::test]
#[serial]
async fn test_completed_delete_update_emits_normalized_tool_snapshot() {
    use agent_client_protocol_schema as acp;
    use pretty_assertions::assert_eq;

    let (persistent_tx, persistent_rx) = mpsc::channel::<acp::SessionUpdate>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);

    spawn_test_persistent_relay(persistent_rx, event_tx, Some(client_event_tx));
    send_seed_tool_call(&persistent_tx, "call-delete-complete").await;

    persistent_tx
        .send(acp::SessionUpdate::ToolCallUpdate(
            acp::ToolCallUpdate::new(
                "call-delete-complete",
                acp::ToolCallUpdateFields::new()
                    .title("Delete README.md")
                    .kind(acp::ToolKind::Delete)
                    .status(acp::ToolCallStatus::Completed)
                    .raw_input(serde_json::json!({
                        "path": "README.md",
                        "content": "before\n",
                    })),
            ),
        ))
        .await
        .expect("send tool call update");

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("client event timeout")
            .expect("client event missing");
    let nori_protocol::ClientEvent::ToolSnapshot(snapshot) = client_event else {
        panic!("expected tool snapshot");
    };
    assert!(snapshot.owner_request_id.is_some());
    assert_eq!(
        snapshot,
        nori_protocol::ToolSnapshot {
            call_id: "call-delete-complete".into(),
            title: "Delete README.md".into(),
            kind: nori_protocol::ToolKind::Delete,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::FileOperations {
                operations: vec![nori_protocol::FileOperation::Delete {
                    path: PathBuf::from("README.md"),
                    old_text: Some("before\n".into()),
                }],
            }),
            artifacts: vec![],
            raw_input: Some(serde_json::json!({
                "path": "README.md",
                "content": "before\n",
            })),
            raw_output: None,
            owner_request_id: snapshot.owner_request_id.clone(),
        }
    );

    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
    assert!(
        event.is_err(),
        "delete tool snapshots should not be emitted on the legacy event channel once the client-event path exists",
    );
}

#[tokio::test]
#[serial]
async fn test_completed_fetch_update_emits_normalized_tool_snapshot() {
    use agent_client_protocol_schema as acp;
    use pretty_assertions::assert_eq;

    let (persistent_tx, persistent_rx) = mpsc::channel::<acp::SessionUpdate>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);

    spawn_test_persistent_relay(persistent_rx, event_tx, Some(client_event_tx));
    send_seed_tool_call(&persistent_tx, "call-fetch-complete").await;

    persistent_tx
        .send(acp::SessionUpdate::ToolCallUpdate(
            acp::ToolCallUpdate::new(
                "call-fetch-complete",
                acp::ToolCallUpdateFields::new()
                    .title("Fetch")
                    .kind(acp::ToolKind::Fetch)
                    .status(acp::ToolCallStatus::Completed)
                    .raw_input(serde_json::json!({
                        "url": "https://example.com",
                    }))
                    .raw_output(serde_json::json!({
                        "stdout": "ok\n",
                    })),
            ),
        ))
        .await
        .expect("send tool call update");

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("client event timeout")
            .expect("client event missing");
    let nori_protocol::ClientEvent::ToolSnapshot(snapshot) = client_event else {
        panic!("expected tool snapshot");
    };
    assert!(snapshot.owner_request_id.is_some());
    assert_eq!(
        snapshot,
        nori_protocol::ToolSnapshot {
            call_id: "call-fetch-complete".into(),
            title: "Fetch".into(),
            kind: nori_protocol::ToolKind::Fetch,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Tool {
                tool_name: "Fetch".into(),
                input: Some(serde_json::json!({
                    "url": "https://example.com",
                })),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "ok\n".into()
            }],
            raw_input: Some(serde_json::json!({
                "url": "https://example.com",
            })),
            raw_output: Some(serde_json::json!({
                "stdout": "ok\n",
            })),
            owner_request_id: snapshot.owner_request_id.clone(),
        }
    );

    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
    assert!(
        event.is_err(),
        "fetch tool snapshots should not be emitted on the legacy event channel once the client-event path exists",
    );
}

#[tokio::test]
#[serial]
async fn test_completed_execute_update_emits_normalized_tool_snapshot() {
    use agent_client_protocol_schema as acp;
    use pretty_assertions::assert_eq;

    let (persistent_tx, persistent_rx) = mpsc::channel::<acp::SessionUpdate>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);

    spawn_test_persistent_relay(persistent_rx, event_tx, Some(client_event_tx));
    send_seed_tool_call(&persistent_tx, "call-exec-complete").await;

    persistent_tx
        .send(acp::SessionUpdate::ToolCallUpdate(
            acp::ToolCallUpdate::new(
                "call-exec-complete",
                acp::ToolCallUpdateFields::new()
                    .title("Terminal")
                    .kind(acp::ToolKind::Execute)
                    .status(acp::ToolCallStatus::Completed)
                    .raw_input(serde_json::json!({"command": "git status"}))
                    .raw_output(serde_json::json!({"stdout": "On branch main\n"})),
            ),
        ))
        .await
        .expect("send tool call update");

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("client event timeout")
            .expect("client event missing");
    let nori_protocol::ClientEvent::ToolSnapshot(snapshot) = client_event else {
        panic!("expected tool snapshot");
    };
    assert!(snapshot.owner_request_id.is_some());
    assert_eq!(
        snapshot,
        nori_protocol::ToolSnapshot {
            call_id: "call-exec-complete".into(),
            title: "Terminal".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "git status".into(),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "On branch main\n".into(),
            }],
            raw_input: Some(serde_json::json!({"command": "git status"})),
            raw_output: Some(serde_json::json!({"stdout": "On branch main\n"})),
            owner_request_id: snapshot.owner_request_id.clone(),
        }
    );

    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
    assert!(
        event.is_err(),
        "completed execute snapshots should not emit the legacy exec end event once the client-event path exists",
    );
}

#[tokio::test]
#[serial]
async fn test_agent_message_chunk_emits_normalized_message_delta() {
    use agent_client_protocol_schema as acp;
    use pretty_assertions::assert_eq;

    let (persistent_tx, persistent_rx) = mpsc::channel::<acp::SessionUpdate>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);

    spawn_test_persistent_relay(persistent_rx, event_tx, Some(client_event_tx));

    persistent_tx
        .send(acp::SessionUpdate::AgentMessageChunk(
            acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
                "hello from the agent",
            ))),
        ))
        .await
        .expect("send message chunk");

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("client event timeout")
            .expect("client event missing");
    assert_eq!(
        client_event,
        nori_protocol::ClientEvent::MessageDelta(nori_protocol::MessageDelta {
            stream: nori_protocol::MessageStream::Answer,
            delta: "hello from the agent".into(),
        })
    );

    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
    assert!(
        event.is_err(),
        "message chunks should not emit legacy agent message deltas once the client-event path exists",
    );
}

#[tokio::test]
#[serial]
async fn test_plan_update_emits_normalized_plan_snapshot() {
    use agent_client_protocol_schema as acp;
    use pretty_assertions::assert_eq;

    let (persistent_tx, persistent_rx) = mpsc::channel::<acp::SessionUpdate>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);

    spawn_test_persistent_relay(persistent_rx, event_tx, Some(client_event_tx));

    persistent_tx
        .send(acp::SessionUpdate::Plan(acp::Plan::new(vec![
            acp::PlanEntry::new(
                "Research current flow",
                acp::PlanEntryPriority::High,
                acp::PlanEntryStatus::Completed,
            ),
            acp::PlanEntry::new(
                "Wire client events",
                acp::PlanEntryPriority::Medium,
                acp::PlanEntryStatus::InProgress,
            ),
        ])))
        .await
        .expect("send plan update");

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("client event timeout")
            .expect("client event missing");
    assert_eq!(
        client_event,
        nori_protocol::ClientEvent::PlanSnapshot(nori_protocol::PlanSnapshot {
            entries: vec![
                nori_protocol::PlanEntry {
                    step: "Research current flow".into(),
                    status: nori_protocol::PlanStatus::Completed,
                },
                nori_protocol::PlanEntry {
                    step: "Wire client events".into(),
                    status: nori_protocol::PlanStatus::InProgress,
                },
            ],
        })
    );

    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
    assert!(
        event.is_err(),
        "plan updates should not emit legacy plan events once the client-event path exists",
    );
}

#[tokio::test]
#[serial]
async fn test_completed_exploring_updates_emit_normalized_tool_snapshots() {
    use agent_client_protocol_schema as acp;
    use pretty_assertions::assert_eq;

    let cases = vec![
        (
            acp::ToolCallUpdate::new(
                "call-read-complete",
                acp::ToolCallUpdateFields::new()
                    .title("Read Cargo.toml")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Completed)
                    .raw_input(serde_json::json!({"path": "Cargo.toml"}))
                    .raw_output(serde_json::json!({"stdout": "[package]\nname = \"nori\"\n"})),
            ),
            nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
                call_id: "call-read-complete".into(),
                title: "Read Cargo.toml".into(),
                kind: nori_protocol::ToolKind::Read,
                phase: nori_protocol::ToolPhase::Completed,
                locations: vec![],
                invocation: Some(nori_protocol::Invocation::Read {
                    path: PathBuf::from("Cargo.toml"),
                }),
                artifacts: vec![nori_protocol::Artifact::Text {
                    text: "[package]\nname = \"nori\"\n".into(),
                }],
                raw_input: Some(serde_json::json!({"path": "Cargo.toml"})),
                raw_output: Some(serde_json::json!({"stdout": "[package]\nname = \"nori\"\n"})),
                owner_request_id: None,
            }),
        ),
        (
            acp::ToolCallUpdate::new(
                "call-search-complete",
                acp::ToolCallUpdateFields::new()
                    .title("Search src")
                    .kind(acp::ToolKind::Search)
                    .status(acp::ToolCallStatus::Completed)
                    .raw_input(serde_json::json!({"pattern": "TODO", "path": "src"}))
                    .raw_output(serde_json::json!({"stdout": "src/main.rs:12:// TODO\n"})),
            ),
            nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
                call_id: "call-search-complete".into(),
                title: "Search src".into(),
                kind: nori_protocol::ToolKind::Search,
                phase: nori_protocol::ToolPhase::Completed,
                locations: vec![],
                invocation: Some(nori_protocol::Invocation::Search {
                    query: Some("TODO".into()),
                    path: Some(PathBuf::from("src")),
                }),
                artifacts: vec![nori_protocol::Artifact::Text {
                    text: "src/main.rs:12:// TODO\n".into(),
                }],
                raw_input: Some(serde_json::json!({"pattern": "TODO", "path": "src"})),
                raw_output: Some(serde_json::json!({"stdout": "src/main.rs:12:// TODO\n"})),
                owner_request_id: None,
            }),
        ),
        (
            acp::ToolCallUpdate::new(
                "call-list-complete",
                acp::ToolCallUpdateFields::new()
                    .title("List src")
                    .kind(acp::ToolKind::Search)
                    .status(acp::ToolCallStatus::Completed)
                    .raw_input(serde_json::json!({"path": "src"}))
                    .raw_output(serde_json::json!({"stdout": "src/main.rs\nsrc/lib.rs\n"})),
            ),
            nori_protocol::ClientEvent::ToolSnapshot(nori_protocol::ToolSnapshot {
                call_id: "call-list-complete".into(),
                title: "List src".into(),
                kind: nori_protocol::ToolKind::Search,
                phase: nori_protocol::ToolPhase::Completed,
                locations: vec![],
                invocation: Some(nori_protocol::Invocation::ListFiles {
                    path: Some(PathBuf::from("src")),
                }),
                artifacts: vec![nori_protocol::Artifact::Text {
                    text: "src/main.rs\nsrc/lib.rs\n".into(),
                }],
                raw_input: Some(serde_json::json!({"path": "src"})),
                raw_output: Some(serde_json::json!({"stdout": "src/main.rs\nsrc/lib.rs\n"})),
                owner_request_id: None,
            }),
        ),
    ];

    for (update, expected_client_event) in cases {
        let (persistent_tx, persistent_rx) = mpsc::channel::<acp::SessionUpdate>(16);
        let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
        let (client_event_tx, mut client_event_rx) =
            mpsc::channel::<nori_protocol::ClientEvent>(16);

        spawn_test_persistent_relay(persistent_rx, event_tx, Some(client_event_tx));
        send_seed_tool_call(&persistent_tx, &update.tool_call_id.to_string()).await;

        persistent_tx
            .send(acp::SessionUpdate::ToolCallUpdate(update))
            .await
            .expect("send tool call update");

        let client_event =
            tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
                .await
                .expect("client event timeout")
                .expect("client event missing");
        let nori_protocol::ClientEvent::ToolSnapshot(snapshot) = client_event else {
            panic!("expected tool snapshot");
        };
        let nori_protocol::ClientEvent::ToolSnapshot(expected_snapshot) = expected_client_event
        else {
            panic!("expected tool snapshot");
        };
        assert!(snapshot.owner_request_id.is_some());
        assert_eq!(
            snapshot,
            nori_protocol::ToolSnapshot {
                owner_request_id: snapshot.owner_request_id.clone(),
                ..expected_snapshot
            }
        );

        let event =
            tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
        assert!(
            event.is_err(),
            "completed normalized exploring snapshots should not emit legacy exec events once the client-event path exists",
        );
    }
}

#[tokio::test]
#[serial]
async fn test_completed_generic_execute_update_emits_normalized_tool_snapshot() {
    use agent_client_protocol_schema as acp;
    use pretty_assertions::assert_eq;

    let (persistent_tx, persistent_rx) = mpsc::channel::<acp::SessionUpdate>(16);
    let (event_tx, mut event_rx) = mpsc::channel::<Event>(16);
    let (client_event_tx, mut client_event_rx) = mpsc::channel::<nori_protocol::ClientEvent>(16);

    spawn_test_persistent_relay(persistent_rx, event_tx, Some(client_event_tx));

    persistent_tx
        .send(acp::SessionUpdate::ToolCall(
            acp::ToolCall::new("toolu_generic_test_001", "Terminal")
                .kind(acp::ToolKind::Execute)
                .status(acp::ToolCallStatus::Pending),
        ))
        .await
        .expect("send generic tool call");

    persistent_tx
        .send(acp::SessionUpdate::ToolCallUpdate(
            acp::ToolCallUpdate::new(
                "toolu_generic_test_001",
                acp::ToolCallUpdateFields::new()
                    .status(acp::ToolCallStatus::Completed)
                    .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                        acp::ContentBlock::Text(acp::TextContent::new("command output here")),
                    ))])
                    .raw_output(
                        serde_json::json!({"exit_code": 0, "stdout": "command output here"}),
                    ),
            ),
        ))
        .await
        .expect("send generic tool call update");

    let client_event =
        tokio::time::timeout(std::time::Duration::from_secs(1), client_event_rx.recv())
            .await
            .expect("client event timeout")
            .expect("client event missing");
    let nori_protocol::ClientEvent::ToolSnapshot(snapshot) = client_event else {
        panic!("expected tool snapshot");
    };
    assert!(snapshot.owner_request_id.is_some());
    assert_eq!(
        snapshot,
        nori_protocol::ToolSnapshot {
            call_id: "toolu_generic_test_001".into(),
            title: "Terminal".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: None,
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "command output here".into(),
            }],
            raw_input: None,
            raw_output: Some(serde_json::json!({"exit_code": 0, "stdout": "command output here"}),),
            owner_request_id: snapshot.owner_request_id.clone(),
        }
    );

    let event = tokio::time::timeout(std::time::Duration::from_millis(100), event_rx.recv()).await;
    assert!(
        event.is_err(),
        "completed generic execute snapshots should not emit the legacy exec end event once the client-event path exists",
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
    let (client_event_tx, mut client_event_rx) = mpsc::channel(64);

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
        initial_context: None,
        session_context: None,
        mcp_servers: std::collections::HashMap::new(),
        mcp_oauth_credentials_store_mode: codex_rmcp_client::OAuthCredentialsStoreMode::default(),
    };

    let backend = spawn_test_backend(&config, event_tx, Some(client_event_tx))
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

    // Collect client events with a timeout
    let mut client_events = Vec::new();
    let mut warning_events = Vec::new();
    let timeout = Duration::from_secs(10);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        tokio::select! {
            client_event = client_event_rx.recv() => {
                if let Some(client_event) = client_event {
                    let done = matches!(
                        client_event,
                        nori_protocol::ClientEvent::PromptCompleted(_)
                    );
                    client_events.push(client_event);
                    if done {
                        break;
                    }
                }
            }
            event = event_rx.recv() => {
                if let Some(event) = event {
                    warning_events.push(event);
                }
            }
        }
    }

    while let Ok(Some(event)) =
        tokio::time::timeout(Duration::from_millis(100), event_rx.recv()).await
    {
        warning_events.push(event);
    }

    let has_task_started = client_events.iter().any(|e| {
        matches!(
            e,
            nori_protocol::ClientEvent::SessionPhaseChanged(
                nori_protocol::session_runtime::SessionPhaseView::Prompt
            )
        )
    });
    let has_context_compacted = client_events
        .iter()
        .any(|e| matches!(e, nori_protocol::ClientEvent::ContextCompacted(_)));
    let has_warning = warning_events
        .iter()
        .any(|e| matches!(e.msg, EventMsg::Warning(_)));
    let has_task_complete = client_events
        .iter()
        .any(|e| matches!(e, nori_protocol::ClientEvent::PromptCompleted(_)));

    assert!(
        has_task_started,
        "Expected normalized turn start event. Client events: {client_events:?}"
    );
    assert!(
        has_context_compacted,
        "Expected normalized context compacted event. Client events: {client_events:?}"
    );
    assert!(
        has_warning,
        "Expected Warning event about long conversations. Events received: {warning_events:?}"
    );
    assert!(
        has_task_complete,
        "Expected normalized task complete event. Client events: {client_events:?}"
    );
}
