use super::*;
use crate::test_backend::VT100Backend;
use pretty_assertions::assert_eq;
use ratatui::Terminal;

fn acp_tool_snapshot(
    call_id: &str,
    title: &str,
    kind: crate::presentation::ToolKind,
    phase: crate::presentation::ToolPhase,
) -> crate::presentation::ToolSnapshot {
    crate::presentation::ToolSnapshot {
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

    chat.handle_client_event(crate::presentation::ClientEvent::ToolSnapshot(
        acp_tool_snapshot(
            "read-skill",
            "Read SKILL.md",
            crate::presentation::ToolKind::Read,
            crate::presentation::ToolPhase::Pending,
        ),
    ));

    let skill_path = PathBuf::from("/tmp/repro-skill/SKILL.md");
    let mut completed_read = acp_tool_snapshot(
        "read-skill",
        "Read SKILL.md",
        crate::presentation::ToolKind::Read,
        crate::presentation::ToolPhase::Completed,
    );
    completed_read.locations = vec![crate::presentation::ToolLocation {
        path: skill_path.clone(),
        line: None,
    }];
    completed_read.invocation = Some(crate::presentation::Invocation::Read {
        path: skill_path.clone(),
    });
    completed_read.raw_input = Some(serde_json::json!({
        "file_path": skill_path,
    }));
    chat.handle_client_event(crate::presentation::ClientEvent::ToolSnapshot(
        completed_read,
    ));

    let mut completed_execute = acp_tool_snapshot(
        "exec",
        "Run command",
        crate::presentation::ToolKind::Execute,
        crate::presentation::ToolPhase::Completed,
    );
    completed_execute.invocation = Some(crate::presentation::Invocation::Command {
        command: "printf done".to_string(),
    });
    completed_execute.raw_input = Some(serde_json::json!({
        "command": "printf done",
    }));
    chat.handle_client_event(crate::presentation::ClientEvent::ToolSnapshot(
        completed_execute,
    ));

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
        crate::presentation::ToolKind::Other("Other".to_string()),
        crate::presentation::ToolPhase::Completed,
    );
    snapshot.raw_input = Some(serde_json::json!({
        "subagent_type": "nori-task-runner",
        "prompt": "Inspect the implementation",
    }));

    chat.handle_client_event(crate::presentation::ClientEvent::ToolSnapshot(snapshot));

    assert_eq!(chat.session_stats.tool_calls.get("Agent"), Some(&1));
    assert_eq!(chat.session_stats.subagents_used, vec!["nori-task-runner"]);
}

#[test]
fn acp_prompt_completed_with_final_message_counts_assistant_once() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(crate::presentation::ClientEvent::PromptCompleted(
        crate::presentation::PromptCompleted {
            stop_reason: nori_protocol::acp::v1::StopReason::EndTurn,
            last_agent_message: Some("Done".to_string()),
            failure: None,
        },
    ));

    assert_eq!(chat.session_stats.assistant_messages, 1);
}

