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
