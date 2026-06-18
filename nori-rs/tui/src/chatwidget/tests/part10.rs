use super::*;
use codex_core::protocol::ErrorEvent;

/// Scan the app-event stream for the next loop re-fire, if any.
#[cfg(feature = "nori-config")]
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

/// Deliver an error to the widget through the real event-dispatch entry point,
/// exercising the same path the ACP backend uses.
#[cfg(feature = "nori-config")]
fn deliver_error(chat: &mut ChatWidget, message: &str, retryable: bool) {
    chat.handle_codex_event(Event {
        id: String::new(),
        msg: EventMsg::Error(ErrorEvent {
            message: message.to_string(),
            retryable,
        }),
    });
}

/// A transient (retryable) error during a loop iteration must not disarm the
/// loop: when the turn completes, the next iteration still fires.
#[cfg(feature = "nori-config")]
#[test]
fn loop_survives_retryable_error() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);

    // The message text is display-only; `retryable` is what drives the loop.
    deliver_error(&mut chat, "server overloaded", true);

    assert_eq!(chat.loop_remaining, Some(5));

    chat.on_task_complete(None);

    assert_eq!(next_loop_iteration(&mut rx), Some((4, 10)));
}

/// A fatal (non-retryable) error stops the loop entirely: it is disarmed and no
/// further iterations fire.
#[cfg(feature = "nori-config")]
#[test]
fn loop_stops_on_fatal_error() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);

    deliver_error(&mut chat, "Authentication error: invalid API key.", false);

    assert_eq!(chat.loop_remaining, None);

    chat.on_task_complete(None);

    assert_eq!(next_loop_iteration(&mut rx), None);
}
