use super::*;
use chrono::Duration;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use insta::assert_snapshot;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;
use std::future::Future;
use std::path::PathBuf;

fn metadata(session_id: &str, project_id: &str, cwd: &str, agent: &str) -> SessionMetadata {
    SessionMetadata {
        session_id: session_id.to_string(),
        project_id: project_id.to_string(),
        started_at: "2025-01-01T00:00:00Z".to_string(),
        cwd: PathBuf::from(cwd),
        agent: Some(agent.to_string()),
    }
}

fn row(session_id: &str, cwd: Option<PathBuf>) -> Row {
    Row {
        target: ResumeTarget {
            nori_home: PathBuf::from("/tmp/nori-home"),
            project_id: "project".to_string(),
            session_id: session_id.to_string(),
            agent: Some("codex".to_string()),
        },
        preview: session_id.to_string(),
        created_at: None,
        updated_at: None,
        cwd,
        git_branch: None,
    }
}

fn page(items: Vec<SessionMetadata>) -> TranscriptPage {
    TranscriptPage {
        items,
        num_scanned_files: 0,
    }
}

fn state_with_rows(rows: Vec<Row>, show_all: bool, filter_cwd: Option<PathBuf>) -> PickerState {
    let mut state = PickerState::new(
        PathBuf::from("/tmp/nori-home"),
        FrameRequester::test_dummy(),
        None,
        show_all,
        filter_cwd,
    );
    state.all_rows = rows.clone();
    state.filtered_rows = rows;
    state.apply_filter();
    state
}

fn block_on_future<F: Future<Output = T>, T>(future: F) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn rows_from_metadata_preserves_session_targets() {
    let rows = helpers::rows_from_items(
        vec![
            metadata("session-a", "project-a", "/tmp/a", "claude-code"),
            metadata("session-b", "project-b", "/tmp/b", "codex"),
        ],
        PathBuf::from("/tmp/nori-home"),
    );

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].target.session_id, "session-a");
    assert_eq!(rows[0].target.project_id, "project-a");
    assert_eq!(rows[0].target.agent.as_deref(), Some("claude-code"));
    assert_eq!(rows[1].target.session_id, "session-b");
}

#[test]
fn ingest_page_deduplicates_by_transcript_target() {
    let mut state = state_with_rows(Vec::new(), true, None);
    state.ingest_page(page(vec![
        metadata("session-a", "project-a", "/tmp/a", "codex"),
        metadata("session-a", "project-a", "/tmp/a", "codex"),
        metadata("session-b", "project-a", "/tmp/a", "codex"),
    ]));

    let sessions: Vec<_> = state
        .filtered_rows
        .iter()
        .map(|row| row.target.session_id.as_str())
        .collect();
    assert_eq!(sessions, vec!["session-a", "session-b"]);
}

#[test]
fn cwd_filter_hides_other_projects_unless_show_all() {
    let rows = vec![
        row("same", Some(PathBuf::from("/tmp/project"))),
        row("other", Some(PathBuf::from("/tmp/other"))),
    ];

    let filtered = state_with_rows(rows.clone(), false, Some(PathBuf::from("/tmp/project")));
    assert_eq!(filtered.filtered_rows.len(), 1);
    assert_eq!(filtered.filtered_rows[0].target.session_id, "same");

    let show_all = state_with_rows(rows, true, Some(PathBuf::from("/tmp/project")));
    assert_eq!(show_all.filtered_rows.len(), 2);
}

#[test]
fn search_filters_by_session_id() {
    let mut state = state_with_rows(
        vec![row("session-alpha", None), row("session-beta", None)],
        true,
        None,
    );

    state.set_query("beta".to_string());

    assert_eq!(state.filtered_rows.len(), 1);
    assert_eq!(state.filtered_rows[0].target.session_id, "session-beta");
}

#[test]
fn inactive_resume_picker_uses_jk_for_navigation() {
    let mut state = state_with_rows(
        vec![row("session-alpha", None), row("session-beta", None)],
        true,
        None,
    );

    block_on_future(async {
        state
            .handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE))
            .await
            .unwrap();
        state
            .handle_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE))
            .await
            .unwrap();
        state
            .handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE))
            .await
            .unwrap();
    });
    assert_eq!(state.selected, 1);
    assert_eq!(state.query, "");

    block_on_future(async {
        state
            .handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE))
            .await
            .unwrap();
    });
    assert_eq!(state.selected, 0);
    assert_eq!(state.query, "");
}

