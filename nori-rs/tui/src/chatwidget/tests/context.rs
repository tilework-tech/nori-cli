use super::*;
use crate::presentation::session_runtime::SessionUsageState;
use pretty_assertions::assert_eq;

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
