use super::*;
use crate::config::ConfigOverrides;
use crate::config::ConfigToml;
use crate::exec::ExecToolCallOutput;
use crate::shell::default_user_shell;
use crate::tools::format_exec_output_str;

use crate::protocol::CompactedItem;
use crate::protocol::InitialHistory;
use crate::protocol::ResumedHistory;
use crate::state::TaskKind;
use crate::tasks::SessionTask;
use crate::tasks::SessionTaskContext;
use crate::tools::ToolRouter;
use crate::tools::context::ToolInvocation;
use crate::tools::context::ToolOutput;
use crate::tools::context::ToolPayload;
use crate::tools::handlers::ShellHandler;
use crate::tools::handlers::UnifiedExecHandler;
use crate::tools::registry::ToolHandler;
use crate::turn_diff_tracker::TurnDiffTracker;
use codex_app_server_protocol::AuthMode;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use std::time::Duration;
use tokio::time::sleep;

use mcp_types::ContentBlock;
use mcp_types::TextContent;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration as StdDuration;

#[test]
fn reconstruct_history_matches_live_compactions() {
    let (session, turn_context) = make_session_and_context();
    let (rollout_items, expected) = sample_rollout(&session, &turn_context);

    let reconstructed = session.reconstruct_history_from_rollout(&turn_context, &rollout_items);

    assert_eq!(expected, reconstructed);
}

#[test]
fn record_initial_history_reconstructs_resumed_transcript() {
    let (session, turn_context) = make_session_and_context();
    let (rollout_items, expected) = sample_rollout(&session, &turn_context);

    tokio_test::block_on(
        session.record_initial_history(InitialHistory::Resumed(ResumedHistory {
            conversation_id: ConversationId::default(),
            history: rollout_items,
            rollout_path: PathBuf::from("/tmp/resume.jsonl"),
        })),
    );

    let actual =
        tokio_test::block_on(async { session.state.lock().await.clone_history().get_history() });
    assert_eq!(expected, actual);
}

#[test]
fn record_initial_history_reconstructs_forked_transcript() {
    let (session, turn_context) = make_session_and_context();
    let (rollout_items, expected) = sample_rollout(&session, &turn_context);

    tokio_test::block_on(session.record_initial_history(InitialHistory::Forked(rollout_items)));

    let actual =
        tokio_test::block_on(async { session.state.lock().await.clone_history().get_history() });
    assert_eq!(expected, actual);
}

#[test]
fn prefers_structured_content_when_present() {
    let ctr = CallToolResult {
        // Content present but should be ignored because structured_content is set.
        content: vec![text_block("ignored")],
        is_error: None,
        structured_content: Some(json!({
            "ok": true,
            "value": 42
        })),
    };

    let got = FunctionCallOutputPayload::from(&ctr);
    let expected = FunctionCallOutputPayload {
        content: serde_json::to_string(&json!({
            "ok": true,
            "value": 42
        }))
        .unwrap(),
        success: Some(true),
        ..Default::default()
    };

    assert_eq!(expected, got);
}

#[test]
fn includes_timed_out_message() {
    let exec = ExecToolCallOutput {
        exit_code: 0,
        stdout: StreamOutput::new(String::new()),
        stderr: StreamOutput::new(String::new()),
        aggregated_output: StreamOutput::new("Command output".to_string()),
        duration: StdDuration::from_secs(1),
        timed_out: true,
    };
    let (_, turn_context) = make_session_and_context();

    let out = format_exec_output_str(&exec, turn_context.truncation_policy);

    assert_eq!(
        out,
        "command timed out after 1000 milliseconds\nCommand output"
    );
}

#[test]
fn falls_back_to_content_when_structured_is_null() {
    let ctr = CallToolResult {
        content: vec![text_block("hello"), text_block("world")],
        is_error: None,
        structured_content: Some(serde_json::Value::Null),
    };

    let got = FunctionCallOutputPayload::from(&ctr);
    let expected = FunctionCallOutputPayload {
        content: serde_json::to_string(&vec![text_block("hello"), text_block("world")]).unwrap(),
        success: Some(true),
        ..Default::default()
    };

    assert_eq!(expected, got);
}

#[test]
fn success_flag_reflects_is_error_true() {
    let ctr = CallToolResult {
        content: vec![text_block("unused")],
        is_error: Some(true),
        structured_content: Some(json!({ "message": "bad" })),
    };

    let got = FunctionCallOutputPayload::from(&ctr);
    let expected = FunctionCallOutputPayload {
        content: serde_json::to_string(&json!({ "message": "bad" })).unwrap(),
        success: Some(false),
        ..Default::default()
    };

    assert_eq!(expected, got);
}

