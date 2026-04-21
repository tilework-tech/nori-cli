//! Fork picker component for rewinding conversations.
//!
//! This module provides the UI for selecting a previous user message
//! to rewind the conversation to.

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

/// Maximum characters to show in a message preview in the picker.
const MAX_PREVIEW_CHARS: usize = 80;

/// Truncate a message to a single-line preview suitable for the picker.
fn truncate_preview(message: &str) -> String {
    let first_line = message.lines().next().unwrap_or("");
    if first_line.chars().count() > MAX_PREVIEW_CHARS {
        let truncated: String = first_line.chars().take(MAX_PREVIEW_CHARS).collect();
        format!("{truncated}…")
    } else if message.lines().count() > 1 {
        format!("{first_line}…")
    } else {
        first_line.to_string()
    }
}

/// Create selection view parameters for the fork picker.
///
/// # Arguments
/// * `messages` - List of `(cell_index, message_text)` tuples, ordered oldest-first
/// * `app_event_tx` - The app event sender for triggering fork events
///
/// Items are displayed newest-first (reversed from input order).
pub fn fork_picker_params(
    messages: Vec<(usize, String)>,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    let items: Vec<SelectionItem> = messages
        .into_iter()
        .rev()
        .map(|(cell_index, message)| {
            let preview = truncate_preview(&message);
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::ForkToMessage {
                    cell_index,
                    prefill: message.clone(),
                });
            })];
            SelectionItem {
                name: preview,
                description: None,
                is_current: false,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Fork Conversation".to_string()),
        subtitle: Some("Select a message to rewind to".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        is_searchable: false,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use tokio::sync::mpsc::unbounded_channel;

    fn make_tx() -> (
        AppEventSender,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        let (tx_raw, rx) = unbounded_channel::<AppEvent>();
        (AppEventSender::new(tx_raw), rx)
    }

    #[test]
    fn fork_picker_with_no_messages_returns_empty_items() {
        let (tx, _rx) = make_tx();
        let params = fork_picker_params(vec![], tx);
        assert!(params.items.is_empty());
    }

    #[test]
    fn fork_picker_has_correct_title_and_subtitle() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "Hello".to_string())];
        let params = fork_picker_params(messages, tx);

        assert_eq!(params.title.as_deref(), Some("Fork Conversation"));
        assert!(params.subtitle.is_some());
    }

    #[test]
    fn fork_picker_items_are_newest_first() {
        let (tx, _rx) = make_tx();
        let messages = vec![
            (0, "first message".to_string()),
            (1, "second message".to_string()),
            (2, "third message".to_string()),
        ];
        let params = fork_picker_params(messages, tx);

        assert_eq!(params.items.len(), 3);
        assert_eq!(params.items[0].name, "third message");
        assert_eq!(params.items[1].name, "second message");
        assert_eq!(params.items[2].name, "first message");
    }

    #[test]
    fn fork_picker_truncates_long_messages() {
        let (tx, _rx) = make_tx();
        let long_msg = "a".repeat(200);
        let messages = vec![(0, long_msg.clone())];
        let params = fork_picker_params(messages, tx);

        assert_eq!(params.items.len(), 1);
        assert!(params.items[0].name.len() < long_msg.len());
        assert!(params.items[0].name.ends_with('…'));
    }

    #[test]
    fn fork_picker_truncates_multiline_messages() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "first line\nsecond line\nthird line".to_string())];
        let params = fork_picker_params(messages, tx);

        assert_eq!(params.items.len(), 1);
        assert_eq!(params.items[0].name, "first line…");
    }

    #[test]
    fn fork_picker_action_fires_correct_event() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "first".to_string()), (1, "second".to_string())];
        let params = fork_picker_params(messages, tx);

        // Execute the action for the first item (newest-first, so index 0 = message 1)
        assert!(!params.items[0].actions.is_empty());
        let (verify_tx, mut verify_rx) = unbounded_channel::<AppEvent>();
        let verify_sender = AppEventSender::new(verify_tx);
        (params.items[0].actions[0])(&verify_sender);

        let event = verify_rx.try_recv().expect("should have received event");
        match event {
            AppEvent::ForkToMessage {
                cell_index,
                prefill,
            } => {
                assert_eq!(cell_index, 1);
                assert_eq!(prefill, "second");
            }
            other => panic!("expected ForkToMessage, got {other:?}"),
        }
    }

    #[test]
    fn fork_picker_action_for_oldest_message() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "first".to_string()), (1, "second".to_string())];
        let params = fork_picker_params(messages, tx);

        // Last item in picker = oldest message (index 0)
        let (verify_tx, mut verify_rx) = unbounded_channel::<AppEvent>();
        let verify_sender = AppEventSender::new(verify_tx);
        (params.items[1].actions[0])(&verify_sender);

        let event = verify_rx.try_recv().expect("should have received event");
        match event {
            AppEvent::ForkToMessage {
                cell_index,
                prefill,
            } => {
                assert_eq!(cell_index, 0);
                assert_eq!(prefill, "first");
            }
            other => panic!("expected ForkToMessage, got {other:?}"),
        }
    }
}
