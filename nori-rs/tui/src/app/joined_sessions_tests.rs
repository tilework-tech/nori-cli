//! The App on a joined host: what the host's session list opens and how a
//! session the host stops listing is shown.

use std::sync::Arc;

use codex_login::AuthManager;
use nori_config::NoriConfig;
use nori_protocol::ControlRole;
use nori_protocol::SessionStatus;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::App;
use crate::host_client::HostConnection;
use crate::host_client::HostFeatures;
use crate::tui::FrameRequester;

const AMBER: &str = "a1a1a1a1-0000-4000-8000-000000000001";
const BIRCH: &str = "b2b2b2b2-0000-4000-8000-000000000002";

fn connection() -> HostConnection {
    HostConnection {
        role: ControlRole::Controller,
        connected: true,
        stopped: None,
        features: HostFeatures::default(),
        agents: Vec::new(),
    }
}

fn joined_app(home: &tempfile::TempDir, open_session: Option<&str>) -> App {
    joined_app_with_events(home, open_session).0
}

fn joined_app_with_events(
    home: &tempfile::TempDir,
    open_session: Option<&str>,
) -> (
    App,
    tokio::sync::mpsc::UnboundedReceiver<crate::app_event::AppEvent>,
) {
    joined_app_on(home, open_session, connection())
}

fn joined_app_on(
    home: &tempfile::TempDir,
    open_session: Option<&str>,
    connection: HostConnection,
) -> (
    App,
    tokio::sync::mpsc::UnboundedReceiver<crate::app_event::AppEvent>,
) {
    let config = NoriConfig {
        cwd: home.path().to_path_buf(),
        nori_home: home.path().to_path_buf(),
        ..NoriConfig::default()
    };
    let auth_manager = AuthManager::shared(
        home.path().to_path_buf(),
        false,
        codex_login::AuthCredentialsStoreMode::File,
    );
    App::joined(
        FrameRequester::test_dummy(),
        false,
        Arc::clone(&auth_manager),
        config,
        connection,
        open_session.map(str::to_owned),
        None,
    )
}

/// The notices the terminal printed, one per line.
fn printed(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<crate::app_event::AppEvent>,
) -> String {
    let mut text = Vec::new();
    while let Ok(mut event) = events.try_recv() {
        if let crate::app_event::AppEvent::SessionHistory { event: inner, .. } = event {
            event = *inner;
        }
        if let crate::app_event::AppEvent::InsertHistoryCell(cell) = event {
            for line in cell.display_lines(80) {
                text.push(
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>(),
                );
            }
        }
    }
    text.join("\n")
}

fn row(id: &str, title: &str) -> Value {
    json!({
        "session_id": id,
        "agent": "mock-model",
        "status": "ready",
        "title": title,
    })
}

fn list(rows: &[Value]) -> Value {
    json!({ "sessions": rows })
}

fn on_screen(app: &App) -> Option<String> {
    app.chat_widget
        .managed_conversation_id()
        .map(|id| id.to_string())
}

fn listed_status(app: &App, title: &str) -> Option<SessionStatus> {
    app.session_rows()
        .into_iter()
        .find(|row| row.title.contains(title))
        .map(|row| row.status)
}

#[tokio::test]
async fn the_first_list_opens_the_one_session_a_prefix_names() {
    let home = tempfile::tempdir().unwrap();
    let mut app = joined_app(&home, Some("b2b2"));

    app.on_joined_sessions(
        list(&[row(AMBER, "synthetic amber"), row(BIRCH, "synthetic birch")]),
        FrameRequester::test_dummy(),
        &connection(),
    );

    assert_eq!(on_screen(&app).as_deref(), Some(BIRCH));
    assert!(!app.chat_widget.session_list_in_front());
}

#[tokio::test]
async fn a_prefix_that_names_no_single_session_opens_the_list_instead() {
    let home = tempfile::tempdir().unwrap();
    for prefix in ["c3c3", ""] {
        let mut app = joined_app(&home, Some(prefix));
        app.on_joined_sessions(
            list(&[row(AMBER, "synthetic amber"), row(BIRCH, "synthetic birch")]),
            FrameRequester::test_dummy(),
            &connection(),
        );
        assert_eq!(on_screen(&app), None, "prefix {prefix:?}");
        assert!(app.chat_widget.session_list_in_front(), "prefix {prefix:?}");
    }
}