#[test]
fn success_flag_true_with_no_error_and_content_used() {
    let ctr = CallToolResult {
        content: vec![text_block("alpha")],
        is_error: Some(false),
        structured_content: None,
    };

    let got = FunctionCallOutputPayload::from(&ctr);
    let expected = FunctionCallOutputPayload {
        content: serde_json::to_string(&vec![text_block("alpha")]).unwrap(),
        success: Some(true),
        ..Default::default()
    };

    assert_eq!(expected, got);
}

fn text_block(s: &str) -> ContentBlock {
    ContentBlock::TextContent(TextContent {
        annotations: None,
        text: s.to_string(),
        r#type: "text".to_string(),
    })
}

fn otel_event_manager(conversation_id: ConversationId, config: &Config) -> OtelEventManager {
    OtelEventManager::new(
        conversation_id,
        config.model.as_str(),
        config.model_family.slug.as_str(),
        None,
        Some("test@test.com".to_string()),
        Some(AuthMode::ChatGPT),
        false,
        "test".to_string(),
    )
}

pub(crate) fn make_session_and_context() -> (Session, TurnContext) {
    let (tx_event, _rx_event) = async_channel::unbounded();
    let codex_home = tempfile::tempdir().expect("create temp dir");
    let config = Config::load_from_base_config_with_overrides(
        ConfigToml::default(),
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )
    .expect("load default test config");
    let config = Arc::new(config);
    let conversation_id = ConversationId::default();
    let otel_event_manager = otel_event_manager(conversation_id, config.as_ref());
    let auth_manager = AuthManager::shared(
        config.cwd.clone(),
        false,
        config.cli_auth_credentials_store_mode,
    );

    let session_configuration = SessionConfiguration {
        provider: config.model_provider.clone(),
        model: config.model.clone(),
        model_reasoning_effort: config.model_reasoning_effort,
        model_reasoning_summary: config.model_reasoning_summary,
        developer_instructions: config.developer_instructions.clone(),
        user_instructions: config.user_instructions.clone(),
        base_instructions: config.base_instructions.clone(),
        compact_prompt: config.compact_prompt.clone(),
        approval_policy: config.approval_policy,
        sandbox_policy: config.sandbox_policy.clone(),
        cwd: config.cwd.clone(),
        original_config_do_not_use: Arc::clone(&config),
        features: Features::default(),
        exec_policy: Arc::new(ExecPolicy::empty()),
        session_source: SessionSource::Exec,
    };

    let state = SessionState::new(session_configuration.clone());

    let services = SessionServices {
        mcp_connection_manager: Arc::new(RwLock::new(McpConnectionManager::default())),
        mcp_startup_cancellation_token: CancellationToken::new(),
        unified_exec_manager: UnifiedExecSessionManager::default(),
        notifier: UserNotifier::new(None, false),
        rollout: Mutex::new(None),
        user_shell: default_user_shell(),
        show_raw_agent_reasoning: config.show_raw_agent_reasoning,
        auth_manager: Arc::clone(&auth_manager),
        otel_event_manager: otel_event_manager.clone(),
        tool_approvals: Mutex::new(ApprovalStore::default()),
    };

    let turn_context = Session::make_turn_context(
        Some(Arc::clone(&auth_manager)),
        &otel_event_manager,
        session_configuration.provider.clone(),
        &session_configuration,
        conversation_id,
        "turn_id".to_string(),
    );

    let session = Session {
        conversation_id,
        tx_event,
        state: Mutex::new(state),
        active_turn: Mutex::new(None),
        services,
        next_internal_sub_id: AtomicU64::new(0),
    };

    (session, turn_context)
}

