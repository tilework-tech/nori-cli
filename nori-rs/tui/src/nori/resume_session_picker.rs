//! Resume session picker for /resume command.
//!
//! This module provides the UI for selecting a previous session to resume.
//! Selected sessions are resumed over ACP via `session/load` (history replay)
//! or `session/resume` (live reattach), depending on which capability the
//! agent advertises; agents with neither fall back to a fresh session plus
//! client-side transcript replay.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use nori_harness::transcript::TranscriptLoader;
use nori_protocol::acp::v1::SessionInfo;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::ComponentPickerParams;
use crate::bottom_pane::SelectionAction;
use crate::nori::viewonly_session_picker::SessionPickerInfo;
use crate::nori::viewonly_session_picker::format_relative_time;
use crate::nori::viewonly_session_picker::format_session_name;
use nori_tui_components::PickerColumn;
use nori_tui_components::PickerColumnWidth;
use nori_tui_components::PickerDensity;
use nori_tui_components::PickerDetail;
use nori_tui_components::PickerItem;
use nori_tui_components::PickerState;
use nori_tui_components::SearchMode;

/// Build the local transcript resume screen with the shared component picker.
/// Domain actions remain in this adapter; the reusable crate only returns the
/// selected stable session key.
pub fn resume_session_component_picker_params(
    sessions: Vec<SessionPickerInfo>,
    nori_home: PathBuf,
) -> ComponentPickerParams {
    let columns = vec![
        PickerColumn::flexible("session", "Session").width(PickerColumnWidth::Flexible {
            min: 18,
            max: 38,
            weight: 3,
        }),
        PickerColumn::flexible("preview", "First message")
            .hide_below(70)
            .width(PickerColumnWidth::Flexible {
                min: 14,
                max: 42,
                weight: 3,
            }),
        PickerColumn::fixed("turns", "Turns", 7).hide_below(54),
        PickerColumn::fixed("updated", "Updated", 16),
    ];
    let mut actions = BTreeMap::new();
    let items = sessions
        .into_iter()
        .map(|session| {
            let timestamp = format_relative_time(&session.started_at);
            let label = format_session_name(&timestamp, session.user_turn_count);
            let preview = session.first_message_preview.clone().unwrap_or_default();
            let search_text = resume_session_search_value(
                &session.session_id,
                session.first_message_preview.as_deref(),
            );
            let session_id = session.session_id;
            let project_id = session.project_id;
            let action_session_id = session_id.clone();
            let action_project_id = project_id.clone();
            let action_nori_home = nori_home.clone();
            actions.insert(
                session_id.clone(),
                Box::new(move |tx: &AppEventSender| {
                    tx.send(AppEvent::ResumeSession {
                        nori_home: action_nori_home.clone(),
                        project_id: action_project_id.clone(),
                        session_id: action_session_id.clone(),
                    });
                }) as SelectionAction,
            );
            PickerItem::new(session_id.clone(), "session", label)
                .cell("preview", preview.clone())
                .cell(
                    "turns",
                    session
                        .user_turn_count
                        .map(|turns| turns.to_string())
                        .unwrap_or_else(|| "Not reported".to_string()),
                )
                .cell("updated", timestamp)
                .search_text(search_text)
                .description(if preview.is_empty() {
                    "First message unavailable".to_string()
                } else {
                    preview.clone()
                })
                .details([
                    PickerDetail::new("Session", session_id),
                    PickerDetail::new("Project", project_id),
                    PickerDetail::new("Started", session.started_at),
                    PickerDetail::new(
                        "First message",
                        if preview.is_empty() {
                            "Unavailable".to_string()
                        } else {
                            preview
                        },
                    ),
                ])
        })
        .collect::<Vec<_>>();
    let subtitle = if items.is_empty() {
        "No previous sessions found for this agent"
    } else {
        "Search by first message or session id"
    };
    ComponentPickerParams {
        state: PickerState::new("Resume previous session", columns, items)
            .subtitle(subtitle)
            .search_mode(SearchMode::Fuzzy)
            .search_placeholder("First message or session id"),
        actions,
        on_dismiss: None,
        on_shift_tab: None,
        primary_column: "session".to_string(),
        detail_column: Some("preview".to_string()),
        density: PickerDensity::Compact,
        keep_open: std::collections::BTreeSet::new(),
        footer_hints: None,
    }
}

