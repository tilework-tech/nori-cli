//! Fork picker for the `/fork` command.
//!
//! Builds a selection view listing the current session's user messages so the
//! user can choose a fork point. Forking happens just before the selected
//! message (same semantics as backtrack).

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::text_formatting::truncate_text;

/// Build selection view parameters for the fork picker.
///
/// `user_messages` is a list of `(nth_user_message_index, message_text)` pairs
/// in chronological order.
pub(crate) fn fork_picker_params(
    user_messages: Vec<(usize, String)>,
    app_event_tx: AppEventSender,
) -> SelectionViewParams {
    if user_messages.is_empty() {
        return SelectionViewParams {
            title: Some("Fork conversation".to_string()),
            subtitle: Some("No user messages to fork from".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items: vec![],
            ..Default::default()
        };
    }

    let items: Vec<SelectionItem> = user_messages
        .into_iter()
        .map(|(nth, message)| {
            let turn_label = nth + 1;
            let preview = truncate_text(&message.replace('\n', " "), 60);
            let name = format!("Turn {turn_label}: {preview}");

            let tx = app_event_tx.clone();
            let actions: Vec<SelectionAction> = vec![Box::new(move |_: &AppEventSender| {
                tx.send(AppEvent::ForkAtMessage {
                    nth_user_message: nth,
                });
            })];

            SelectionItem {
                name,
                search_value: Some(message),
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Fork conversation".to_string()),
        subtitle: Some("Select a message to fork before".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        is_searchable: true,
        search_placeholder: Some("Type to search messages".to_string()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_picker_builds_items_from_messages() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let app_event_tx = AppEventSender::new(tx);

        let messages = vec![
            (0, "Hello world".to_string()),
            (1, "Fix the bug in main.rs".to_string()),
            (2, "Add tests for the new feature".to_string()),
        ];

        let params = fork_picker_params(messages, app_event_tx);

        assert_eq!(params.items.len(), 3);
        assert!(params.items[0].name.starts_with("Turn 1:"));
        assert!(params.items[0].name.contains("Hello world"));
        assert!(params.items[1].name.starts_with("Turn 2:"));
        assert!(params.items[2].name.starts_with("Turn 3:"));
        assert!(params.is_searchable);
        assert_eq!(params.title.as_deref(), Some("Fork conversation"));
        assert_eq!(
            params.subtitle.as_deref(),
            Some("Select a message to fork before")
        );
    }

    #[test]
    fn fork_picker_empty_messages_shows_no_items() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let app_event_tx = AppEventSender::new(tx);

        let params = fork_picker_params(vec![], app_event_tx);

        assert!(params.items.is_empty());
        assert_eq!(
            params.subtitle.as_deref(),
            Some("No user messages to fork from")
        );
    }

    #[test]
    fn fork_picker_multiline_message_collapsed_to_single_line() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let app_event_tx = AppEventSender::new(tx);

        let messages = vec![(0, "line one\nline two\nline three".to_string())];

        let params = fork_picker_params(messages, app_event_tx);

        assert_eq!(params.items.len(), 1);
        // Newlines should be replaced with spaces
        assert!(!params.items[0].name.contains('\n'));
        assert!(params.items[0].name.contains("line one line two"));
    }
}