// Like make_session_and_context, but returns Arc<Session> and the event receiver
// so tests can assert on emitted events.
pub(crate) fn make_session_and_context_with_rx() -> (
    Arc<Session>,
    Arc<TurnContext>,
    async_channel::Receiver<Event>,
) {
    let (tx_event, rx_event) = async_channel::unbounded();
    let codex_home = tempfile::tempdir().expect("create temp dir");
    let config = Config::load_from_base_config_with_overrides(
        ConfigToml::default(),
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )
    .expect("load default test config");
    let config = Arc::new(config);
    let conversation_id = ConversationId::default();
    let otel_event_manager = otel_event_manager(conversation_id, config.as_ref());
    let auth_manager = AuthManager::shared(
        config.cwd.clone(),
        false,
        config.cli_auth_credentials_store_mode,
    );

    let session_configuration = SessionConfiguration {
        provider: config.model_provider.clone(),
        model: config.model.clone(),
        model_reasoning_effort: config.model_reasoning_effort,
        model_reasoning_summary: config.model_reasoning_summary,
        developer_instructions: config.developer_instructions.clone(),
        user_instructions: config.user_instructions.clone(),
        base_instructions: config.base_instructions.clone(),
        compact_prompt: config.compact_prompt.clone(),
        approval_policy: config.approval_policy,
        sandbox_policy: config.sandbox_policy.clone(),
        cwd: config.cwd.clone(),
        original_config_do_not_use: Arc::clone(&config),
        features: Features::default(),
        exec_policy: Arc::new(ExecPolicy::empty()),
        session_source: SessionSource::Exec,
    };

    let state = SessionState::new(session_configuration.clone());

    let services = SessionServices {
        mcp_connection_manager: Arc::new(RwLock::new(McpConnectionManager::default())),
        mcp_startup_cancellation_token: CancellationToken::new(),
        unified_exec_manager: UnifiedExecSessionManager::default(),
        notifier: UserNotifier::new(None, false),
        rollout: Mutex::new(None),
        user_shell: default_user_shell(),
        show_raw_agent_reasoning: config.show_raw_agent_reasoning,
        auth_manager: Arc::clone(&auth_manager),
        otel_event_manager: otel_event_manager.clone(),
        tool_approvals: Mutex::new(ApprovalStore::default()),
    };

    let turn_context = Arc::new(Session::make_turn_context(
        Some(Arc::clone(&auth_manager)),
        &otel_event_manager,
        session_configuration.provider.clone(),
        &session_configuration,
        conversation_id,
        "turn_id".to_string(),
    ));

    let session = Arc::new(Session {
        conversation_id,
        tx_event,
        state: Mutex::new(state),
        active_turn: Mutex::new(None),
        services,
        next_internal_sub_id: AtomicU64::new(0),
    });

    (session, turn_context, rx_event)
}

#[derive(Clone, Copy)]
struct NeverEndingTask {
    kind: TaskKind,
    listen_to_cancellation_token: bool,
}

#[async_trait::async_trait]
impl SessionTask for NeverEndingTask {
    fn kind(&self) -> TaskKind {
        self.kind
    }

