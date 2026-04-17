use super::*;

#[test]
fn cancelling_phase_keeps_task_running_until_prompt_completed() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Prompt,
    ));
    drain_insert_history(&mut rx);

    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Cancelling,
    ));

    assert!(chat.bottom_pane.is_task_running());

    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Idle,
    ));
    chat.handle_client_event(nori_protocol::ClientEvent::PromptCompleted(
        nori_protocol::PromptCompleted {
            stop_reason: nori_protocol::StopReason::Cancelled,
            last_agent_message: None,
        },
    ));
    drain_insert_history(&mut rx);

    assert!(!chat.bottom_pane.is_task_running());
}

#[test]
fn queue_projection_submission_during_cancelling_still_sends_user_input() {
    let (mut chat, _rx, mut op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Prompt,
    ));
    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Cancelling,
    ));

    chat.submit_user_message(UserMessage::from("queued follow up"));

    assert!(matches!(op_rx.try_recv(), Ok(Op::UserInput { .. })));
}

#[test]
fn idle_phase_unknown_tool_snapshot_still_renders_visible_history_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::SessionPhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Idle,
    ));
    drain_insert_history(&mut rx);

    chat.handle_client_event(nori_protocol::ClientEvent::ToolSnapshot(
        nori_protocol::ToolSnapshot {
            call_id: "call-exec-idle".into(),
            title: "Terminal".into(),
            kind: nori_protocol::ToolKind::Execute,
            phase: nori_protocol::ToolPhase::Completed,
            locations: vec![],
            invocation: Some(nori_protocol::Invocation::Command {
                command: "git status".into(),
            }),
            artifacts: vec![nori_protocol::Artifact::Text {
                text: "On branch spec\n".into(),
            }],
            raw_input: Some(serde_json::json!({
                "command": "git status",
            })),
            raw_output: Some(serde_json::json!({
                "stdout": "On branch spec\n",
            })),
            owner_request_id: None,
        },
    ));

    let cells = drain_insert_history(&mut rx);
    assert_eq!(cells.len(), 1, "expected visible ACP tool history cell");
    let blob = lines_to_single_string(cells.first().unwrap());
    assert!(
        blob.contains("git status"),
        "expected command in cell: {blob:?}"
    );
    assert!(
        blob.contains("On branch spec"),
        "expected output in cell: {blob:?}"
    );
}
