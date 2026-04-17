//! Resume session picker for /resume command.
//!
//! This module provides the UI for selecting a previous session to resume.
//! Selected sessions are resumed via the ACP `session/load` protocol method,
//! allowing the agent to restore its own context and stream conversation history.

use std::path::Path;
use std::path::PathBuf;

use nori_acp::transcript::TranscriptLoader;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::nori::viewonly_session_picker::SessionPickerInfo;
use crate::nori::viewonly_session_picker::format_relative_time;
use crate::nori::viewonly_session_picker::load_sessions_with_preview;

/// Create selection view parameters for the resume session picker.
///
/// This filters sessions to only show those from the specified agent,
/// since different agents have incompatible session formats.
pub fn resume_session_picker_params(
    sessions: Vec<SessionPickerInfo>,
    nori_home: PathBuf,
    _app_event_tx: AppEventSender,
) -> SelectionViewParams {
    if sessions.is_empty() {
        return SelectionViewParams {
            title: Some("Resume previous session".to_string()),
            subtitle: Some("No previous sessions found for this agent".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items: vec![],
            ..Default::default()
        };
    }

    let items: Vec<SelectionItem> = sessions
        .into_iter()
        .map(|session| {
            let timestamp = format_relative_time(&session.started_at);
            let message_count = session.entry_count.saturating_sub(1);

            let name = format!("{timestamp} · {message_count} messages");

            let description = session
                .first_message_preview
                .clone()
                .map(|preview| format!("\"{preview}\""));

            let session_id = session.session_id.clone();
            let project_id = session.project_id.clone();
            let nori_home = nori_home.clone();

            let actions: Vec<SelectionAction> = vec![Box::new(move |tx: &AppEventSender| {
                tx.send(AppEvent::ResumeSession {
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
        title: Some("Resume previous session".to_string()),
        subtitle: Some("Select a session to resume".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        is_searchable: true,
        search_placeholder: Some("Type to search sessions".to_string()),
        ..Default::default()
    }
}

/// Load resumable sessions for the given agent and working directory.
///
/// Filters sessions to only include those created by the specified agent,
/// since `session/load` can only resume sessions from the same agent type.
pub async fn load_resumable_sessions(
    nori_home: &Path,
    cwd: &Path,
    agent_filter: &str,
) -> std::io::Result<Vec<SessionPickerInfo>> {
    let all_sessions = load_sessions_with_preview(nori_home, cwd).await?;

    // Filter by agent - only show sessions from the currently active agent
    // The agent field in SessionPickerInfo comes from SessionMetaEntry.agent
    let loader = TranscriptLoader::new(nori_home.to_path_buf());
    let session_infos = loader.find_sessions_for_cwd(cwd).await?;

    let matching_session_ids: std::collections::HashSet<String> = session_infos
        .into_iter()
        .filter(|info| info.agent.as_deref() == Some(agent_filter))
        .map(|info| info.session_id)
        .collect();

    Ok(all_sessions
        .into_iter()
        .filter(|s| matching_session_ids.contains(&s.session_id))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_picker_builds_items_from_sessions() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let app_event_tx = AppEventSender::new(tx);

        let sessions = vec![
            SessionPickerInfo {
                session_id: "sess-1".to_string(),
                project_id: "proj-1".to_string(),
                started_at: "2025-01-27T12:00:00Z".to_string(),
                entry_count: 5,
                first_message_preview: Some("Hello world".to_string()),
            },
            SessionPickerInfo {
                session_id: "sess-2".to_string(),
                project_id: "proj-1".to_string(),
                started_at: "2025-01-26T10:00:00Z".to_string(),
                entry_count: 3,
                first_message_preview: None,
            },
        ];

        let params = resume_session_picker_params(sessions, PathBuf::from("/tmp"), app_event_tx);

        assert_eq!(params.items.len(), 2);
        assert!(params.items[0].name.contains("4 messages"));
        assert!(params.items[1].name.contains("2 messages"));
        assert_eq!(
            params.items[0].description.as_deref(),
            Some("\"Hello world\"")
        );
        assert!(params.items[1].description.is_none());
        assert!(params.is_searchable);
    }
}