#[test]
fn resume_picker_supports_all_search_activation_keys() {
    let activation_keys = [
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
    ];

    for activation_key in activation_keys {
        let mut state = state_with_rows(vec![row("alpha", None), row("beta", None)], true, None);

        let activation = block_on_future(state.handle_key(activation_key)).unwrap();
        assert!(activation.is_none(), "activation key: {activation_key:?}");

        block_on_future(state.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)))
            .unwrap();
        assert_eq!(state.query, "b", "activation key: {activation_key:?}");

        let exit_search =
            block_on_future(state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
                .unwrap();
        assert!(exit_search.is_none(), "activation key: {activation_key:?}");
        assert_eq!(state.query, "", "activation key: {activation_key:?}");

        let dismiss =
            block_on_future(state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
                .unwrap();
        assert!(
            matches!(dismiss, Some(ResumeSelection::StartFresh)),
            "activation key: {activation_key:?}"
        );
    }
}

#[test]
fn empty_active_resume_search_exits_before_the_picker_dismisses() {
    let mut state = state_with_rows(vec![row("session", None)], true, None);

    block_on_future(state.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)))
        .unwrap();
    let exit_search =
        block_on_future(state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))).unwrap();
    assert!(exit_search.is_none());

    let dismiss =
        block_on_future(state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))).unwrap();
    assert!(matches!(dismiss, Some(ResumeSelection::StartFresh)));
}

#[test]
fn active_resume_search_receives_reserved_and_general_printable_characters() {
    let mut state = state_with_rows(vec![row("session", None)], true, None);

    block_on_future(state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)))
        .unwrap();
    for (character, modifiers) in [
        ('j', KeyModifiers::NONE),
        ('k', KeyModifiers::NONE),
        ('f', KeyModifiers::NONE),
        ('/', KeyModifiers::NONE),
        ('A', KeyModifiers::SHIFT),
        ('7', KeyModifiers::NONE),
        (' ', KeyModifiers::NONE),
        ('?', KeyModifiers::NONE),
        ('λ', KeyModifiers::NONE),
    ] {
        block_on_future(state.handle_key(KeyEvent::new(KeyCode::Char(character), modifiers)))
            .unwrap();
    }

    assert_eq!(state.query, "jkf/A7 ?λ");
}

#[test]
fn activated_resume_search_filters_before_selecting() {
    let mut state = state_with_rows(
        vec![row("session-alpha", None), row("session-beta", None)],
        true,
        None,
    );

    block_on_future(state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)))
        .unwrap();
    for character in "beta".chars() {
        block_on_future(
            state.handle_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE)),
        )
        .unwrap();
    }
    let selection =
        block_on_future(state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)))
            .unwrap();

    let Some(ResumeSelection::Resume(target)) = selection else {
        panic!("expected filtered resume selection");
    };
    assert_eq!(target.session_id, "session-beta");
}

#[test]
fn enter_selects_resume_target() {
    let mut state = state_with_rows(
        vec![row("session-alpha", None), row("session-beta", None)],
        true,
        None,
    );
    state.selected = 1;

    let selection = block_on_future(async {
        state
            .handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap()
    });

    match selection {
        Some(ResumeSelection::Resume(target)) => {
            assert_eq!(target.session_id, "session-beta");
            assert_eq!(target.agent.as_deref(), Some("codex"));
        }
        other => panic!("expected resume selection, got {other:?}"),
    }
}

#[test]
fn page_navigation_uses_view_rows() {
    let rows: Vec<_> = (0..20)
        .map(|idx| row(&format!("session-{idx}"), None))
        .collect();
    let mut state = state_with_rows(rows, true, None);
    state.update_view_rows(5);

    block_on_future(async {
        state
            .handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
            .await
            .unwrap();
    });
    assert_eq!(state.selected, 5);

    block_on_future(async {
        state
            .handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
            .await
            .unwrap();
    });
    assert_eq!(state.selected, 0);
}

#[test]
fn resume_table_snapshot() {
    use crate::custom_terminal::Terminal;
    use crate::test_backend::VT100Backend;

    let now = Utc::now();
    let rows = vec![
        Row {
            preview: String::from("session-a"),
            created_at: Some(now - Duration::minutes(16)),
            updated_at: Some(now - Duration::seconds(42)),
            cwd: None,
            git_branch: None,
            ..row("session-a", None)
        },
        Row {
            preview: String::from("session-b"),
            created_at: Some(now - Duration::hours(1)),
            updated_at: Some(now - Duration::minutes(35)),
            cwd: None,
            git_branch: None,
            ..row("session-b", None)
        },
    ];
    let mut state = state_with_rows(rows, true, None);
    state.view_rows = Some(2);
    state.selected = 1;

    let width: u16 = 80;
    let height: u16 = 13;
    let backend = VT100Backend::new(width, height);
    let mut terminal = Terminal::with_options(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, width, height));

    {
        let mut frame = terminal.get_frame();
        let component_state = rendering::component_state(&state);
        let area = frame.area();
        rendering::picker(&component_state).render(area, frame.buffer_mut());
    }
    terminal.flush().expect("flush");

    let snapshot = terminal
        .backend()
        .to_string()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    assert_snapshot!("resume_picker_table", snapshot);
}
