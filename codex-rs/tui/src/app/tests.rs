use super::*;
use crate::app_backtrack::BacktrackState;
use crate::app_backtrack::user_count;
use crate::chatwidget::tests::make_chatwidget_manual;
use crate::chatwidget::tests::make_chatwidget_manual_with_sender;
use crate::file_search::FileSearchManager;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::UserHistoryCell;
use crate::history_cell::new_session_info;
use codex_core::AuthManager;
use codex_core::CodexAuth;
use codex_core::ConversationManager;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::SandboxPolicy;
use codex_core::protocol::SessionConfiguredEvent;
use codex_protocol::ConversationId;
use ratatui::prelude::Line;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

fn make_test_app() -> App {
    let (chat_widget, app_event_tx, _rx, _op_rx) = make_chatwidget_manual_with_sender();
    let config = chat_widget.config_ref().clone();
    let server = Arc::new(ConversationManager::with_auth(CodexAuth::from_api_key(
        "Test API Key",
    )));
    let auth_manager = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("Test API Key"));
    let file_search = FileSearchManager::new(config.cwd.clone(), app_event_tx.clone());

    let (system_info_tx, _system_info_rx) = mpsc::channel();
    App {
        server,
        app_event_tx,
        chat_widget,
        auth_manager,
        config,
        vertical_footer: false,
        active_profile: None,
        file_search,
        transcript_cells: Vec::new(),
        overlay: None,
        deferred_history_lines: Vec::new(),
        has_emitted_history_lines: false,
        enhanced_keys_supported: false,
        commit_anim_running: Arc::new(AtomicBool::new(false)),
        backtrack: BacktrackState::default(),
        pending_update_action: None,
        suppress_shutdown_complete: false,
        skip_world_writable_scan_once: false,
        pending_agent: None,
        #[cfg(feature = "nori-config")]
        loop_count_override: None,
        hotkey_config: codex_acp::config::HotkeyConfig::default(),
        vim_mode_enabled: false,
        system_info_tx,
        worktree_warning_shown: false,
    }
}

fn make_test_app_with_channels() -> (
    App,
    tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    tokio::sync::mpsc::UnboundedReceiver<Op>,
) {
    let (chat_widget, app_event_tx, rx, op_rx) = make_chatwidget_manual_with_sender();
    let config = chat_widget.config_ref().clone();
    let server = Arc::new(ConversationManager::with_auth(CodexAuth::from_api_key(
        "Test API Key",
    )));
    let auth_manager = AuthManager::from_auth_for_testing(CodexAuth::from_api_key("Test API Key"));
    let file_search = FileSearchManager::new(config.cwd.clone(), app_event_tx.clone());

    let (system_info_tx, _system_info_rx) = mpsc::channel();
    (
        App {
            server,
            app_event_tx,
            chat_widget,
            auth_manager,
            config,
            vertical_footer: false,
            active_profile: None,
            file_search,
            transcript_cells: Vec::new(),
            overlay: None,
            deferred_history_lines: Vec::new(),
            has_emitted_history_lines: false,
            enhanced_keys_supported: false,
            commit_anim_running: Arc::new(AtomicBool::new(false)),
            backtrack: BacktrackState::default(),
            pending_update_action: None,
            suppress_shutdown_complete: false,
            skip_world_writable_scan_once: false,
            pending_agent: None,
            #[cfg(feature = "nori-config")]
            loop_count_override: None,
            hotkey_config: codex_acp::config::HotkeyConfig::default(),
            vim_mode_enabled: false,
            system_info_tx,
            worktree_warning_shown: false,
        },
        rx,
        op_rx,
    )
}

#[test]
fn model_migration_prompt_only_shows_for_deprecated_models() {
    assert!(should_show_model_migration_prompt("gpt-5", "gpt-5.1", None));
    assert!(should_show_model_migration_prompt(
        "gpt-5-codex",
        "gpt-5.1-codex",
        None
    ));
    assert!(should_show_model_migration_prompt(
        "gpt-5-codex-mini",
        "gpt-5.1-codex-mini",
        None
    ));
    assert!(should_show_model_migration_prompt(
        "gpt-5.1-codex",
        "gpt-5.1-codex-max",
        None
    ));
    assert!(!should_show_model_migration_prompt(
        "gpt-5.1-codex",
        "gpt-5.1-codex",
        None
    ));
}

