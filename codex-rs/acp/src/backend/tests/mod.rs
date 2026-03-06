use super::*;
use serial_test::serial;

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
    }
}

mod part1;
mod part2;
mod part3;
mod part4;
mod part5;
mod part6;