#[tokio::test]
async fn a_later_list_never_moves_the_terminal_to_another_session() {
    let home = tempfile::tempdir().unwrap();
    let mut app = joined_app(&home, Some(AMBER));
    app.on_joined_sessions(
        list(&[row(AMBER, "synthetic amber")]),
        FrameRequester::test_dummy(),
        &connection(),
    );
    app.on_joined_sessions(
        list(&[row(AMBER, "synthetic amber"), row(BIRCH, "synthetic birch")]),
        FrameRequester::test_dummy(),
        &connection(),
    );
    assert_eq!(on_screen(&app).as_deref(), Some(AMBER));
}

#[tokio::test]
async fn a_session_the_host_stops_listing_is_shown_closed() {
    let home = tempfile::tempdir().unwrap();
    let mut app = joined_app(&home, Some(AMBER));
    app.on_joined_sessions(
        list(&[row(AMBER, "synthetic amber"), row(BIRCH, "synthetic birch")]),
        FrameRequester::test_dummy(),
        &connection(),
    );
    app.on_joined_sessions(
        list(&[row(AMBER, "synthetic amber")]),
        FrameRequester::test_dummy(),
        &connection(),
    );

    assert_eq!(
        listed_status(&app, "synthetic birch"),
        Some(SessionStatus::Closed)
    );
    assert_eq!(
        listed_status(&app, "synthetic amber"),
        Some(SessionStatus::Ready)
    );
}

#[tokio::test]
async fn typing_before_a_picked_session_comes_on_screen_reaches_that_session() {
    let home = tempfile::tempdir().unwrap();
    let mut app = joined_app(&home, None);
    app.on_joined_sessions(
        list(&[row(AMBER, "synthetic amber")]),
        FrameRequester::test_dummy(),
        &connection(),
    );
    app.chat_widget.set_composer_text("/q".to_string());

    let generation = app
        .session_rows()
        .into_iter()
        .find(|row| row.title.contains("synthetic amber"))
        .map(|row| row.generation)
        .unwrap();
    app.show_session(generation);

    assert_eq!(on_screen(&app).as_deref(), Some(AMBER));
    assert_eq!(app.chat_widget.composer_text(), "/q");
}

#[tokio::test]
async fn an_ended_session_prints_only_the_ended_notice() {
    let home = tempfile::tempdir().unwrap();
    let (mut app, mut events) = joined_app_with_events(&home, Some(AMBER));
    app.on_joined_sessions(
        list(&[row(AMBER, "synthetic amber")]),
        FrameRequester::test_dummy(),
        &connection(),
    );
    let _ = printed(&mut events);

    app.show_joined_placeholder(
        FrameRequester::test_dummy(),
        super::session_lifecycle::SESSION_ENDED,
    );
    app.refresh_joined_session_status();
    app.refresh_joined_session_status();

    let text = printed(&mut events);
    assert!(!text.contains("No session selected"), "{text}");
    insta::assert_snapshot!(text);
}

#[tokio::test]
async fn closing_the_first_list_without_a_pick_says_how_to_get_a_session() {
    let home = tempfile::tempdir().unwrap();
    let (mut app, mut events) = joined_app_with_events(&home, None);
    app.on_joined_sessions(
        list(&[row(AMBER, "synthetic amber")]),
        FrameRequester::test_dummy(),
        &connection(),
    );
    app.chat_widget.close_session_list();
    let _ = printed(&mut events);

    app.refresh_joined_session_status();
    app.refresh_joined_session_status();

    let text = printed(&mut events);
    assert_eq!(text.matches("No session selected").count(), 1, "{text}");
    insta::assert_snapshot!(text);
}

#[tokio::test]
async fn a_started_session_opens_once_the_screen_is_free() {
    let home = tempfile::tempdir().unwrap();
    let mut app = joined_app(&home, Some(AMBER));
    app.on_joined_sessions(
        list(&[row(AMBER, "synthetic amber")]),
        FrameRequester::test_dummy(),
        &connection(),
    );
    app.open_when_listed(BIRCH.to_owned(), None);

    app.pending_session_activation = Some(super::PendingSessionActivation::New);
    let both = list(&[row(AMBER, "synthetic amber"), row(BIRCH, "synthetic birch")]);
    app.on_joined_sessions(both.clone(), FrameRequester::test_dummy(), &connection());
    assert_eq!(on_screen(&app).as_deref(), Some(AMBER));
    app.pending_session_activation = None;
    app.on_joined_sessions(both, FrameRequester::test_dummy(), &connection());

    assert_eq!(on_screen(&app).as_deref(), Some(BIRCH));
}

