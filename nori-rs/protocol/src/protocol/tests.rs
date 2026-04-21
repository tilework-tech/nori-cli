use super::*;
use crate::items::UserMessageItem;
use crate::items::WebSearchItem;
use anyhow::Result;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::NamedTempFile;

#[test]
fn item_started_event_from_web_search_emits_begin_event() {
    let event = ItemStartedEvent {
        thread_id: ConversationId::new(),
        turn_id: "turn-1".into(),
        item: TurnItem::WebSearch(WebSearchItem {
            id: "search-1".into(),
            query: "find docs".into(),
        }),
    };

    let legacy_events = event.as_legacy_events(false);
    assert_eq!(legacy_events.len(), 1);
    match &legacy_events[0] {
        EventMsg::WebSearchBegin(event) => assert_eq!(event.call_id, "search-1"),
        _ => panic!("expected WebSearchBegin event"),
    }
}

#[test]
fn item_started_event_from_non_web_search_emits_no_legacy_events() {
    let event = ItemStartedEvent {
        thread_id: ConversationId::new(),
        turn_id: "turn-1".into(),
        item: TurnItem::UserMessage(UserMessageItem::new(&[])),
    };

    assert!(event.as_legacy_events(false).is_empty());
}

/// Serialize Event to verify that its JSON representation has the expected
/// amount of nesting.
#[test]
fn serialize_event() -> Result<()> {
    let conversation_id = ConversationId::from_string("67e55044-10b1-426f-9247-bb680e5fe0c8")?;
    let rollout_file = NamedTempFile::new()?;
    let event = Event {
        id: "1234".to_string(),
        msg: EventMsg::SessionConfigured(SessionConfiguredEvent {
            session_id: conversation_id,
            model: "codex-mini-latest".to_string(),
            model_provider_id: "openai".to_string(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::ReadOnly,
            cwd: PathBuf::from("/home/user/project"),
            reasoning_effort: Some(ReasoningEffortConfig::default()),
            history_log_id: 0,
            history_entry_count: 0,
            initial_messages: None,
            rollout_path: rollout_file.path().to_path_buf(),
        }),
    };

    let expected = json!({
        "id": "1234",
        "msg": {
            "type": "session_configured",
            "session_id": "67e55044-10b1-426f-9247-bb680e5fe0c8",
            "model": "codex-mini-latest",
            "model_provider_id": "openai",
            "approval_policy": "never",
            "sandbox_policy": {
                "type": "read-only"
            },
            "cwd": "/home/user/project",
            "reasoning_effort": "medium",
            "history_log_id": 0,
            "history_entry_count": 0,
            "rollout_path": format!("{}", rollout_file.path().display()),
        }
    });
    assert_eq!(expected, serde_json::to_value(&event)?);
    Ok(())
}

#[test]
fn vec_u8_as_base64_serialization_and_deserialization() -> Result<()> {
    let event = ExecCommandOutputDeltaEvent {
        call_id: "call21".to_string(),
        stream: ExecOutputStream::Stdout,
        chunk: vec![1, 2, 3, 4, 5],
    };
    let serialized = serde_json::to_string(&event)?;
    assert_eq!(
        r#"{"call_id":"call21","stream":"stdout","chunk":"AQIDBAU="}"#,
        serialized,
    );

    let deserialized: ExecCommandOutputDeltaEvent = serde_json::from_str(&serialized)?;
    assert_eq!(deserialized, event);
    Ok(())
}

#[test]
fn serialize_mcp_startup_update_event() -> Result<()> {
    let event = Event {
        id: "init".to_string(),
        msg: EventMsg::McpStartupUpdate(McpStartupUpdateEvent {
            server: "srv".to_string(),
            status: McpStartupStatus::Failed {
                error: "boom".to_string(),
            },
        }),
    };

    let value = serde_json::to_value(&event)?;
    assert_eq!(value["msg"]["type"], "mcp_startup_update");
    assert_eq!(value["msg"]["server"], "srv");
    assert_eq!(value["msg"]["status"]["state"], "failed");
    assert_eq!(value["msg"]["status"]["error"], "boom");
    Ok(())
}

#[test]
fn serialize_mcp_startup_complete_event() -> Result<()> {
    let event = Event {
        id: "init".to_string(),
        msg: EventMsg::McpStartupComplete(McpStartupCompleteEvent {
            ready: vec!["a".to_string()],
            failed: vec![McpStartupFailure {
                server: "b".to_string(),
                error: "bad".to_string(),
            }],
            cancelled: vec!["c".to_string()],
        }),
    };

    let value = serde_json::to_value(&event)?;
    assert_eq!(value["msg"]["type"], "mcp_startup_complete");
    assert_eq!(value["msg"]["ready"][0], "a");
    assert_eq!(value["msg"]["failed"][0]["server"], "b");
    assert_eq!(value["msg"]["failed"][0]["error"], "bad");
    assert_eq!(value["msg"]["cancelled"][0], "c");
    Ok(())
}