    async fn run(
        self: Arc<Self>,
        _session: Arc<SessionTaskContext>,
        _ctx: Arc<TurnContext>,
        _input: Vec<UserInput>,
        cancellation_token: CancellationToken,
    ) -> Option<String> {
        if self.listen_to_cancellation_token {
            cancellation_token.cancelled().await;
            return None;
        }
        loop {
            sleep(Duration::from_secs(60)).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[test_log::test]
async fn abort_regular_task_emits_turn_aborted_only() {
    let (sess, tc, rx) = make_session_and_context_with_rx();
    let input = vec![UserInput::Text {
        text: "hello".to_string(),
    }];
    sess.spawn_task(
        Arc::clone(&tc),
        input,
        NeverEndingTask {
            kind: TaskKind::Regular,
            listen_to_cancellation_token: false,
        },
    )
    .await;

    sess.abort_all_tasks(TurnAbortReason::Interrupted).await;

    let evt = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout waiting for event")
        .expect("event");
    match evt.msg {
        EventMsg::TurnAborted(e) => assert_eq!(TurnAbortReason::Interrupted, e.reason),
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn abort_gracefuly_emits_turn_aborted_only() {
    let (sess, tc, rx) = make_session_and_context_with_rx();
    let input = vec![UserInput::Text {
        text: "hello".to_string(),
    }];
    sess.spawn_task(
        Arc::clone(&tc),
        input,
        NeverEndingTask {
            kind: TaskKind::Regular,
            listen_to_cancellation_token: true,
        },
    )
    .await;

    sess.abort_all_tasks(TurnAbortReason::Interrupted).await;

    let evt = rx.recv().await.expect("event");
    match evt.msg {
        EventMsg::TurnAborted(e) => assert_eq!(TurnAbortReason::Interrupted, e.reason),
        other => panic!("unexpected event: {other:?}"),
    }
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn fatal_tool_error_stops_turn_and_reports_error() {
    let (session, turn_context, _rx) = make_session_and_context_with_rx();
    let tools = {
        session
            .services
            .mcp_connection_manager
            .read()
            .await
            .list_all_tools()
            .await
    };
    let router = ToolRouter::from_config(
        &turn_context.tools_config,
        Some(
            tools
                .into_iter()
                .map(|(name, tool)| (name, tool.tool))
                .collect(),
        ),
    );
    let item = ResponseItem::CustomToolCall {
        id: None,
        status: None,
        call_id: "call-1".to_string(),
        name: "shell".to_string(),
        input: "{}".to_string(),
    };

    let call = ToolRouter::build_tool_call(session.as_ref(), item.clone())
        .await
        .expect("build tool call")
        .expect("tool call present");
    let tracker = Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new()));
    let err = router
        .dispatch_tool_call(
            Arc::clone(&session),
            Arc::clone(&turn_context),
            tracker,
            call,
        )
        .await
        .expect_err("expected fatal error");

    match err {
        FunctionCallError::Fatal(message) => {
            assert_eq!(message, "tool shell invoked with incompatible payload");
        }
        other => panic!("expected FunctionCallError::Fatal, got {other:?}"),
    }
}

fn sample_rollout(
    session: &Session,
    turn_context: &TurnContext,
) -> (Vec<RolloutItem>, Vec<ResponseItem>) {
    let mut rollout_items = Vec::new();
    let mut live_history = ContextManager::new();

    let initial_context = session.build_initial_context(turn_context);
    for item in &initial_context {
        rollout_items.push(RolloutItem::ResponseItem(item.clone()));
    }
    live_history.record_items(initial_context.iter(), turn_context.truncation_policy);

    let user1 = ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "first user".to_string(),
        }],
    };
    live_history.record_items(std::iter::once(&user1), turn_context.truncation_policy);
    rollout_items.push(RolloutItem::ResponseItem(user1.clone()));

    let assistant1 = ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: "assistant reply one".to_string(),
        }],
    };
    live_history.record_items(std::iter::once(&assistant1), turn_context.truncation_policy);
    rollout_items.push(RolloutItem::ResponseItem(assistant1.clone()));

    let summary1 = "summary one";
    let snapshot1 = live_history.get_history();
    let user_messages1 = collect_user_messages(&snapshot1);
    let rebuilt1 = compact::build_compacted_history(
        session.build_initial_context(turn_context),
        &user_messages1,
        summary1,
    );
    live_history.replace(rebuilt1);
    rollout_items.push(RolloutItem::Compacted(CompactedItem {
        message: summary1.to_string(),
        replacement_history: None,
    }));

    let user2 = ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "second user".to_string(),
        }],
    };
    live_history.record_items(std::iter::once(&user2), turn_context.truncation_policy);
    rollout_items.push(RolloutItem::ResponseItem(user2.clone()));

    let assistant2 = ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: "assistant reply two".to_string(),
        }],
    };
    live_history.record_items(std::iter::once(&assistant2), turn_context.truncation_policy);
    rollout_items.push(RolloutItem::ResponseItem(assistant2.clone()));

    let summary2 = "summary two";
    let snapshot2 = live_history.get_history();
    let user_messages2 = collect_user_messages(&snapshot2);
    let rebuilt2 = compact::build_compacted_history(
        session.build_initial_context(turn_context),
        &user_messages2,
        summary2,
    );
    live_history.replace(rebuilt2);
    rollout_items.push(RolloutItem::Compacted(CompactedItem {
        message: summary2.to_string(),
        replacement_history: None,
    }));

    let user3 = ResponseItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "third user".to_string(),
        }],
    };
    live_history.record_items(std::iter::once(&user3), turn_context.truncation_policy);
    rollout_items.push(RolloutItem::ResponseItem(user3.clone()));

    let assistant3 = ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText {
            text: "assistant reply three".to_string(),
        }],
    };
    live_history.record_items(std::iter::once(&assistant3), turn_context.truncation_policy);
    rollout_items.push(RolloutItem::ResponseItem(assistant3.clone()));

    (rollout_items, live_history.get_history())
}

