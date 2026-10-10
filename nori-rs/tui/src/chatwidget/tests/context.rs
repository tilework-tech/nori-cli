use super::*;
use crate::presentation::session_runtime::SessionUsageState;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "current_thread")]
async fn local_context_command_returns_promptly_and_loads_instruction_inventory() {
    let dir = tempfile::tempdir().expect("instruction directory");
    std::fs::write(
        dir.path().join("CLAUDE.md"),
        "Use the shared component library.\n",
    )
    .expect("write instructions");
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.config.active_agent = "claude-code".into();
    chat.config.cwd = dir.path().to_path_buf();
    chat.effective_cwd_tracker = Default::default();
    while rx.try_recv().is_ok() {}

    let started = std::time::Instant::now();
    chat.dispatch_command(SlashCommand::Context);
    assert!(
        started.elapsed() < std::time::Duration::from_millis(100),
        "context inspection blocked input for {:?}",
        started.elapsed()
    );

    let output = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            match rx.recv().await.expect("context event channel") {
                AppEvent::ContextTimelineReady {
                    generation,
                    conversation_id,
                    result,
                } => {
                    chat.handle_context_timeline(generation, conversation_id, result);
                }
                AppEvent::InsertHistoryCell(cell) => {
                    break lines_to_single_string(&cell.display_lines(160));
                }
                _ => {}
            }
        }
    })
    .await
    .expect("context snapshot produced");
    let expected_path =
        crate::nori::session_header::format_instruction_path(&dir.path().join("CLAUDE.md"));
    assert!(
        output.contains(&expected_path),
        "instruction inventory: {output}"
    );
    assert!(
        output.contains("Local instructions"),
        "instruction inventory: {output}"
    );
}

#[test]
fn context_command_uses_reported_usage_and_prints_a_detached_snapshot() {
    let (mut chat, mut rx, _op_rx) = make_cloud_chatwidget_manual();
    chat.bottom_pane.set_session_usage(Some(SessionUsageState {
        used_tokens: 340_412,
        total_tokens: 1_000_000,
        cost_display: None,
    }));
    while rx.try_recv().is_ok() {}
    chat.dispatch_command(SlashCommand::Context);
    let mut cells = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            cells.push(cell);
        }
    }
    assert_eq!(cells.len(), 1);
    let before = lines_to_single_string(&cells[0].display_lines(100));
    assert!(before.contains("340,412 / 1,000,000 tokens"));
    let stacked = lines_to_single_string(&cells[0].display_lines(80));
    assert!(stacked.contains("Remote instruction files are not available locally."));
    chat.bottom_pane.set_session_usage(Some(SessionUsageState {
        used_tokens: 0,
        total_tokens: 1_000_000,
        cost_display: None,
    }));
    assert_eq!(before, lines_to_single_string(&cells[0].display_lines(100)));
    assert_eq!(chat.bottom_pane.context_usage_snapshot().0.tokens, Some(0));
    assert!(SlashCommand::Context.available_during_task());
}

#[test]
fn cumulative_billing_tokens_are_not_context_occupancy() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.bottom_pane
        .set_system_info(crate::system_info::SystemInfo {
            transcript_location: Some(nori_harness::TranscriptLocation {
                agent_kind: nori_harness::AgentKind::ClaudeCode,
                transcript_path: PathBuf::from("/tmp/context-test.jsonl"),
                session_id: "context-test".into(),
                token_breakdown: Some(nori_harness::TranscriptTokenUsage {
                    input_tokens: 800_000,
                    output_tokens: 50_000,
                    cached_tokens: 700_000,
                    last_context_tokens: None,
                }),
                subagents_used: Vec::new(),
            }),
            ..Default::default()
        });
    assert_eq!(chat.bottom_pane.context_usage_snapshot().0.tokens, None);
}

#[test]
fn context_query_results_from_another_session_are_discarded() {
    let (mut chat, mut rx, _op_rx) = make_cloud_chatwidget_manual();
    while rx.try_recv().is_ok() {}
    let stale = chat.session_generation() + 1;
    chat.handle_context_timeline(stale, chat.conversation_id, Ok(Default::default()));
    chat.handle_context_timeline(
        stale,
        chat.conversation_id,
        Err("old agent disconnected".into()),
    );
    assert!(
        rx.try_recv().is_err(),
        "stale query must emit neither a snapshot nor an error"
    );
}

#[test]
fn current_context_query_failure_is_visible() {
    let (mut chat, mut rx, _op_rx) = make_cloud_chatwidget_manual();
    while rx.try_recv().is_ok() {}
    chat.handle_context_timeline(
        chat.session_generation(),
        chat.conversation_id,
        Err("agent disconnected".into()),
    );
    let mut text = String::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            text.push_str(&lines_to_single_string(&cell.display_lines(100)));
        }
    }
    assert!(text.contains("agent disconnected"));
}

#[test]
fn context_query_results_from_the_parent_are_discarded_after_a_fork() {
    let (mut chat, mut rx, _op_rx) = make_cloud_chatwidget_manual();
    let parent =
        nori_harness::ConversationId::from_string("11111111-1111-1111-1111-111111111111").unwrap();
    chat.conversation_id = Some(parent);
    let generation = chat.session_generation();
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionForked(
            nori_protocol::SessionForked {
                previous_conversation_id: parent.to_string(),
                new_conversation_id: "22222222-2222-2222-2222-222222222222".into(),
                new_acp_session_id: "forked".into(),
            },
        )),
    );
    while rx.try_recv().is_ok() {}
    chat.handle_context_timeline(generation, Some(parent), Ok(Default::default()));
    chat.handle_context_timeline(generation, Some(parent), Err("parent stopped".into()));
    assert!(rx.try_recv().is_err());
}
