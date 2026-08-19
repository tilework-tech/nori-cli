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

/// Label for the branch-at-head entry, always the first item in the picker.
const BRANCH_FROM_CURRENT_LABEL: &str = "⎇ Branch from current point";

/// Create selection view parameters for the fork picker.
///
/// # Arguments
/// * `messages` - List of `(cell_index, message_text)` tuples, ordered oldest-first
/// * `supports_fork` - Whether the agent advertises ACP `session/fork`; when
///   true the picker's first item branches at the current head
/// * `app_event_tx` - The app event sender for triggering fork events
///
/// When the agent supports `session/fork`, the first item branches at the
/// current head. The remaining items are the earlier user messages, displayed
/// newest-first (reversed from input order).
pub fn fork_picker_params(
    messages: Vec<(usize, String)>,
    supports_fork: bool,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    let mut items: Vec<SelectionItem> = Vec::new();
    if supports_fork {
        let branch_actions: Vec<SelectionAction> = vec![Box::new(|tx| {
            tx.send(AppEvent::BranchFromCurrent);
        })];
        items.push(SelectionItem {
            name: BRANCH_FROM_CURRENT_LABEL.to_string(),
            description: None,
            is_current: false,
            actions: branch_actions,
            dismiss_on_select: true,
            ..Default::default()
        });
    }
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

    let subtitle = if supports_fork {
        "Branch at the current point, or select a message to rewind to"
    } else {
        "Select a message to rewind to"
    };
    SelectionViewParams {
        title: Some("Fork Conversation".to_string()),
        subtitle: Some(subtitle.to_string()),
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
    use crate::bottom_pane::ListSelectionView;
    use crate::render::renderable::Renderable;
    use insta::assert_snapshot;
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use tokio::sync::mpsc::unbounded_channel;

    fn make_tx() -> (
        AppEventSender,
        tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    ) {
        let (tx_raw, rx) = unbounded_channel::<AppEvent>();
        (AppEventSender::new(tx_raw), rx)
    }

    fn render_lines(view: &ListSelectionView, width: u16) -> String {
        let height = view.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);
        (0..area.height)
            .map(|row| {
                (0..area.width)
                    .map(|col| {
                        let symbol = buf[(area.x + col, area.y + row)].symbol();
                        if symbol.is_empty() { " " } else { symbol }.to_string()
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn fork_picker_with_no_messages_still_offers_branch() {
        let (tx, _rx) = make_tx();
        let params = fork_picker_params(vec![], true, tx);
        assert_eq!(params.items.len(), 1);
        assert_eq!(params.items[0].name, BRANCH_FROM_CURRENT_LABEL);
    }

    #[test]
    fn fork_picker_without_fork_support_omits_branch_entry() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "only message".to_string())];
        let params = fork_picker_params(messages, false, tx);
        assert_eq!(params.items.len(), 1);
        assert_eq!(params.items[0].name, "only message");
    }

    #[test]
    fn fork_picker_branch_entry_is_first_even_with_messages() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "only message".to_string())];
        let params = fork_picker_params(messages, true, tx);
        assert_eq!(params.items.len(), 2);
        assert_eq!(params.items[0].name, BRANCH_FROM_CURRENT_LABEL);
        assert_eq!(params.items[1].name, "only message");
    }

    #[test]
    fn fork_picker_branch_action_fires_branch_from_current() {
        let (tx, _rx) = make_tx();
        let params = fork_picker_params(vec![], true, tx);

        assert!(!params.items[0].actions.is_empty());
        let (verify_tx, mut verify_rx) = unbounded_channel::<AppEvent>();
        let verify_sender = AppEventSender::new(verify_tx);
        (params.items[0].actions[0])(&verify_sender);

        let event = verify_rx.try_recv().expect("should have received event");
        assert!(
            matches!(event, AppEvent::BranchFromCurrent),
            "expected BranchFromCurrent, got {event:?}"
        );
    }

    #[test]
    fn fork_picker_has_correct_title_and_subtitle() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "Hello".to_string())];
        let params = fork_picker_params(messages, true, tx);

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
        let params = fork_picker_params(messages, true, tx);

        assert_eq!(params.items.len(), 4);
        assert_eq!(params.items[0].name, BRANCH_FROM_CURRENT_LABEL);
        assert_eq!(params.items[1].name, "third message");
        assert_eq!(params.items[2].name, "second message");
        assert_eq!(params.items[3].name, "first message");
    }

    #[test]
    fn fork_picker_truncates_long_messages() {
        let (tx, _rx) = make_tx();
        let long_msg = "a".repeat(200);
        let messages = vec![(0, long_msg.clone())];
        let params = fork_picker_params(messages, true, tx);

        assert_eq!(params.items.len(), 2);
        assert!(params.items[1].name.len() < long_msg.len());
        assert!(params.items[1].name.ends_with('…'));
    }

    #[test]
    fn fork_picker_truncates_multiline_messages() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "first line\nsecond line\nthird line".to_string())];
        let params = fork_picker_params(messages, true, tx);

        assert_eq!(params.items.len(), 2);
        assert_eq!(params.items[1].name, "first line…");
    }

    #[test]
    fn fork_picker_action_fires_correct_event() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "first".to_string()), (1, "second".to_string())];
        let params = fork_picker_params(messages, true, tx);

        // Item 0 is the branch entry; item 1 is the newest message (index 1).
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
        let params = fork_picker_params(messages, true, tx);

        // Last item in picker = oldest message (index 0).
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

    #[test]
    fn fork_picker_renders_branch_entry() {
        let (tx, _rx) = make_tx();
        let messages = vec![(0, "an earlier message".to_string())];
        let params = fork_picker_params(messages, true, tx.clone());
        let view = ListSelectionView::new(params, tx);
        assert_snapshot!("fork_picker_with_branch_entry", render_lines(&view, 60));
    }
}
