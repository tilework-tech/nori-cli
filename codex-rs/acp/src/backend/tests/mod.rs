use super::*;
use serial_test::serial;

async fn recv_backend_control(
    rx: &mut mpsc::Receiver<BackendEvent>,
    timeout: std::time::Duration,
) -> Option<Event> {
    while let Ok(event) = tokio::time::timeout(timeout, rx.recv()).await {
        match event {
            Some(BackendEvent::Control(event)) => return Some(event),
            Some(BackendEvent::Client(_)) => continue,
            None => return None,
        }
    }
    None
}

async fn recv_backend_client(
    rx: &mut mpsc::Receiver<BackendEvent>,
    timeout: std::time::Duration,
) -> Option<nori_protocol::ClientEvent> {
    while let Ok(event) = tokio::time::timeout(timeout, rx.recv()).await {
        match event {
            Some(BackendEvent::Client(event)) => return Some(event),
            Some(BackendEvent::Control(_)) => continue,
            None => return None,
        }
    }
    None
}

fn forward_test_backend_events(
    mut backend_event_rx: mpsc::Receiver<BackendEvent>,
    event_tx: mpsc::Sender<Event>,
    client_event_tx: Option<mpsc::Sender<nori_protocol::ClientEvent>>,
) {
    tokio::spawn(async move {
        while let Some(event) = backend_event_rx.recv().await {
            match event {
                BackendEvent::Control(event) => {
                    let _ = event_tx.send(event).await;
                }
                BackendEvent::Client(client_event) => {
                    if let Some(client_event_tx) = &client_event_tx {
                        let _ = client_event_tx.send(client_event).await;
                    }
                }
            }
        }
    });
}

async fn spawn_test_backend(
    config: &AcpBackendConfig,
    event_tx: mpsc::Sender<Event>,
    client_event_tx: Option<mpsc::Sender<nori_protocol::ClientEvent>>,
) -> anyhow::Result<AcpBackend> {
    let (backend_event_tx, backend_event_rx) = mpsc::channel(64);
    forward_test_backend_events(backend_event_rx, event_tx, client_event_tx);
    AcpBackend::spawn(config, backend_event_tx).await
}

#[allow(clippy::too_many_arguments)]
fn spawn_test_approval_handler(
    approval_rx: mpsc::Receiver<ApprovalRequest>,
    event_tx: mpsc::Sender<Event>,
    client_event_tx: Option<mpsc::Sender<nori_protocol::ClientEvent>>,
    pending_approvals: Arc<Mutex<Vec<ApprovalRequest>>>,
    user_notifier: Arc<codex_core::UserNotifier>,
    cwd: PathBuf,
    approval_policy_rx: watch::Receiver<AskForApproval>,
    pending_tool_calls: Arc<Mutex<HashMap<String, AccumulatedToolCall>>>,
    client_event_normalizer: Arc<Mutex<ClientEventNormalizer>>,
    transcript_recorder: Option<Arc<TranscriptRecorder>>,
) {
    let (backend_event_tx, backend_event_rx) = mpsc::channel(64);
    forward_test_backend_events(backend_event_rx, event_tx, client_event_tx);
    tokio::spawn(AcpBackend::run_approval_handler(
        approval_rx,
        backend_event_tx,
        pending_approvals,
        user_notifier,
        cwd,
        approval_policy_rx,
        pending_tool_calls,
        client_event_normalizer,
        transcript_recorder,
    ));
}

fn spawn_test_persistent_relay(
    persistent_rx: mpsc::Receiver<acp::SessionUpdate>,
    event_tx: mpsc::Sender<Event>,
    client_event_tx: Option<mpsc::Sender<nori_protocol::ClientEvent>>,
    client_event_normalizer: Arc<Mutex<ClientEventNormalizer>>,
) {
    let (backend_event_tx, backend_event_rx) = mpsc::channel(64);
    forward_test_backend_events(backend_event_rx, event_tx, client_event_tx);
    tokio::spawn(AcpBackend::run_persistent_relay(
        persistent_rx,
        client_event_normalizer,
        backend_event_tx,
    ));
}

/// Helper to build a minimal transcript for resume tests.
fn build_test_transcript() -> crate::transcript::Transcript {
    use crate::transcript::*;

    let entries = vec![
        TranscriptLine::new(TranscriptEntry::SessionMeta(SessionMetaEntry {
            session_id: "test-session-1".into(),
            project_id: "test-project".into(),
            started_at: "2025-01-01T00:00:00.000Z".into(),
            cwd: PathBuf::from("/tmp"),
            agent: Some("mock-agent".into()),
            cli_version: "0.1.0".into(),
            git: None,
            acp_session_id: Some("acp-session-42".into()),
        })),
        TranscriptLine::new(TranscriptEntry::User(UserEntry {
            id: "msg-001".into(),
            content: "Hello, world!".into(),
            attachments: vec![],
        })),
        TranscriptLine::new(TranscriptEntry::Assistant(AssistantEntry {
            id: "msg-002".into(),
            content: vec![ContentBlock::Text {
                text: "Hi there! I can help.".into(),
            }],
            agent: Some("mock-agent".into()),
        })),
    ];

    crate::transcript::Transcript {
        meta: match &entries[0].entry {
            TranscriptEntry::SessionMeta(m) => m.clone(),
            _ => unreachable!(),
        },
        entries,
    }
}

/// Helper to build a standard AcpBackendConfig for testing.
fn build_test_config(temp_dir: &std::path::Path) -> AcpBackendConfig {
    AcpBackendConfig {
        agent: "mock-model".to_string(),
        cwd: temp_dir.to_path_buf(),
        approval_policy: AskForApproval::Never,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        notify: None,
        os_notifications: crate::config::OsNotifications::Disabled,
        nori_home: temp_dir.to_path_buf(),
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
        mcp_servers: std::collections::HashMap::new(),
        mcp_oauth_credentials_store_mode: codex_rmcp_client::OAuthCredentialsStoreMode::default(),
    }
}

mod part2;
mod part3;
mod part4;
