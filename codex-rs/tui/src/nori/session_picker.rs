//! Session picker component for viewing previous session transcripts.
//!
//! This module provides the UI for selecting from available session transcripts
//! in the current project. Selecting a session loads its transcript for view-only
//! display in the TranscriptOverlay.

use chrono::DateTime;
use chrono::Utc;
use codex_acp::SessionSummary;
use ratatui::text::Line;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

/// Format a Unix timestamp as a human-readable date/time string.
fn format_timestamp(ts: u64) -> String {
    DateTime::<Utc>::from_timestamp(ts as i64, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// Create selection view parameters for the session picker.
///
/// Shows available sessions with timestamp, first message preview, and message count.
/// Selecting a session triggers loading its transcript for view-only display.
///
/// # Arguments
/// * `sessions` - List of available session summaries
/// * `project_key` - The project key for loading transcripts
/// * `app_event_tx` - The app event sender for triggering selection events
pub fn session_picker_params(
    sessions: Vec<SessionSummary>,
    project_key: String,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    if sessions.is_empty() {
        return SelectionViewParams {
            title: Some("Previous Sessions".to_string()),
            subtitle: Some("No previous sessions found for this project".to_string()),
            footer_hint: Some(Line::from("Press esc to dismiss.")),
            items: vec![SelectionItem {
                name: "No previous sessions".to_string(),
                description: Some(
                    "Start a conversation and use /new to create a new session".to_string(),
                ),
                is_current: false,
                actions: vec![],
                dismiss_on_select: true,
                ..Default::default()
            }],
            ..Default::default()
        };
    }

    let items: Vec<SelectionItem> = sessions
        .into_iter()
        .map(|session| {
            let session_id = session.session_id.clone();
            let project_key_clone = project_key.clone();

            // Format: "timestamp | message_count msgs | first_message_preview"
            let timestamp_str = format_timestamp(session.created_at);
            let msg_count = format!("{} msgs", session.message_count);

            // Create action that sends the LoadSessionTranscript event
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::LoadSessionTranscript {
                    project_key: project_key_clone.clone(),
                    session_id: session_id.clone(),
                });
            })];

            SelectionItem {
                name: session.first_message_preview,
                description: Some(format!("{timestamp_str} | {msg_count}")),
                is_current: false,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("Select Session".to_string()),
        subtitle: Some("View transcript of a previous session (read-only)".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use tokio::sync::mpsc::unbounded_channel;

    #[test]
    fn test_session_picker_empty_sessions() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let params = session_picker_params(vec![], "test-project".to_string(), tx);

        assert!(params.title.is_some());
        assert!(params.title.unwrap().contains("Previous Sessions"));
        assert_eq!(params.items.len(), 1);
        assert!(params.items[0].name.contains("No previous sessions"));
    }

    #[test]
    fn test_session_picker_with_sessions() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);

        let sessions = vec![
            SessionSummary {
                session_id: "session-1".to_string(),
                created_at: 1700000000,
                last_activity: 1700001000,
                first_message_preview: "Hello world".to_string(),
                message_count: 4,
            },
            SessionSummary {
                session_id: "session-2".to_string(),
                created_at: 1699999000,
                last_activity: 1699999500,
                first_message_preview: "Another conversation".to_string(),
                message_count: 2,
            },
        ];

        let params = session_picker_params(sessions, "test-project".to_string(), tx);

        assert!(params.title.is_some());
        assert!(params.title.unwrap().contains("Select Session"));
        assert_eq!(params.items.len(), 2);
        assert_eq!(params.items[0].name, "Hello world");
        assert_eq!(params.items[1].name, "Another conversation");
    }

    #[test]
    fn test_format_timestamp() {
        let ts = 1700000000; // 2023-11-14 22:13:20 UTC
        let formatted = format_timestamp(ts);
        assert!(formatted.contains("2023-11-14"));
    }
}
