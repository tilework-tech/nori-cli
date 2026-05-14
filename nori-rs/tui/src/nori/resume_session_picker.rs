//! Resume session picker for /resume command.
//!
//! This module provides the UI for selecting a previous session to resume.
//! Selected sessions are resumed via the ACP `session/load` protocol method,
//! allowing the agent to restore its own context and stream conversation history.

use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use nori_acp::transcript::TranscriptLoader;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::nori::viewonly_session_picker::SessionPickerInfo;
use crate::nori::viewonly_session_picker::format_relative_time;
use crate::nori::viewonly_session_picker::format_session_name;

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
            let name = format_session_name(&timestamp, session.user_turn_count);

            let description = session
                .first_message_preview
                .clone()
                .map(|preview| format!("\"{preview}\""));
            let search_value = resume_session_search_value(
                &session.session_id,
                session.first_message_preview.as_deref(),
            );

            let session_id = session.session_id;
            let project_id = session.project_id;
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
                search_value: Some(search_value),
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

pub(crate) fn resume_session_item_update(
    session_id: &str,
    started_at: &str,
    first_message_preview: Option<&str>,
    user_turn_count: Option<usize>,
) -> (String, Option<String>, String) {
    let timestamp = format_relative_time(started_at);
    let name = format_session_name(&timestamp, user_turn_count);
    let description = first_message_preview.map(|preview| format!("\"{preview}\""));
    let search_value = resume_session_search_value(session_id, first_message_preview);
    (name, description, search_value)
}