#[tokio::test]
async fn each_listed_row_names_the_folder_its_session_works_in() {
    use crate::render::renderable::Renderable;
    let home = tempfile::tempdir().unwrap();
    let mut app = joined_app(&home, None);
    let in_folder = |id: &str, title: &str, folder: &str, reply: &str| {
        let mut row = row(id, title);
        row["cwd"] = json!(format!("/synthetic/{folder}"));
        row["reply_preview"] = json!(reply);
        row["idle_seconds"] = json!(42);
        row
    };
    app.on_joined_sessions(
        list(&[
            in_folder(
                AMBER,
                "synthetic amber",
                "orchard",
                "Pruned the pear trees.",
            ),
            in_folder(BIRCH, "synthetic birch", "harbor", "Counted the gulls."),
        ]),
        FrameRequester::test_dummy(),
        &connection(),
    );

    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let list = crate::bottom_pane::session_list_view::SessionListView::new(
        app.session_rows(),
        crate::app_event_sender::AppEventSender::new(tx),
    );
    let area = ratatui::layout::Rect::new(0, 0, 80, list.desired_height(80));
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    list.render(area, &mut buffer);
    let screen = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(screen);
}

/// The sessions `@` asked the host to search in, oldest first; `None` is the
/// host's own folder.
fn searched_in(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<crate::app_event::AppEvent>,
) -> Vec<Option<String>> {
    let mut sessions = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let crate::app_event::AppEvent::HostRequest {
            call: crate::host_client::HostCall::FindPaths { session_id, .. },
            ..
        } = event
        {
            sessions.push(session_id);
        }
    }
    sessions
}

#[tokio::test]
async fn at_mentions_search_the_host_and_follow_the_session_on_screen() {
    let home = tempfile::tempdir().unwrap();
    let finding = HostConnection {
        features: HostFeatures {
            find_paths: true,
            ..HostFeatures::default()
        },
        ..connection()
    };
    let (mut app, mut events) = joined_app_on(&home, None, finding.clone());
    app.on_joined_sessions(
        list(&[row(AMBER, "synthetic amber"), row(BIRCH, "synthetic birch")]),
        FrameRequester::test_dummy(),
        &finding,
    );
    let generation_of = |app: &App, title: &str| {
        app.session_rows()
            .into_iter()
            .find(|row| row.title.contains(title))
            .map(|row| row.generation)
            .unwrap()
    };
    let _ = searched_in(&mut events);

    app.start_file_search("harbor".to_string());
    let with_no_session = searched_in(&mut events);
    app.on_host_file_search_landed(
        crate::host_client::FileSearch {
            session_id: None,
            query: "harbor".to_string(),
        },
        Ok(json!({"root": "/srv", "matches": []})),
    );
    app.show_session(generation_of(&app, "synthetic amber"));
    app.start_file_search("harbor".to_string());
    let on_amber = searched_in(&mut events);
    app.on_host_file_search_landed(
        crate::host_client::FileSearch {
            session_id: Some(AMBER.to_string()),
            query: "harbor".to_string(),
        },
        Ok(json!({"root": "/srv", "matches": []})),
    );
    app.show_session(generation_of(&app, "synthetic birch"));
    app.start_file_search("harbor".to_string());
    let on_birch = searched_in(&mut events);

    assert_eq!(
        (with_no_session, on_amber, on_birch),
        (
            vec![None],
            vec![Some(AMBER.to_string())],
            vec![Some(BIRCH.to_string())]
        )
    );
}

#[tokio::test]
async fn an_at_mention_typed_again_after_its_answer_asks_the_host_again() {
    let home = tempfile::tempdir().unwrap();
    let finding = HostConnection {
        features: HostFeatures {
            find_paths: true,
            ..HostFeatures::default()
        },
        ..connection()
    };
    let (mut app, mut events) = joined_app_on(&home, None, finding.clone());
    app.on_joined_sessions(
        list(&[row(AMBER, "synthetic amber")]),
        FrameRequester::test_dummy(),
        &finding,
    );
    let amber = app
        .session_rows()
        .into_iter()
        .find(|row| row.title.contains("synthetic amber"))
        .map(|row| row.generation)
        .unwrap();
    app.show_session(amber);
    let _ = searched_in(&mut events);

    let mut asked = Vec::new();
    for _ in 0..2 {
        app.start_file_search("harbor".to_string());
        while let Ok(event) = events.try_recv() {
            if let crate::app_event::AppEvent::HostRequest {
                call:
                    crate::host_client::HostCall::FindPaths {
                        session_id, query, ..
                    },
                ..
            } = event
            {
                asked.push((session_id, query));
            }
        }
        app.on_host_file_search_landed(
            crate::host_client::FileSearch {
                session_id: Some(AMBER.to_string()),
                query: "harbor".to_string(),
            },
            Ok(json!({"root": "/srv", "matches": [{"path": "harbor.md"}]})),
        );
    }

    assert_eq!(
        asked,
        vec![(Some(AMBER.to_string()), "harbor".to_string()); 2]
    );
}