/// Build the ACP `session/list` resume screen with the shared component
/// picker. Every standard ACP field is represented in a column or detail pane;
/// `_meta` is retained as formatted detail, and turn status is an optional
/// declarative column for multi-session agents.
pub fn acp_resume_session_component_picker_params(
    mut sessions: Vec<SessionInfo>,
) -> ComponentPickerParams {
    sessions.retain(|session| {
        !is_cloud_session(session)
            || cloud_session_type(session)
                .is_none_or(|source| matches!(source, "slack" | "cli" | "web"))
    });
    let has_sessions = !sessions.is_empty();
    let cloud_picker = has_sessions && sessions.iter().all(is_cloud_session);
    sessions.sort_by_cached_key(|session| {
        std::cmp::Reverse(
            session
                .updated_at
                .as_deref()
                .and_then(|timestamp| chrono::DateTime::parse_from_rfc3339(timestamp).ok()),
        )
    });
    let mut columns = vec![
        PickerColumn::flexible("title", "Session").width(PickerColumnWidth::Flexible {
            min: 18,
            max: 72,
            weight: 6,
        }),
        PickerColumn::fixed("source", "Source", 9).hide_below(31),
        PickerColumn::fixed("updated", "Updated", 16).hide_below(49),
    ];
    if !cloud_picker {
        columns.insert(
            2,
            PickerColumn::flexible("cwd", "Working directory")
                .hide_below(68)
                .width(PickerColumnWidth::Flexible {
                    min: 14,
                    max: 24,
                    weight: 1,
                }),
        );
        columns.push(PickerColumn::fixed("status", "Turn status", 14).hide_below(81));
    }
    let mut actions = BTreeMap::new();
    actions.insert(
        "__new__".to_string(),
        Box::new(|tx: &AppEventSender| tx.send(AppEvent::NewSession)) as SelectionAction,
    );
    let create_new = PickerItem::new("__new__".to_string(), "title", "Start a new session")
        .cell("source", "CLI")
        .cell("cwd", "Not reported")
        .cell("updated", "now")
        .cell("status", "ready")
        .search_text("start a new session create")
        .pinned(true)
        .description("Create a fresh ACP session")
        .details([
            PickerDetail::new("Action", "Create a fresh ACP session"),
            PickerDetail::new("Existing session", "No session will be claimed implicitly"),
        ]);
    let session_items = sessions.into_iter().map(|session| {
        let is_cloud_origin = is_cloud_session(&session);
        let session_type = cloud_session_type(&session);
        let source = match session_type {
            Some("slack") => "Slack".to_string(),
            Some("cli") => "CLI".to_string(),
            Some("web") => "Web".to_string(),
            Some(_) | None if is_cloud_origin => "Unknown".to_string(),
            Some(other) => other.to_string(),
            None => "Local".to_string(),
        };
        let cwd = if !is_cloud_origin {
            session.cwd.display().to_string()
        } else {
            Default::default()
        };
        let title = session
            .title
            .clone()
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| session.session_id.to_string());
        let updated = session
            .updated_at
            .as_deref()
            .map(format_relative_time)
            .unwrap_or_else(|| "unknown".to_string());
        let turn_status = session
            .meta
            .as_ref()
            .and_then(|meta| meta.get("nori"))
            .and_then(|nori| {
                nori.get("currentTurnStatus")
                    .or_else(|| nori.get("current_turn_status"))
            })
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Not reported")
            .to_string();
        let session_id = session.session_id.to_string();
        let search_text = [
            session_id.as_str(),
            title.as_str(),
            source.as_str(),
            cwd.as_str(),
            turn_status.as_str(),
        ]
        .into_iter()
        .filter(|value| !value.is_empty() && *value != "Not reported")
        .collect::<Vec<_>>()
        .join(" ");
        let action_session_id = session_id.clone();
        let action_title = session.title.filter(|title| !title.is_empty());
        actions.insert(
            session_id.clone(),
            Box::new(move |tx: &AppEventSender| {
                tx.send(AppEvent::ResumeAcpSession {
                    acp_session_id: action_session_id.clone(),
                    title: action_title.clone(),
                });
            }) as SelectionAction,
        );
        let meta = session
            .meta
            .as_ref()
            .and_then(|meta| serde_json::to_string(meta).ok())
            .unwrap_or_else(|| "none".to_string());
        PickerItem::new(session_id.clone(), "title", title)
            .cell("source", source.clone())
            .cell("cwd", if cwd.is_empty() { "Not reported" } else { &cwd })
            .cell("updated", updated)
            .cell("status", &turn_status)
            .search_text(search_text)
            .description(format!(
                "{} · {turn_status}",
                if cwd.is_empty() {
                    "Remote session"
                } else {
                    &cwd
                }
            ))
            .details([
                PickerDetail::new("Session id", session_id),
                PickerDetail::new("Source", source),
                PickerDetail::new(
                    "Working directory",
                    if cwd.is_empty() {
                        "Not reported".to_string()
                    } else {
                        cwd
                    },
                ),
                PickerDetail::new(
                    "Updated at",
                    session
                        .updated_at
                        .unwrap_or_else(|| "Not reported".to_string()),
                ),
                PickerDetail::new("ACP _meta", meta),
            ])
    });
    let items = std::iter::once(create_new)
        .chain(session_items)
        .collect::<Vec<_>>();
    let on_dismiss = Box::new(|tx: &AppEventSender| {
        tx.send(AppEvent::InsertHistoryCell(Box::new(
            crate::history_cell::new_info_event(
                "No session selected. /resume reopens the picker; /new starts a fresh session."
                    .to_string(),
                None,
            ),
        )));
    }) as SelectionAction;
    ComponentPickerParams {
        state: PickerState::new("Sessions", columns, items)
            .subtitle(if has_sessions {
                "Resume a live session or start a new one"
            } else {
                "The agent reported no resumable sessions"
            })
            .search_mode(SearchMode::Fuzzy)
            .search_placeholder("Title, path, status, or session id"),
        actions,
        on_dismiss: Some(on_dismiss),
        on_shift_tab: None,
        primary_column: "title".to_string(),
        detail_column: None,
        density: PickerDensity::Compact,
        keep_open: std::collections::BTreeSet::new(),
        footer_hints: None,
    }
}