fn resume_session_search_value(session_id: &str, first_message_preview: Option<&str>) -> String {
    match first_message_preview {
        Some(preview) => format!("{session_id} {preview}"),
        None => session_id.to_string(),
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
    let started = Instant::now();
    tracing::info!(
        target: "nori_resume",
        phase = "load_resumable_sessions.start",
        nori_home = %nori_home.display(),
        cwd = %cwd.display(),
        agent_filter = %agent_filter,
        "loading /resume sessions before picker display",
    );

    // Filter by agent before any transcript body work. Different agents have
    // incompatible resume formats, and transcript summary loading may scan
    // large files.
    let filter_started = Instant::now();
    let loader = TranscriptLoader::new(nori_home.to_path_buf());
    let session_infos = loader.find_session_metadata_for_cwd(cwd).await?;
    let session_info_count = session_infos.len();

    let matching_session_infos: Vec<_> = session_infos
        .into_iter()
        .filter(|info| info.agent.as_deref() == Some(agent_filter))
        .collect();

    tracing::info!(
        target: "nori_resume",
        phase = "load_resumable_sessions.agent_filter_metadata_loaded",
        elapsed_ms = filter_started.elapsed().as_millis(),
        total_elapsed_ms = started.elapsed().as_millis(),
        session_info_count,
        matching_session_count = matching_session_infos.len(),
        agent_filter = %agent_filter,
        "loaded session metadata for /resume agent filtering",
    );

    let filtered: Vec<SessionPickerInfo> = matching_session_infos
        .into_iter()
        .map(SessionPickerInfo::from)
        .collect();
    tracing::info!(
        target: "nori_resume",
        phase = "load_resumable_sessions.metadata_rows_built",
        total_elapsed_ms = started.elapsed().as_millis(),
        returned_session_count = filtered.len(),
        agent_filter = %agent_filter,
        "built metadata-only resumable session rows",
    );

    tracing::info!(
        target: "nori_resume",
        phase = "load_resumable_sessions.done",
        total_elapsed_ms = started.elapsed().as_millis(),
        returned_session_count = filtered.len(),
        agent_filter = %agent_filter,
        "finished loading /resume sessions before picker display",
    );

    Ok(filtered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::Mutex;

    use nori_acp::TranscriptRecorder;
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone)]
    struct CapturedLogs {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl CapturedLogs {
        fn new() -> Self {
            Self {
                bytes: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn contents(&self) -> String {
            let bytes = self.bytes.lock().unwrap();
            String::from_utf8_lossy(&bytes).into_owned()
        }
    }

    struct CapturedLogWriter {
        bytes: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for CapturedLogWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.bytes.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'writer> MakeWriter<'writer> for CapturedLogs {
        type Writer = CapturedLogWriter;

        fn make_writer(&'writer self) -> Self::Writer {
            CapturedLogWriter {
                bytes: self.bytes.clone(),
            }
        }
    }

    #[test]
    fn resume_picker_builds_items_from_sessions() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let app_event_tx = AppEventSender::new(tx);

        let sessions = vec![
            SessionPickerInfo {
                session_id: "sess-1".to_string(),
                project_id: "proj-1".to_string(),
                started_at: "2025-01-27T12:00:00Z".to_string(),
                user_turn_count: Some(4),
                first_message_preview: Some("Hello world".to_string()),
            },
            SessionPickerInfo {
                session_id: "sess-2".to_string(),
                project_id: "proj-1".to_string(),
                started_at: "2025-01-26T10:00:00Z".to_string(),
                user_turn_count: Some(2),
                first_message_preview: None,
            },
        ];

        let params = resume_session_picker_params(sessions, PathBuf::from("/tmp"), app_event_tx);

        assert_eq!(params.items.len(), 2);
        assert!(params.items[0].name.contains("4 turns"));
        assert!(params.items[1].name.contains("2 turns"));
        assert_eq!(
            params.items[0].description.as_deref(),
            Some("\"Hello world\"")
        );
        assert!(params.items[1].description.is_none());
        assert!(params.is_searchable);
    }

    #[test]
    fn resume_picker_omits_turn_count_until_known() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let app_event_tx = AppEventSender::new(tx);
        let sessions = vec![SessionPickerInfo {
            session_id: "sess-1".to_string(),
            project_id: "proj-1".to_string(),
            started_at: "2025-01-27T12:00:00Z".to_string(),
            user_turn_count: None,
            first_message_preview: None,
        }];

        let params = resume_session_picker_params(sessions, PathBuf::from("/tmp"), app_event_tx);

        assert_eq!(params.items.len(), 1);
        assert!(!params.items[0].name.contains("turn"));
        assert!(params.items[0].description.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn load_resumable_sessions_filters_agent_before_loading_previews() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let nori_home = temp_dir.path().join("nori-home");
        let cwd = temp_dir.path().join("repo");
        tokio::fs::create_dir_all(&cwd).await.unwrap();

        let nonmatching_recorder = TranscriptRecorder::new(
            &nori_home,
            &cwd,
            Some("claude-code".to_string()),
            "0.1.0",
            None,
        )
        .await
        .unwrap();
        let nonmatching_session_id = nonmatching_recorder.session_id().to_string();
        nonmatching_recorder
            .record_user_message("msg-nonmatching", "do not preview me", vec![])
            .await
            .unwrap();
        nonmatching_recorder.flush().await.unwrap();
        nonmatching_recorder.shutdown().await.unwrap();

        let matching_recorder =
            TranscriptRecorder::new(&nori_home, &cwd, Some("codex".to_string()), "0.1.0", None)
                .await
                .unwrap();
        let matching_session_id = matching_recorder.session_id().to_string();
        matching_recorder
            .record_user_message("msg-matching", "preview me", vec![])
            .await
            .unwrap();
        matching_recorder.flush().await.unwrap();
        matching_recorder.shutdown().await.unwrap();

        let captured_logs = CapturedLogs::new();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(captured_logs.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let sessions = load_resumable_sessions(&nori_home, &cwd, "codex")
            .await
            .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, matching_session_id);
        assert!(sessions[0].first_message_preview.is_none());
        assert!(sessions[0].user_turn_count.is_none());

        let logs = captured_logs.contents();
        let preview_started_for = |session_id: &str| {
            logs.lines().any(|line| {
                line.contains("phase=\"load_first_message_preview.start\"")
                    && line.contains(&format!("session_id=\"{session_id}\""))
            })
        };

        assert!(
            !preview_started_for(&matching_session_id),
            "initial /resume load should not preview matching session before picker display; logs:\n{logs}"
        );
        assert!(
            !preview_started_for(&nonmatching_session_id),
            "nonmatching session should be filtered before preview loading; logs:\n{logs}"
        );
    }
}