#[test]
fn acp_streamed_answer_counts_assistant_once_when_prompt_completes() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(crate::presentation::ClientEvent::MessageDelta(
        crate::presentation::MessageDelta {
            stream: crate::presentation::MessageStream::Answer,
            message_id: None,
            delta: "Done".to_string(),
        },
    ));
    chat.handle_client_event(crate::presentation::ClientEvent::PromptCompleted(
        crate::presentation::PromptCompleted {
            stop_reason: nori_protocol::acp::v1::StopReason::EndTurn,
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
            crate::presentation::ToolKind::Execute,
            crate::presentation::ToolPhase::InProgress,
        );
        snapshot.invocation = Some(crate::presentation::Invocation::Command {
            command: "cargo clippy".to_string(),
        });
        snapshot
    };
    chat.handle_client_event(crate::presentation::ClientEvent::ToolSnapshot(
        running_tool(),
    ));

    // The agent streams its answer while the tool is still running; progress
    // updates for the running tool interleave with the answer deltas.
    let deltas = [
        "Scoped clippy fix is still runn",
        "ing; waiting for it to fin",
        "ish.",
    ];
    for delta in deltas {
        chat.handle_client_event(crate::presentation::ClientEvent::MessageDelta(
            crate::presentation::MessageDelta {
                stream: crate::presentation::MessageStream::Answer,
                message_id: None,
                delta: delta.to_string(),
            },
        ));
        chat.handle_client_event(crate::presentation::ClientEvent::ToolSnapshot(
            running_tool(),
        ));
    }

    chat.handle_client_event(crate::presentation::ClientEvent::PromptCompleted(
        crate::presentation::PromptCompleted {
            stop_reason: nori_protocol::acp::v1::StopReason::EndTurn,
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
        transcript_location: Some(nori_harness::TranscriptLocation {
            agent_kind: nori_harness::AgentKind::Codex,
            transcript_path: PathBuf::from("/tmp/session.jsonl"),
            session_id: "codex-session".to_string(),
            token_breakdown: None,
            subagents_used: vec!["nori-task-runner".to_string()],
        }),
        ..Default::default()
    });

    assert_eq!(chat.session_stats.subagents_used, vec!["nori-task-runner"]);
}

#[test]
fn session_usage_updates_footer_and_disables_transcript_fallback() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.apply_system_info_refresh(crate::system_info::SystemInfo {
        transcript_location: Some(nori_harness::TranscriptLocation {
            agent_kind: nori_harness::AgentKind::Codex,
            transcript_path: PathBuf::from("/tmp/codex-transcript.jsonl"),
            session_id: "codex-session".to_string(),
            token_breakdown: Some(nori_harness::TranscriptTokenUsage {
                input_tokens: 995_726,
                output_tokens: 8_452,
                cached_tokens: 500_000,
                last_context_tokens: Some(69_246),
            }),
            subagents_used: Vec::new(),
        }),
        ..Default::default()
    });
    chat.handle_client_event(crate::presentation::ClientEvent::SessionUpdateInfo(
        crate::presentation::SessionUpdateInfo {
            kind: crate::presentation::SessionUpdateKind::Usage,
            message: "Session usage: 42600 / 258400 tokens".into(),
            hint: None,
            usage: Some(crate::presentation::session_runtime::SessionUsageState {
                used_tokens: 42_600,
                total_tokens: 258_400,
                cost_display: None,
            }),
            session_info_patch: None,
        },
    ));

    assert!(drain_insert_history(&mut rx).is_empty());

    let height = chat.desired_height(80);
    let mut terminal = Terminal::new(VT100Backend::new(80, height)).expect("create terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 80, height));
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw chat with footer usage");
    let contents = terminal.backend().vt100().screen().contents();

    assert!(
        contents.contains("16% / 258k"),
        "expected ACP session usage in footer, got: {contents:?}"
    );
}

#[test]
fn transcript_usage_supplies_default_context_percentage_and_window_size() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.apply_system_info_refresh(crate::system_info::SystemInfo {
        transcript_location: Some(nori_harness::TranscriptLocation {
            agent_kind: nori_harness::AgentKind::Codex,
            transcript_path: PathBuf::from("/tmp/codex-transcript.jsonl"),
            session_id: "codex-session".to_string(),
            token_breakdown: Some(nori_harness::TranscriptTokenUsage {
                input_tokens: 69_246,
                output_tokens: 1_200,
                cached_tokens: 45_000,
                last_context_tokens: Some(69_246),
            }),
            subagents_used: Vec::new(),
        }),
        ..Default::default()
    });

    let height = chat.desired_height(80);
    let mut terminal = Terminal::new(VT100Backend::new(80, height)).expect("create terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 80, height));
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw chat with transcript context usage");
    let contents = terminal.backend().vt100().screen().contents();

    assert!(
        contents.contains("27% / 258k"),
        "expected transcript usage and agent window size in footer: {contents:?}"
    );
}

#[test]
fn custom_footer_formats_can_compose_context_values_in_footer_and_corner() {
    let config: nori_config::NoriConfigToml = toml::from_str(
        r#"
[tui.footer_layout]
footer_left = [
    { format = "{context_used_percent} / {context_window_tokens}" },
]
textarea_top_right = [
    { format = "{context_remaining_percent} remaining" },
]
"#,
    )
    .expect("custom footer layout should parse");
    let layout = nori_config::FooterLayoutConfig::from_toml(&config.tui.footer_layout);
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual_with_footer_layout(layout);

    chat.handle_client_event(crate::presentation::ClientEvent::SessionUpdateInfo(
        crate::presentation::SessionUpdateInfo {
            kind: crate::presentation::SessionUpdateKind::Usage,
            message: "Session usage: 42600 / 258400 tokens".into(),
            hint: None,
            usage: Some(crate::presentation::session_runtime::SessionUsageState {
                used_tokens: 42_600,
                total_tokens: 258_400,
                cost_display: None,
            }),
            session_info_patch: None,
        },
    ));
    assert!(drain_insert_history(&mut rx).is_empty());

    let height = chat.desired_height(80);
    let mut terminal = Terminal::new(VT100Backend::new(80, height)).expect("create terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 80, height));
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw chat with custom footer layout");
    let contents = terminal.backend().vt100().screen().contents();

    assert!(
        contents.contains("16% / 258k"),
        "custom footer should compose used percentage and window size: {contents:?}"
    );
    assert!(
        contents.contains("84% remaining"),
        "custom corner should compose remaining percentage: {contents:?}"
    );
}

#[test]
fn all_atomic_context_segments_render_session_usage_values() {
    let config: nori_config::NoriConfigToml = toml::from_str(
        r#"
[tui.footer_segments]
context = false
token_usage = false
context_used_percent = true
context_remaining_percent = true
context_used_tokens = true
context_remaining_tokens = true
context_window_tokens = true

[tui.footer_layout]
footer_left = [
    "context_used_percent",
    "context_remaining_percent",
    "context_used_tokens",
    "context_remaining_tokens",
    "context_window_tokens",
]
"#,
    )
    .expect("atomic context segments should parse");
    let segments = nori_config::FooterSegmentConfig::from_toml(&config.tui.footer_segments);
    let layout = nori_config::FooterLayoutConfig::from_toml(&config.tui.footer_layout);
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual_with_footer_config(segments, layout);

    chat.handle_client_event(crate::presentation::ClientEvent::SessionUpdateInfo(
        crate::presentation::SessionUpdateInfo {
            kind: crate::presentation::SessionUpdateKind::Usage,
            message: "Session usage: 42600 / 258400 tokens".into(),
            hint: None,
            usage: Some(crate::presentation::session_runtime::SessionUsageState {
                used_tokens: 42_600,
                total_tokens: 258_400,
                cost_display: None,
            }),
            session_info_patch: None,
        },
    ));
    assert!(drain_insert_history(&mut rx).is_empty());

    let height = chat.desired_height(80);
    let mut terminal = Terminal::new(VT100Backend::new(80, height)).expect("create terminal");
    terminal.set_viewport_area(Rect::new(0, 0, 80, height));
    terminal
        .draw(|f| chat.render(f.area(), f.buffer_mut()))
        .expect("draw chat with atomic context segments");
    let contents = terminal.backend().vt100().screen().contents();

    assert!(
        contents.contains("16% · 84% · 42.6k · 216k · 258k"),
        "expected every atomic context value in the footer: {contents:?}"
    );
}
