//! View-only session picker for /resume-viewonly command.
//!
//! This module provides the UI for selecting a previous session to view.
//! Selected sessions are displayed read-only in the conversation history.

use std::path::Path;
use std::path::PathBuf;

use codex_acp::transcript::TranscriptLoader;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;

/// Metadata for a session in the picker, including preview text.
#[derive(Debug, Clone)]
pub struct SessionPickerInfo {
    /// Session identifier
    pub session_id: String,
    /// Project identifier
    pub project_id: String,
    /// When the session started (ISO 8601)
    pub started_at: String,
    /// Number of conversation entries
    pub entry_count: usize,
    /// Preview of first user message (truncated)
    pub first_message_preview: Option<String>,
}

/// Load sessions for the current working directory with preview text.
///
/// Sessions with only the session_meta entry (entry_count <= 1) are filtered out
/// since they have no actual conversation content to display.
pub async fn load_sessions_with_preview(
    nori_home: &Path,
    cwd: &Path,
) -> std::io::Result<Vec<SessionPickerInfo>> {
    let loader = TranscriptLoader::new(nori_home.to_path_buf());
    let sessions = loader.find_sessions_for_cwd(cwd).await?;

    let mut result = Vec::new();
    for session in sessions {
        // Skip sessions with no conversation content (only session_meta)
        if session.entry_count <= 1 {
            continue;
        }

        let preview =
            load_first_message_preview(&loader, &session.project_id, &session.session_id).await;
        result.push(SessionPickerInfo {
            session_id: session.session_id,
            project_id: session.project_id,
            started_at: session.started_at,
            entry_count: session.entry_count,
            first_message_preview: preview,
        });
    }

    Ok(result)
}

/// Load the first user message from a transcript for preview.
async fn load_first_message_preview(
    loader: &TranscriptLoader,
    project_id: &str,
    session_id: &str,
) -> Option<String> {
    let transcript = loader.load_transcript(project_id, session_id).await.ok()?;

    // Find the first user entry
    for line in &transcript.entries {
        if let codex_acp::transcript::TranscriptEntry::User(user) = &line.entry {
            let content = &user.content;
            // Truncate to first 50 chars for preview
            let preview = if content.chars().count() > 50 {
                let truncated: String = content.chars().take(50).collect();
                format!("{truncated}...")
            } else {
                content.clone()
            };
            return Some(preview);
        }
    }

    None
}

/// Create selection view parameters for the viewonly session picker.
pub fn viewonly_session_picker_params(
    sessions: Vec<SessionPickerInfo>,
    nori_home: PathBuf,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    if sessions.is_empty() {
        return SelectionViewParams {
            title: Some("View previous session".to_string()),
            subtitle: Some("No previous sessions found for this project".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items: vec![],
            ..Default::default()
        };
    }

    let items: Vec<SelectionItem> = sessions
        .into_iter()
        .map(|session| {
            let timestamp = format_relative_time(&session.started_at);
            let message_count = session.entry_count.saturating_sub(1); // Exclude session_meta

            // Build display name: timestamp · N messages
            let name = format!("{timestamp} · {message_count} messages");

            // Description shows first message preview
            let description = session
                .first_message_preview
                .clone()
                .map(|preview| format!("\"{preview}\""));

            let project_id = session.project_id.clone();
            let session_id = session.session_id.clone();
            let nori_home = nori_home.clone();

            // Create action that loads and displays the transcript
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx: &AppEventSender| {
                tx.send(AppEvent::LoadViewonlyTranscript {
                    nori_home: nori_home.clone(),
                    project_id: project_id.clone(),
                    session_id: session_id.clone(),
                });
            })];

            SelectionItem {
                name,
                description,
                search_value: Some(session.session_id),
                is_current: false,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();

    SelectionViewParams {
        title: Some("View previous session".to_string()),
        subtitle: Some("Select a session to view its transcript".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        is_searchable: true,
        search_placeholder: Some("Type to search sessions".to_string()),
        ..Default::default()
    }
}

/// Format an ISO 8601 timestamp as a relative time string.
pub(crate) fn format_relative_time(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|dt| {
            let now = chrono::Utc::now();
            let delta = now.signed_duration_since(dt);

            if delta.num_minutes() < 1 {
                "just now".to_string()
            } else if delta.num_hours() < 1 {
                let mins = delta.num_minutes();
                if mins == 1 {
                    "1 min ago".to_string()
                } else {
                    format!("{mins} min ago")
                }
            } else if delta.num_hours() < 24 {
                let hours = delta.num_hours();
                if hours == 1 {
                    "1 hour ago".to_string()
                } else {
                    format!("{hours} hours ago")
                }
            } else if delta.num_days() < 7 {
                let days = delta.num_days();
                if days == 1 {
                    "yesterday".to_string()
                } else {
                    format!("{days} days ago")
                }
            } else {
                dt.format("%Y-%m-%d %H:%M").to_string()
            }
        })
        .unwrap_or_else(|_| iso.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_relative_time_invalid() {
        let result = format_relative_time("not-a-date");
        assert_eq!(result, "not-a-date");
    }

    #[test]
    fn test_format_relative_time_old_date() {
        // Test with a date far in the past
        let result = format_relative_time("2020-01-15T10:30:00Z");
        assert!(result.starts_with("2020-01-15"));
    }
}
