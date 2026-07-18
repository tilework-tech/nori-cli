use super::*;
use codex_protocol::config_types::McpServerConfig;
use codex_protocol::config_types::McpServerTransportConfig;

#[test]
fn set_config_updates_config_ref() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    // Initially empty
    assert!(
        chat.config_ref().mcp_servers.is_empty(),
        "mcp_servers should start empty"
    );

    // Set servers
    let mut servers = std::collections::HashMap::new();
    servers.insert(
        "test-server".to_string(),
        McpServerConfig {
            transport: McpServerTransportConfig::StreamableHttp {
                url: "https://example.com/mcp".to_string(),
                bearer_token_env_var: None,
                http_headers: None,
                env_http_headers: None,
                client_id: None,
                client_secret_env_var: None,
            },
            enabled: true,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
        },
    );
    let mut config = chat.config_ref().clone();
    config.mcp_servers = servers.clone();
    chat.set_config(config);

    // config_ref should now reflect the updated servers
    assert_eq!(
        chat.config_ref().mcp_servers.len(),
        1,
        "config_ref should show 1 server after replacing the runtime config"
    );
    assert!(
        chat.config_ref().mcp_servers.contains_key("test-server"),
        "config_ref should contain 'test-server'"
    );
}

#[test]
fn browse_command_uses_the_runtime_config_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let mut config = chat.config_ref().clone();
    config.file_manager = Some(nori_config::FileManager::Lf);
    chat.set_config(config);

    chat.dispatch_command(SlashCommand::Browse);

    assert_matches!(
        rx.try_recv(),
        Ok(AppEvent::BrowseFiles(nori_config::FileManager::Lf))
    );
}

#[test]
fn cancelling_phase_keeps_task_running_until_prompt_completed() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Prompt,
    ));
    drain_insert_history(&mut rx);

    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Cancelling,
    ));

    assert!(chat.bottom_pane.is_task_running());

    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Idle,
    ));
    chat.handle_client_event(nori_protocol::ClientEvent::PromptCompleted(
        nori_protocol::PromptCompleted {
            stop_reason: nori_protocol::StopReason::Cancelled,
            last_agent_message: None,
            failure: None,
        },
    ));
    drain_insert_history(&mut rx);

    assert!(!chat.bottom_pane.is_task_running());
}

#[test]
fn queue_projection_submission_during_cancelling_still_sends_user_input() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Prompt,
    ));
    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Cancelling,
    ));

    chat.submit_user_message(UserMessage::from("queued follow up"));

    assert!(matches!(op_rx.try_recv(), Ok(Op::UserInput { .. })));
}

#[test]
fn idle_phase_unknown_tool_snapshot_still_renders_visible_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Idle,
    ));
    drain_insert_history(&mut rx);

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-exec-idle".into(),
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
            owner_request_id: None,
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected visible ACP tool history cell");
    let blob = lines_to_single_string(cells.first().unwrap());
    assert!(
        blob.contains("git status"),
        "expected command in cell: {blob:?}"
    );
    assert!(
        blob.contains("On branch spec"),
        "expected output in cell: {blob:?}"
    );
}
