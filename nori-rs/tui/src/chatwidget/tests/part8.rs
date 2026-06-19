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
            failure: None,
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
            failure: None,
        },
    ));

    assert_eq!(chat.session_stats.assistant_messages, 1);
}

/// A long-running tool keeps sending ToolCallUpdate notifications while the
/// agent streams its answer. Those no-op snapshot updates (the tool cell was
/// already flushed to history) must not finalize the answer stream, otherwise
/// one assistant message fragments into many `•` cells.
#[test]
fn noop_tool_updates_do_not_fragment_streaming_answer() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // A long-running Execute tool call becomes the active cell.
    let running_tool = || {
        let mut snapshot = acp_tool_snapshot(
            "clippy",
            "cargo clippy",
            nori_protocol::ToolKind::Execute,
            nori_protocol::ToolPhase::InProgress,
        );
        snapshot.invocation = Some(nori_protocol::Invocation::Command {
            command: "cargo clippy".to_string(),
        });
        snapshot
    };
    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(running_tool()));

    // The agent streams its answer while the tool is still running; progress
    // updates for the running tool interleave with the answer deltas.
    let deltas = [
        "Scoped clippy fix is still runn",
        "ing; waiting for it to fin",
        "ish.",
    ];
    for delta in deltas {
        chat.handle_client_event(nori_protocol::ClientEvent::MessageDelta(
            nori_protocol::MessageDelta {
                stream: nori_protocol::MessageStream::Answer,
                delta: delta.to_string(),
            },
        ));
        chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(running_tool()));
    }

    chat.handle_client_event(nori_protocol::ClientEvent::PromptCompleted(
        nori_protocol::PromptCompleted {
            stop_reason: nori_protocol::StopReason::EndTurn,
            last_agent_message: None,
            failure: None,
        },
    ));

    let rendered = drain_insert_history(&mut rx)
        .into_iter()
        .map(|lines| lines_to_single_string(&lines))
        .collect::<Vec<_>>()
        .join("");

    assert_eq!(
        rendered
            .matches("• Scoped clippy fix is still running; waiting for it to finish.")
            .count(),
        1,
        "{rendered}"
    );
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
