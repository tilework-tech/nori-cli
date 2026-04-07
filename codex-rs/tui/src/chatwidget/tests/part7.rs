use super::*;

#[test]
fn cancelling_phase_keeps_ui_running_until_prompt_finished() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.handle_client_event(nori_protocol::ClientEvent::PhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Prompt,
    ));
    assert!(chat.bottom_pane.is_task_running());

    chat.handle_client_event(nori_protocol::ClientEvent::PhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Cancelling,
    ));

    assert!(
        chat.bottom_pane.is_task_running(),
        "Cancelling must keep the ACP UI in a running state until the prompt really finishes"
    );
}

#[test]
fn cancelled_prompt_finished_returns_to_idle_without_interrupt_restore() {
    let (mut chat, mut rx, mut op_rx) = make_chatwidget_manual();

    chat.bottom_pane
        .set_composer_text("keep my draft".to_string());
    chat.handle_client_event(nori_protocol::ClientEvent::QueuedPromptsUpdate(
        nori_protocol::QueuedPromptsUpdate {
            prompts: vec!["queued follow up".to_string()],
        },
    ));
    chat.handle_client_event(nori_protocol::ClientEvent::PhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Prompt,
    ));
    chat.handle_client_event(nori_protocol::ClientEvent::PhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Cancelling,
    ));

    chat.handle_client_event(nori_protocol::ClientEvent::PromptFinished(
        nori_protocol::PromptFinishedEvent {
            stop_reason: serde_json::from_str("\"cancelled\"").expect("valid stop reason"),
            last_agent_message: None,
        },
    ));
    chat.handle_client_event(nori_protocol::ClientEvent::PhaseChanged(
        nori_protocol::session_runtime::SessionPhaseView::Idle,
    ));

    assert!(
        !chat.bottom_pane.is_task_running(),
        "Idle should only arrive after prompt finished"
    );
    assert_eq!(
        chat.bottom_pane.composer_text(),
        "keep my draft",
        "ACP cancel should not merge backend queue entries into the composer"
    );
    assert!(
        op_rx.try_recv().is_err(),
        "ACP prompt completion should not auto-submit or restore queued prompts locally"
    );

    let rendered = render_bottom_popup(&chat, 80);
    assert!(
        rendered.contains("queued follow up"),
        "backend queue projection should remain the single visible queue source: {rendered}"
    );

    let _ = drain_insert_history(&mut rx);
}