fn is_cloud_session(session: &SessionInfo) -> bool {
    session
        .meta
        .as_ref()
        .and_then(|meta| meta.get("nori"))
        .and_then(|nori| nori.get("origin"))
        .and_then(serde_json::Value::as_str)
        == Some("cloud")
        || session.cwd == Path::new("/")
}

fn cloud_session_type(session: &SessionInfo) -> Option<&str> {
    session
        .meta
        .as_ref()
        .and_then(|meta| meta.get("nori"))
        .and_then(|nori| nori.get("sessionType"))
        .and_then(serde_json::Value::as_str)
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
    use assert_matches::assert_matches;
    use insta::assert_snapshot;
    use nori_tui_components::Picker;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::Mutex;

    use nori_harness::TranscriptRecorder;
    use tracing_subscriber::fmt::MakeWriter;

    fn acp_session_info(
        session_id: &str,
        cwd: &str,
        title: Option<&str>,
        updated_at: Option<&str>,
        meta: Option<serde_json::Value>,
    ) -> SessionInfo {
        let mut session = SessionInfo::new(session_id.to_string(), PathBuf::from(cwd));
        session.title = title.map(str::to_string);
        session.updated_at = updated_at.map(str::to_string);
        session.meta = meta.and_then(|value| value.as_object().cloned());
        session
    }

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

    fn component_picker_snapshot(
        params: &ComponentPickerParams,
        width: u16,
        height: u16,
    ) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    Picker::new(&params.state)
                        .density(params.density)
                        .fullscreen_selection_rails(true),
                    frame.area(),
                )
            })
            .expect("draw shared resume picker");
        terminal.backend().to_string()
    }

    fn item_cell<'a>(params: &'a ComponentPickerParams, index: usize, column: &str) -> &'a str {
        params.state.items[index]
            .cells
            .get(column)
            .map(String::as_str)
            .unwrap_or("")
    }

    #[test]
    fn shared_local_resume_picker_snapshot() {
        let sessions = vec![
            SessionPickerInfo {
                session_id: "session-019f-local".to_string(),
                project_id: "project-nori-cli".to_string(),
                started_at: "2025-01-27T12:00:00Z".to_string(),
                user_turn_count: Some(4),
                first_message_preview: Some("Build the reusable picker".to_string()),
            },
            SessionPickerInfo {
                session_id: "session-018a-local".to_string(),
                project_id: "project-nori-cli".to_string(),
                started_at: "2025-01-26T10:00:00Z".to_string(),
                user_turn_count: Some(2),
                first_message_preview: Some("Improve Markdown tables".to_string()),
            },
        ];
        let params = resume_session_component_picker_params(sessions, PathBuf::from("/tmp/nori"));

        assert_snapshot!(component_picker_snapshot(&params, 104, 14));
    }

    #[test]
    fn shared_acp_resume_picker_includes_extensible_status_column_snapshot() {
        let sessions = vec![
            acp_session_info(
                "agent-session-working",
                "/workspace/nori/cli",
                Some("Build TUI components"),
                Some("2020-01-15T10:30:00Z"),
                Some(serde_json::json!({
                    "nori": {
                        "origin": "local",
                        "currentTurnStatus": "working"
                    }
                })),
            ),
            acp_session_info(
                "agent-session-cloud",
                "/",
                Some("slack · claude"),
                Some("2020-01-14T10:30:00Z"),
                Some(serde_json::json!({
                    "nori": {
                        "origin": "cloud",
                        "currentTurnStatus": "waiting"
                    }
                })),
            ),
        ];
        let params = acp_resume_session_component_picker_params(sessions);

        assert_eq!(
            params
                .state
                .columns
                .iter()
                .map(|column| column.key.as_str())
                .collect::<Vec<_>>(),
            vec!["title", "source", "cwd", "updated", "status"]
        );
        assert_snapshot!(component_picker_snapshot(&params, 124, 15));
    }

    #[test]
    fn cloud_resume_picker_shows_user_facing_sources_and_hides_internal_sources() {
        let cloud = |session_id: &str, source: Option<&str>, title: &str| {
            acp_session_info(
                session_id,
                "/",
                Some(title),
                Some("2020-01-15T10:30:00Z"),
                Some(match source {
                    Some(source) => serde_json::json!({
                        "nori": { "origin": "cloud", "sessionType": source }
                    }),
                    None => serde_json::json!({ "nori": { "origin": "cloud" } }),
                }),
            )
        };
        let params = acp_resume_session_component_picker_params(vec![
            cloud("slack-session", Some("slack"), "Investigate deploy alerts"),
            cloud("cli-session", Some("cli"), "Fix the session picker"),
            cloud("web-session", Some("web"), "Review onboarding"),
            cloud("trigger-session", Some("trigger"), "Nightly maintenance"),
            cloud("legacy-session", None, "Legacy handroll session"),
        ]);
        let rendered = component_picker_snapshot(&params, 180, 14);

        assert!(rendered.contains("Slack"));
        assert!(rendered.contains("Web"));
        assert!(rendered.contains("Unknown"));
        assert!(rendered.contains("Investigate deploy alerts"));
        assert!(rendered.contains("Fix the session picker"));
        assert!(rendered.contains("Review onboarding"));
        assert!(rendered.contains("Legacy handroll session"));
        assert!(!rendered.contains("Nightly maintenance"));
        assert!(!rendered.contains("trigger-session"));
        let cli_item = params
            .state
            .items
            .iter()
            .find(|item| item.key == "cli-session")
            .expect("CLI session should remain visible");
        assert_eq!(
            cli_item.cells.get("source").map(String::as_str),
            Some("CLI")
        );
    }

    #[test]
    fn cloud_resume_picker_gives_titles_most_of_the_wide_viewport() {
        let params = acp_resume_session_component_picker_params(vec![acp_session_info(
            "cli-session",
            "/",
            Some("Fix cloud session discovery and make the complete user prompt visible"),
            Some("2020-01-15T10:30:00Z"),
            Some(serde_json::json!({
                "nori": { "origin": "cloud", "sessionType": "cli" }
            })),
        )]);

        let rendered = component_picker_snapshot(&params, 180, 12);
        assert!(
            rendered
                .contains("Fix cloud session discovery and make the complete user prompt visible")
        );
        assert!(rendered.contains("CLI"));
    }

    #[test]
    fn mixed_resume_picker_hides_columns_that_do_not_fit_beside_details() {
        let params = acp_resume_session_component_picker_params(vec![acp_session_info(
            "local-session",
            "/workspace/nori/cli",
            Some("Fix the picker layout"),
            Some("2020-01-15T10:30:00Z"),
            None,
        )]);

        let rendered = component_picker_snapshot(&params, 124, 12);
        assert!(!rendered.contains("Turn status"));
        assert!(rendered.contains("Working direc"));
        assert!(rendered.contains("Details"));
    }

    #[test]
    fn all_internal_cloud_sessions_use_the_empty_picker_copy() {
        let params = acp_resume_session_component_picker_params(vec![acp_session_info(
            "trigger-session",
            "/",
            Some("Nightly maintenance"),
            Some("2020-01-15T10:30:00Z"),
            Some(serde_json::json!({
                "nori": { "origin": "cloud", "sessionType": "trigger" }
            })),
        )]);

        assert_eq!(params.state.items.len(), 1);
        assert_eq!(
            params.state.subtitle.as_deref(),
            Some("The agent reported no resumable sessions")
        );
    }

    #[test]
    fn resume_picker_builds_items_from_sessions() {
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

        let params = resume_session_component_picker_params(sessions, PathBuf::from("/tmp"));

        assert_eq!(params.state.items.len(), 2);
        assert!(item_cell(&params, 0, "session").contains("4 turns"));
        assert!(item_cell(&params, 1, "session").contains("2 turns"));
        assert_eq!(item_cell(&params, 0, "preview"), "Hello world");
        assert_eq!(item_cell(&params, 1, "preview"), "");
        assert_eq!(params.state.search_mode, SearchMode::Fuzzy);
    }

    #[test]
    fn acp_resume_picker_maps_agent_session_summaries() {
        let sessions = vec![
            acp_session_info(
                "agent-sess-1",
                "/repo/one",
                Some("Fix the parser"),
                Some("2020-01-15T10:30:00Z"),
                None,
            ),
            acp_session_info("agent-sess-2", "/repo/two", None, None, None),
        ];

        let params = acp_resume_session_component_picker_params(sessions);

        // Row 0 is the pinned "start new" action; sessions follow.
        assert_eq!(params.state.items.len(), 3);
        // Title becomes the row name; missing title falls back to session id.
        assert_eq!(item_cell(&params, 1, "title"), "Fix the parser");
        assert_eq!(item_cell(&params, 2, "title"), "agent-sess-2");
        assert_eq!(item_cell(&params, 1, "cwd"), "/repo/one");
        assert_eq!(item_cell(&params, 2, "cwd"), "/repo/two");
        assert_eq!(params.state.search_mode, SearchMode::Fuzzy);
    }

    /// The agent-sourced picker always puts an explicit "start a new
    /// session" row first (first in vector order; search filtering applies
    /// to it like any row via its search text), so entering `nori cloud`
    /// (or `/resume` on a cloud agent) never has to claim a VM implicitly —
    /// creating is a deliberate pick.
    #[test]
    fn acp_resume_picker_pins_a_create_new_row_first() {
        let sessions = vec![acp_session_info(
            "agent-sess-1",
            "/",
            Some("slack · claude"),
            None,
            None,
        )];

        let params = acp_resume_session_component_picker_params(sessions);

        assert_eq!(params.state.items.len(), 2);
        let create_new = &params.state.items[0];
        assert_eq!(item_cell(&params, 0, "title"), "Start a new session");
        assert!(create_new.pinned);

        // Selecting the row must request a fresh session (the existing
        // new-session flow), and nothing else — a stray ResumeAcpSession
        // here would reattach instead of create.
        let (tx_raw, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let tx = AppEventSender::new(tx_raw);
        params.actions["__new__"](&tx);
        assert_matches!(rx.try_recv(), Ok(AppEvent::NewSession));
        assert_matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        );
    }

    /// Transition shim: handroll in the wild does not yet emit `_meta`, so a
    /// bare cwd == "/" still hides the pathname. When every agent emits the
    /// cloud `_meta` marker this shim (and this test) can be deleted in favor
    /// of `acp_resume_picker_hides_cwd_for_meta_marked_cloud_rows`.
    #[test]
    fn acp_resume_picker_hides_legacy_cloud_cwd_sentinel() {
        // Cloud sessions carry the sentinel cwd "/" — the broker tracks no
        // real working directory — so the picker must neither display it nor
        // include it in the search haystack.
        let sessions = vec![acp_session_info(
            "cloud-sess-1",
            "/",
            Some("slack · claude"),
            None,
            None,
        )];

        let params = acp_resume_session_component_picker_params(sessions);

        assert_eq!(item_cell(&params, 1, "title"), "slack · claude");
        assert_eq!(item_cell(&params, 1, "cwd"), "Not reported");
        assert_eq!(
            params.state.items[1].search_text,
            "cloud-sess-1 slack · claude Unknown"
        );
    }

    /// A cloud session's `_meta.nori.origin == "cloud"` marker hides the cwd
    /// even when the agent reports a real-looking path, decoupling cwd-hiding
    /// from the legacy "/" sentinel. The path must not appear in the row
    /// description nor the search haystack.
    #[test]
    fn acp_resume_picker_hides_cwd_for_meta_marked_cloud_rows() {
        let sessions = vec![acp_session_info(
            "cloud-sess-1",
            "/home/x/proj",
            Some("slack · claude"),
            None,
            Some(serde_json::json!({"nori": {"origin": "cloud"}})),
        )];

        let params = acp_resume_session_component_picker_params(sessions);

        assert_eq!(item_cell(&params, 1, "cwd"), "Not reported");
        assert_eq!(
            params.state.items[1].search_text,
            "cloud-sess-1 slack · claude Unknown"
        );
    }

    /// Guard against over-hiding: a session with a real cwd and no cloud
    /// `_meta` marker must still show its pathname — whether `_meta` is
    /// absent entirely or present but not cloud-shaped. An implementation
    /// that hides the cwd on any `Some(meta)` must fail here.
    #[test]
    fn acp_resume_picker_shows_cwd_for_unmarked_rows() {
        let sessions = vec![
            acp_session_info(
                "local-sess-1",
                "/home/x/proj",
                Some("Fix the parser"),
                None,
                None,
            ),
            acp_session_info(
                "local-sess-2",
                "/home/x/other",
                Some("Tune the linter"),
                None,
                Some(serde_json::json!({"nori": {"origin": "local"}})),
            ),
        ];

        let params = acp_resume_session_component_picker_params(sessions);

        assert_eq!(item_cell(&params, 1, "cwd"), "/home/x/proj");
        assert_eq!(
            params.state.items[1].search_text,
            "local-sess-1 Fix the parser Local /home/x/proj"
        );
        assert_eq!(item_cell(&params, 2, "cwd"), "/home/x/other");
        assert_eq!(
            params.state.items[2].search_text,
            "local-sess-2 Tune the linter Local /home/x/other"
        );
    }

    /// Session rows are ordered most-recent-first by `updated_at` regardless
    /// of the order the agent returned them in, with the pinned create-new
    /// row still first.
    #[test]
    fn acp_resume_picker_orders_rows_most_recent_first() {
        let sessions = vec![
            acp_session_info(
                "sess-jan",
                "/",
                Some("January session"),
                Some("2026-01-01T00:00:00Z"),
                None,
            ),
            acp_session_info(
                "sess-mar",
                "/",
                Some("March session"),
                Some("2026-03-01T00:00:00Z"),
                None,
            ),
            acp_session_info(
                "sess-feb",
                "/",
                Some("February session"),
                Some("2026-02-01T00:00:00Z"),
                None,
            ),
        ];

        let params = acp_resume_session_component_picker_params(sessions);

        let names = params
            .state
            .items
            .iter()
            .map(|item| item.cells["title"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "Start a new session",
                "March session",
                "February session",
                "January session",
            ]
        );
    }

    /// Rows without an `updated_at` sort after every dated row, and two
    /// undated rows keep their relative input order (the sort is stable).
    #[test]
    fn acp_resume_picker_orders_undated_rows_after_dated_rows_stably() {
        let sessions = vec![
            acp_session_info("sess-undated-a", "/", Some("Undated A"), None, None),
            acp_session_info(
                "sess-dated",
                "/",
                Some("Dated"),
                Some("2026-02-01T00:00:00Z"),
                None,
            ),
            acp_session_info("sess-undated-b", "/", Some("Undated B"), None, None),
        ];

        let params = acp_resume_session_component_picker_params(sessions);

        let names = params
            .state
            .items
            .iter()
            .map(|item| item.cells["title"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec!["Start a new session", "Dated", "Undated A", "Undated B"]
        );
    }

    /// An `updated_at` that fails RFC 3339 parsing is treated like a missing
    /// timestamp: the row sorts after every dated row rather than wherever a
    /// raw string comparison would land it.
    #[test]
    fn acp_resume_picker_treats_unparseable_updated_at_like_undated() {
        let sessions = vec![
            // Lexicographically after "2026-…", so a raw string sort
            // (descending) would wrongly rank this row first.
            acp_session_info(
                "sess-garbage",
                "/",
                Some("Garbage timestamp"),
                Some("not-a-timestamp"),
                None,
            ),
            acp_session_info(
                "sess-dated",
                "/",
                Some("Dated"),
                Some("2026-02-01T00:00:00Z"),
                None,
            ),
        ];

        let params = acp_resume_session_component_picker_params(sessions);

        let names = params
            .state
            .items
            .iter()
            .map(|item| item.cells["title"].as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec!["Start a new session", "Dated", "Garbage timestamp"]
        );
    }

    /// Even with no resumable sessions the picker offers the create-new row —
    /// this is the whole entry point for a first-time `nori cloud` user.
    #[test]
    fn acp_resume_picker_handles_empty_session_list() {
        let params = acp_resume_session_component_picker_params(vec![]);

        assert_eq!(params.state.items.len(), 1);
        assert_eq!(item_cell(&params, 0, "title"), "Start a new session");
        assert_eq!(
            params.state.subtitle.as_deref(),
            Some("The agent reported no resumable sessions")
        );
    }

    #[test]
    fn resume_picker_omits_turn_count_until_known() {
        let sessions = vec![SessionPickerInfo {
            session_id: "sess-1".to_string(),
            project_id: "proj-1".to_string(),
            started_at: "2025-01-27T12:00:00Z".to_string(),
            user_turn_count: None,
            first_message_preview: None,
        }];

        let params = resume_session_component_picker_params(sessions, PathBuf::from("/tmp"));

        assert_eq!(params.state.items.len(), 1);
        assert!(!item_cell(&params, 0, "session").contains("turn"));
        assert_eq!(item_cell(&params, 0, "preview"), "");
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