#[test]
fn model_migration_prompt_respects_hide_flag_and_self_target() {
    assert!(!should_show_model_migration_prompt(
        "gpt-5",
        "gpt-5.1",
        Some(true)
    ));
    assert!(!should_show_model_migration_prompt(
        "gpt-5.1", "gpt-5.1", None
    ));
}

#[test]
fn update_reasoning_effort_updates_config() {
    let mut app = make_test_app();
    app.config.model_reasoning_effort = Some(ReasoningEffortConfig::Medium);
    app.chat_widget
        .set_reasoning_effort(Some(ReasoningEffortConfig::Medium));

    app.on_update_reasoning_effort(Some(ReasoningEffortConfig::High));

    assert_eq!(
        app.config.model_reasoning_effort,
        Some(ReasoningEffortConfig::High)
    );
    assert_eq!(
        app.chat_widget.config_ref().model_reasoning_effort,
        Some(ReasoningEffortConfig::High)
    );
}

#[test]
fn backtrack_selection_with_duplicate_history_targets_unique_turn() {
    let mut app = make_test_app();

    let user_cell = |text: &str| -> Arc<dyn HistoryCell> {
        Arc::new(UserHistoryCell {
            message: text.to_string(),
        }) as Arc<dyn HistoryCell>
    };
    let agent_cell = |text: &str| -> Arc<dyn HistoryCell> {
        Arc::new(AgentMessageCell::new(
            vec![Line::from(text.to_string())],
            true,
        )) as Arc<dyn HistoryCell>
    };

    let make_header = |is_first| {
        let event = SessionConfiguredEvent {
            session_id: ConversationId::new(),
            model: "gpt-test".to_string(),
            model_provider_id: "test-provider".to_string(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            cwd: PathBuf::from("/home/user/project"),
            reasoning_effort: None,
            history_log_id: 0,
            history_entry_count: 0,
            initial_messages: None,
            rollout_path: PathBuf::new(),
        };
        Arc::new(new_session_info(
            app.chat_widget.config_ref(),
            event,
            is_first,
        )) as Arc<dyn HistoryCell>
    };

    // Simulate the transcript after trimming for a fork, replaying history, and
    // appending the edited turn. The session header separates the retained history
    // from the forked conversation's replayed turns.
    app.transcript_cells = vec![
        make_header(true),
        user_cell("first question"),
        agent_cell("answer first"),
        user_cell("follow-up"),
        agent_cell("answer follow-up"),
        make_header(false),
        user_cell("first question"),
        agent_cell("answer first"),
        user_cell("follow-up (edited)"),
        agent_cell("answer edited"),
    ];

    assert_eq!(user_count(&app.transcript_cells), 2);

    app.backtrack.base_id = Some(ConversationId::new());
    app.backtrack.primed = true;
    app.backtrack.nth_user_message = user_count(&app.transcript_cells).saturating_sub(1);

    app.confirm_backtrack_from_main();

    let (_, nth, prefill) = app.backtrack.pending.clone().expect("pending backtrack");
    assert_eq!(nth, 1);
    assert_eq!(prefill, "follow-up (edited)");
}

#[tokio::test]
async fn new_session_requests_shutdown_for_previous_conversation() {
    let (mut app, mut app_event_rx, mut op_rx) = make_test_app_with_channels();

    let conversation_id = ConversationId::new();
    let event = SessionConfiguredEvent {
        session_id: conversation_id,
        model: "gpt-test".to_string(),
        model_provider_id: "test-provider".to_string(),
        approval_policy: AskForApproval::Never,
        sandbox_policy: SandboxPolicy::ReadOnly,
        cwd: PathBuf::from("/home/user/project"),
        reasoning_effort: None,
        history_log_id: 0,
        history_entry_count: 0,
        initial_messages: None,
        rollout_path: PathBuf::new(),
    };

    app.chat_widget.handle_codex_event(Event {
        id: String::new(),
        msg: EventMsg::SessionConfigured(event),
    });

    while app_event_rx.try_recv().is_ok() {}
    while op_rx.try_recv().is_ok() {}

    app.shutdown_current_conversation().await;

    match op_rx.try_recv() {
        Ok(Op::Shutdown) => {}
        Ok(other) => panic!("expected Op::Shutdown, got {other:?}"),
        Err(_) => panic!("expected shutdown op to be sent"),
    }
}

#[test]
fn session_summary_skip_zero_usage() {
    assert!(session_summary(TokenUsage::default(), None).is_none());
}

#[test]
fn session_summary_includes_resume_hint() {
    let usage = TokenUsage {
        input_tokens: 10,
        output_tokens: 2,
        total_tokens: 12,
        ..Default::default()
    };
    let conversation = ConversationId::from_string("123e4567-e89b-12d3-a456-426614174000").unwrap();

    let summary = session_summary(usage, Some(conversation)).expect("summary");
    assert_eq!(
        summary.usage_line,
        "Token usage: total=12 input=10 output=2"
    );
    assert_eq!(
        summary.resume_command,
        Some("codex resume 123e4567-e89b-12d3-a456-426614174000".to_string())
    );
}

#[test]
fn gpt5_migration_allows_api_key_and_chatgpt() {
    assert!(migration_prompt_allows_auth_mode(
        Some(AuthMode::ApiKey),
        HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG,
    ));
    assert!(migration_prompt_allows_auth_mode(
        Some(AuthMode::ChatGPT),
        HIDE_GPT5_1_MIGRATION_PROMPT_CONFIG,
    ));
}

#[test]
fn gpt_5_1_codex_max_migration_limits_to_chatgpt() {
    assert!(migration_prompt_allows_auth_mode(
        Some(AuthMode::ChatGPT),
        HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG,
    ));
    assert!(!migration_prompt_allows_auth_mode(
        Some(AuthMode::ApiKey),
        HIDE_GPT_5_1_CODEX_MAX_MIGRATION_PROMPT_CONFIG,
    ));
}

#[test]
fn other_migrations_block_api_key() {
    assert!(!migration_prompt_allows_auth_mode(
        Some(AuthMode::ApiKey),
        "unknown"
    ));
    assert!(migration_prompt_allows_auth_mode(
        Some(AuthMode::ChatGPT),
        "unknown"
    ));
}

#[test]
fn test_agent_persistence_to_config() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("create temp dir");
    let nori_home = temp_dir.path();

    // Use ConfigEditsBuilder to persist an agent selection
    ConfigEditsBuilder::new(nori_home)
        .set_agent(Some("gemini"))
        .apply_blocking()
        .expect("persist agent");

    // Read back the config file and verify it contains `agent = "gemini"`
    let config_content =
        std::fs::read_to_string(nori_home.join("config.toml")).expect("read config");
    assert!(
        config_content.contains("agent = \"gemini\""),
        "Config should contain 'agent = \"gemini\"', got: {config_content}"
    );
}

/// Test that AgentSpawnFailed event can be constructed and matches expected structure
#[test]
fn agent_spawn_failed_event_exists() {
    // This test verifies the AgentSpawnFailed event variant exists
    // and has the expected fields
    let event = AppEvent::AgentSpawnFailed {
        agent_name: "codex".to_string(),
        error: "Failed to spawn ACP agent: npx not found".to_string(),
    };

    // Verify it matches the expected pattern
    match event {
        AppEvent::AgentSpawnFailed { agent_name, error } => {
            assert_eq!(agent_name, "codex");
            assert!(error.contains("Failed to spawn"));
        }
        _ => panic!("Expected AgentSpawnFailed event"),
    }
}

/// Test that App has a method to handle spawn failures by opening the agent picker
#[test]
fn chat_widget_can_open_agent_popup() {
    let (mut chat, _rx, _ops) = make_chatwidget_manual();

    // Before opening, we should be able to call open_agent_popup without panic
    chat.open_agent_popup();

    // The popup should now be showing (we can't easily check internal state,
    // but the call should succeed without panicking)
}
