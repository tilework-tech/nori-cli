use super::*;

/// Deliver a prompt completion through the real client-event entry point.
fn deliver_completion(chat: &mut ChatWidget, failure: Option<crate::presentation::TurnFailure>) {
    chat.handle_client_event(crate::presentation::ClientEvent::PromptCompleted(
        crate::presentation::PromptCompleted {
            stop_reason: nori_protocol::acp::v1::StopReason::Cancelled,
            last_agent_message: None,
            failure,
        },
    ));
}

fn history_text(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    drain_insert_history(rx)
        .iter()
        .map(|cell| lines_to_single_string(cell))
        .collect()
}

/// Scan the app-event stream for the next loop re-fire, if any.
fn next_loop_iteration(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Option<(i32, i32)> {
    while let Ok(ev) = rx.try_recv() {
        if let AppEvent::LoopIteration {
            remaining, total, ..
        } = ev
        {
            return Some((remaining, total));
        }
    }
    None
}

/// A transient (retryable) turn failure must leave the loop armed: the next
/// iteration fires when the turn completes.
#[test]
fn loop_survives_retryable_failure() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);

    deliver_completion(&mut chat, Some(crate::presentation::TurnFailure::Retryable));

    assert_eq!(next_loop_iteration(&mut rx), Some((4, 10)));
}

/// A fatal turn failure disarms the loop before it can re-fire.
#[test]
fn loop_stops_on_fatal_failure() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);

    deliver_completion(&mut chat, Some(crate::presentation::TurnFailure::Fatal));

    assert_eq!(chat.loop_remaining, None);
    assert_eq!(next_loop_iteration(&mut rx), None);
}

/// A turn that ended in a failure must not also render the generic
/// "Conversation interrupted" cell — the surfaced error already explains it.
#[test]
fn failure_completion_does_not_add_interrupted_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    deliver_completion(&mut chat, Some(crate::presentation::TurnFailure::Fatal));

    assert!(
        !history_text(&mut rx).contains("Conversation interrupted"),
        "a failure completion should not add the interrupted cell"
    );
}

/// A genuine user cancellation (no failure) still renders the interrupted cell.
#[test]
fn user_cancellation_adds_interrupted_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    deliver_completion(&mut chat, None);

    assert!(
        history_text(&mut rx).contains("Conversation interrupted"),
        "a clean user cancellation should add the interrupted cell"
    );
}
