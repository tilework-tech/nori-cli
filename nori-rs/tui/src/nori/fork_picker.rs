//! Fork picker component for branching and rewinding conversations.
//!
//! This module provides the UI for either branching the conversation at its
//! current point (native ACP `session/fork`) or selecting a previous user
//! message to rewind to (local summary-based fork).

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
    // First entry: branch at the current point via the agent's native
    // `session/fork` (no rewind). Earlier messages rewind locally instead.
    let branch_actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
        tx.send(AppEvent::CodexOp(codex_core::protocol::Op::BranchSession));
    })];
    let mut items = vec![SelectionItem {
        name: "⎇ Branch from current point".to_string(),
        description: Some("duplicate this conversation and continue; the original session is preserved (requires agent support)".to_string()),
        is_current: false,
        actions: branch_actions,
        dismiss_on_select: true,
        ..Default::default()
    }];

    items.extend(messages.into_iter().rev().map(|(cell_index, message)| {
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
    }));

    SelectionViewParams {
        title: Some("Fork Conversation".to_string()),
        subtitle: Some(
            "Branch from the current point, or select a message to rewind to".to_string(),
        ),
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
    fn fork_picker_renders_branch_entry_and_messages() {
        use crate::render::renderable::Renderable;
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;

        let (tx, _rx) = make_tx();
        let params = fork_picker_params(
            vec![
                (0, "refactor the parser".to_string()),
                (1, "now add tests".to_string()),
            ],
            tx.clone(),
        );
        let view = crate::bottom_pane::ListSelectionView::new(params, tx);

        let width = 64;
        let height = view.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        let rendered = (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|col| {
                        let symbol = buf[(col, row)].symbol();
                        if symbol.is_empty() { " " } else { symbol }
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!("fork_picker_with_branch_entry", rendered);
    }

    #[test]
    fn fork_picker_with_no_messages_offers_only_branch_entry() {
        let (tx, _rx) = make_tx();
        let params = fork_picker_params(vec![], tx);
        assert_eq!(params.items.len(), 1);
        assert_eq!(params.items[0].name, "⎇ Branch from current point");
    }

    #[test]
    fn fork_picker_branch_entry_fires_branch_session_op() {
        let (tx, _rx) = make_tx();
        let params = fork_picker_params(vec![(0, "hello".to_string())], tx);

        let (verify_tx, mut verify_rx) = unbounded_channel::<AppEvent>();
        let verify_sender = AppEventSender::new(verify_tx);
        (params.items[0].actions[0])(&verify_sender);

        let event = verify_rx.try_recv().expect("should have received event");
        match event {
            AppEvent::CodexOp(codex_core::protocol::Op::BranchSession) => {}
            other => panic!("expected CodexOp(BranchSession), got {other:?}"),
        }
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

        assert_eq!(params.items.len(), 4);
        assert_eq!(params.items[0].name, "⎇ Branch from current point");
        assert_eq!(params.items[1].name, "third message");
        assert_eq!(params.items[2].name, "second message");
        assert_eq!(params.items[3].name, "first message");
    }

    #[test]
    fn fork_picker_truncates_long_messages() {
        let (tx, _rx) = make_tx();
        let long_msg = "a".repeat(200);
        let messages = vec![(0, long_msg.clone())];
        let params = fork_picker_params(messages, tx);

        assert_eq!(params.items.len(), 2);
        assert!(params.items[1].name.len() < long_msg.len());
        assert!(params.items[1].name.ends_with('…'));
    }

    #[test]
    fn fork_picker_truncates_multiline_messages() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "first line\nsecond line\nthird line".to_string())];
        let params = fork_picker_params(messages, tx);

        assert_eq!(params.items.len(), 2);
        assert_eq!(params.items[1].name, "first line…");
    }

    #[test]
    fn fork_picker_action_fires_correct_event() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "first".to_string()), (1, "second".to_string())];
        let params = fork_picker_params(messages, tx);

        // Execute the action for the first message item (item 0 is the branch
        // entry; messages are newest-first, so index 1 = message 1)
        assert!(!params.items[1].actions.is_empty());
        let (verify_tx, mut verify_rx) = unbounded_channel::<AppEvent>();
        let verify_sender = AppEventSender::new(verify_tx);
        (params.items[1].actions[0])(&verify_sender);

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
        (params.items[2].actions[0])(&verify_sender);

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