#[tokio::test]
async fn rejects_escalated_permissions_when_policy_not_on_request() {
    use crate::exec::ExecParams;
    use crate::protocol::AskForApproval;
    use crate::protocol::SandboxPolicy;
    use crate::turn_diff_tracker::TurnDiffTracker;
    use std::collections::HashMap;

    let (session, mut turn_context_raw) = make_session_and_context();
    // Ensure policy is NOT OnRequest so the early rejection path triggers
    turn_context_raw.approval_policy = AskForApproval::OnFailure;
    let session = Arc::new(session);
    let mut turn_context = Arc::new(turn_context_raw);

    let timeout_ms = 1000;
    let params = ExecParams {
        command: if cfg!(windows) {
            vec![
                "cmd.exe".to_string(),
                "/C".to_string(),
                "echo hi".to_string(),
            ]
        } else {
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "echo hi".to_string(),
            ]
        },
        cwd: turn_context.cwd.clone(),
        expiration: timeout_ms.into(),
        env: HashMap::new(),
        with_escalated_permissions: Some(true),
        justification: Some("test".to_string()),
        arg0: None,
    };

    let params2 = ExecParams {
        with_escalated_permissions: Some(false),
        command: params.command.clone(),
        cwd: params.cwd.clone(),
        expiration: timeout_ms.into(),
        env: HashMap::new(),
        justification: params.justification.clone(),
        arg0: None,
    };

    let turn_diff_tracker = Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new()));

    let tool_name = "shell";
    let call_id = "test-call".to_string();

    let handler = ShellHandler;
    let resp = handler
        .handle(ToolInvocation {
            session: Arc::clone(&session),
            turn: Arc::clone(&turn_context),
            tracker: Arc::clone(&turn_diff_tracker),
            call_id,
            tool_name: tool_name.to_string(),
            payload: ToolPayload::Function {
                arguments: serde_json::json!({
                    "command": params.command.clone(),
                    "workdir": Some(turn_context.cwd.to_string_lossy().to_string()),
                    "timeout_ms": params.expiration.timeout_ms(),
                    "with_escalated_permissions": params.with_escalated_permissions,
                    "justification": params.justification.clone(),
                })
                .to_string(),
            },
        })
        .await;

    let Err(FunctionCallError::RespondToModel(output)) = resp else {
        panic!("expected error result");
    };

    let expected = format!(
        "approval policy is {policy:?}; reject command — you should not ask for escalated permissions if the approval policy is {policy:?}",
        policy = turn_context.approval_policy
    );

    pretty_assertions::assert_eq!(output, expected);

    // Now retry the same command WITHOUT escalated permissions; should succeed.
    // Force DangerFullAccess to avoid platform sandbox dependencies in tests.
    Arc::get_mut(&mut turn_context)
        .expect("unique turn context Arc")
        .sandbox_policy = SandboxPolicy::DangerFullAccess;

    let resp2 = handler
        .handle(ToolInvocation {
            session: Arc::clone(&session),
            turn: Arc::clone(&turn_context),
            tracker: Arc::clone(&turn_diff_tracker),
            call_id: "test-call-2".to_string(),
            tool_name: tool_name.to_string(),
            payload: ToolPayload::Function {
                arguments: serde_json::json!({
                    "command": params2.command.clone(),
                    "workdir": Some(turn_context.cwd.to_string_lossy().to_string()),
                    "timeout_ms": params2.expiration.timeout_ms(),
                    "with_escalated_permissions": params2.with_escalated_permissions,
                    "justification": params2.justification.clone(),
                })
                .to_string(),
            },
        })
        .await;

    let output = match resp2.expect("expected Ok result") {
        ToolOutput::Function { content, .. } => content,
        _ => panic!("unexpected tool output"),
    };

    #[derive(Deserialize, PartialEq, Eq, Debug)]
    struct ResponseExecMetadata {
        exit_code: i32,
    }

    #[derive(Deserialize)]
    struct ResponseExecOutput {
        output: String,
        metadata: ResponseExecMetadata,
    }

    let exec_output: ResponseExecOutput =
        serde_json::from_str(&output).expect("valid exec output json");

    pretty_assertions::assert_eq!(exec_output.metadata, ResponseExecMetadata { exit_code: 0 });
    assert!(exec_output.output.contains("hi"));
}
#[tokio::test]
async fn unified_exec_rejects_escalated_permissions_when_policy_not_on_request() {
    use crate::protocol::AskForApproval;
    use crate::turn_diff_tracker::TurnDiffTracker;

    let (session, mut turn_context_raw) = make_session_and_context();
    turn_context_raw.approval_policy = AskForApproval::OnFailure;
    let session = Arc::new(session);
    let turn_context = Arc::new(turn_context_raw);
    let tracker = Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new()));

    let handler = UnifiedExecHandler;
    let resp = handler
        .handle(ToolInvocation {
            session: Arc::clone(&session),
            turn: Arc::clone(&turn_context),
            tracker: Arc::clone(&tracker),
            call_id: "exec-call".to_string(),
            tool_name: "exec_command".to_string(),
            payload: ToolPayload::Function {
                arguments: serde_json::json!({
                    "cmd": "echo hi",
                    "with_escalated_permissions": true,
                    "justification": "need unsandboxed execution",
                })
                .to_string(),
            },
        })
        .await;

    let Err(FunctionCallError::RespondToModel(output)) = resp else {
        panic!("expected error result");
    };

    let expected = format!(
        "approval policy is {policy:?}; reject command — you cannot ask for escalated permissions if the approval policy is {policy:?}",
        policy = turn_context.approval_policy
    );

    pretty_assertions::assert_eq!(output, expected);
}
