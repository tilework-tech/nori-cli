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
    }
}

fn joined_app(home: &tempfile::TempDir, open_session: Option<&str>) -> App {
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
    let (app, _events) = App::joined(
        FrameRequester::test_dummy(),
        false,
        Arc::clone(&auth_manager),
        config,
        connection(),
        open_session.map(str::to_owned),
        None,
    );
    app
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
