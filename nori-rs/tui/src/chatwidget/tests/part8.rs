use super::*;
use pretty_assertions::assert_eq;

fn acp_tool_snapshot(
    call_id: &str,
    title: &str,
    kind: nori_protocol::ToolKind,
    phase: nori_protocol::ToolPhase,
) -> nori_protocol::ToolSnapshot {
    nori_protocol::ToolSnapshot {
        call_id: call_id.to_string(),
        title: title.to_string(),
        kind,
        phase,
        locations: vec![],
        invocation: None,
        artifacts: vec![],
        raw_input: None,
        raw_output: None,
        owner_request_id: None,
    }
}

#[test]
fn acp_tool_snapshots_update_exit_stats_once_and_extract_skill() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(acp_tool_snapshot(
        "read-skill",
        "Read SKILL.md",
        nori_protocol::ToolKind::Read,
        nori_protocol::ToolPhase::Pending,
    )));

    let skill_path = PathBuf::from("/tmp/repro-skill/SKILL.md");
    let mut completed_read = acp_tool_snapshot(
        "read-skill",
        "Read SKILL.md",
        nori_protocol::ToolKind::Read,
        nori_protocol::ToolPhase::Completed,
    );
    completed_read.locations = vec![nori_protocol::ToolLocation {
        path: skill_path.clone(),
        line: None,
    }];
    completed_read.invocation = Some(nori_protocol::Invocation::Read {
        path: skill_path.clone(),
    });
    completed_read.raw_input = Some(serde_json::json!({
        "file_path": skill_path,
    }));
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(completed_read));

    let mut completed_execute = acp_tool_snapshot(
        "exec",
        "Run command",
        nori_protocol::ToolKind::Execute,
        nori_protocol::ToolPhase::Completed,
    );
    completed_execute.invocation = Some(nori_protocol::Invocation::Command {
        command: "printf done".to_string(),
    });
    completed_execute.raw_input = Some(serde_json::json!({
        "command": "printf done",
    }));
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(completed_execute));

    assert_eq!(chat.session_stats.tool_calls.get("read"), Some(&1));
    assert_eq!(chat.session_stats.tool_calls.get("execute"), Some(&1));
    assert_eq!(chat.session_stats.skills_used, vec!["repro-skill"]);
}

#[test]
fn acp_agent_snapshot_uses_title_for_generic_other_and_records_subagent() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    let mut snapshot = acp_tool_snapshot(
        "agent",
        "Agent",
        nori_protocol::ToolKind::Other("Other".to_string()),
        nori_protocol::ToolPhase::Completed,
    );
    snapshot.raw_input = Some(serde_json::json!({
        "subagent_type": "nori-task-runner",
        "prompt": "Inspect the implementation",
    }));

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(snapshot));

    assert_eq!(chat.session_stats.tool_calls.get("Agent"), Some(&1));
    assert_eq!(chat.session_stats.subagents_used, vec!["nori-task-runner"]);
}

#[test]
fn acp_prompt_completed_with_final_message_counts_assistant_once() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::PromptCompleted(
        nori_protocol::PromptCompleted {
            stop_reason: nori_protocol::StopReason::EndTurn,
            last_agent_message: Some("Done".to_string()),
        },
    ));

    assert_eq!(chat.session_stats.assistant_messages, 1);
}

#[test]
fn acp_streamed_answer_counts_assistant_once_when_prompt_completes() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::MessageDelta(
        nori_protocol::MessageDelta {
            stream: nori_protocol::MessageStream::Answer,
            delta: "Done".to_string(),
        },
    ));
    chat.handle_client_event(nori_protocol::ClientEvent::PromptCompleted(
        nori_protocol::PromptCompleted {
            stop_reason: nori_protocol::StopReason::EndTurn,
            last_agent_message: None,
        },
    ));

    assert_eq!(chat.session_stats.assistant_messages, 1);
}

#[test]
fn transcript_subagents_are_merged_into_exit_stats() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.apply_system_info_refresh(crate::system_info::SystemInfo {
        transcript_location: Some(nori_acp::TranscriptLocation {
            agent_kind: nori_acp::AgentKind::Codex,
            transcript_path: PathBuf::from("/tmp/session.jsonl"),
            session_id: "codex-session".to_string(),
            token_breakdown: None,
            subagents_used: vec!["nori-task-runner".to_string()],
        }),
        ..Default::default()
    });

    assert_eq!(chat.session_stats.subagents_used, vec!["nori-task-runner"]);
}
