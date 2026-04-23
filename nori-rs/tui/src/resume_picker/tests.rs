use super::*;
use chrono::Duration;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use insta::assert_snapshot;
use serde_json::json;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

fn head_with_ts_and_user_text(ts: &str, texts: &[&str]) -> Vec<serde_json::Value> {
    vec![
        json!({ "timestamp": ts }),
        json!({
            "type": "message",
            "role": "user",
            "content": texts
                .iter()
                .map(|t| json!({ "type": "input_text", "text": *t }))
                .collect::<Vec<_>>()
        }),
    ]
}

fn make_item(path: &str, ts: &str, preview: &str) -> ConversationItem {
    ConversationItem {
        path: PathBuf::from(path),
        head: head_with_ts_and_user_text(ts, &[preview]),
        tail: Vec::new(),
        created_at: Some(ts.to_string()),
        updated_at: Some(ts.to_string()),
    }
}

fn cursor_from_str(repr: &str) -> Cursor {
    serde_json::from_str::<Cursor>(&format!("\"{repr}\""))
        .expect("cursor format should deserialize")
}

fn page(
    items: Vec<ConversationItem>,
    next_cursor: Option<Cursor>,
    num_scanned_files: usize,
    reached_scan_cap: bool,
) -> ConversationsPage {
    ConversationsPage {
        items,
        next_cursor,
        num_scanned_files,
        reached_scan_cap,
    }
}

fn block_on_future<F: Future<Output = T>, T>(future: F) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn preview_uses_first_message_input_text() {
    let head = vec![
        json!({ "timestamp": "2025-01-01T00:00:00Z" }),
        json!({
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": "# AGENTS.md instructions for project\n\n<INSTRUCTIONS>\nhi\n</INSTRUCTIONS>" },
            ]
        }),
        json!({
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": "<environment_context>...</environment_context>" },
            ]
        }),
        json!({
            "type": "message",
            "role": "user",
            "content": [
                { "type": "input_text", "text": "real question" },
                { "type": "input_image", "image_url": "ignored" }
            ]
        }),
        json!({
            "type": "message",
            "role": "user",
            "content": [ { "type": "input_text", "text": "later text" } ]
        }),
    ];
    let preview = helpers::preview_from_head(&head);
    assert_eq!(preview.as_deref(), Some("real question"));
}

#[test]
fn rows_from_items_preserves_backend_order() {
    // Construct two items with different timestamps and real user text.
    let a = ConversationItem {
        path: PathBuf::from("/tmp/a.jsonl"),
        head: head_with_ts_and_user_text("2025-01-01T00:00:00Z", &["A"]),
        tail: Vec::new(),
        created_at: Some("2025-01-01T00:00:00Z".into()),
        updated_at: Some("2025-01-01T00:00:00Z".into()),
    };
    let b = ConversationItem {
        path: PathBuf::from("/tmp/b.jsonl"),
        head: head_with_ts_and_user_text("2025-01-02T00:00:00Z", &["B"]),
        tail: Vec::new(),
        created_at: Some("2025-01-02T00:00:00Z".into()),
        updated_at: Some("2025-01-02T00:00:00Z".into()),
    };
    let rows = helpers::rows_from_items(vec![a, b]);
    assert_eq!(rows.len(), 2);
    // Preserve the given order even if timestamps differ; backend already provides newest-first.
    assert!(rows[0].preview.contains('A'));
    assert!(rows[1].preview.contains('B'));
}

#[test]
fn row_uses_tail_timestamp_for_updated_at() {
    let head = head_with_ts_and_user_text("2025-01-01T00:00:00Z", &["Hello"]);
    let tail = vec![json!({
        "timestamp": "2025-01-01T01:00:00Z",
        "type": "message",
        "role": "assistant",
        "content": [
            {
                "type": "output_text",
                "text": "hi",
            }
        ],
    })];
    let item = ConversationItem {
        path: PathBuf::from("/tmp/a.jsonl"),
        head,
        tail,
        created_at: Some("2025-01-01T00:00:00Z".into()),
        updated_at: Some("2025-01-01T01:00:00Z".into()),
    };

    let rows = helpers::rows_from_items(vec![item]);
    let row = &rows[0];
    let expected_created = chrono::DateTime::parse_from_rfc3339("2025-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let expected_updated = chrono::DateTime::parse_from_rfc3339("2025-01-01T01:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    assert_eq!(row.created_at, Some(expected_created));
    assert_eq!(row.updated_at, Some(expected_updated));
}

#[test]
fn resume_table_snapshot() {
    use crate::custom_terminal::Terminal;
    use crate::test_backend::VT100Backend;
    use ratatui::layout::Constraint;
    use ratatui::layout::Layout;

    let loader: PageLoader = Arc::new(|_| {});
    let mut state = PickerState::new(
        PathBuf::from("/tmp"),
        FrameRequester::test_dummy(),
        loader,
        String::from("openai"),
        true,
        None,
    );

    let now = Utc::now();
    let rows = vec![
        Row {
            path: PathBuf::from("/tmp/a.jsonl"),
            preview: String::from("Fix resume picker timestamps"),
            created_at: Some(now - Duration::minutes(16)),
            updated_at: Some(now - Duration::seconds(42)),
            cwd: None,
            git_branch: None,
        },
        Row {
            path: PathBuf::from("/tmp/b.jsonl"),
            preview: String::from("Investigate lazy pagination cap"),
            created_at: Some(now - Duration::hours(1)),
            updated_at: Some(now - Duration::minutes(35)),
            cwd: None,
            git_branch: None,
        },
        Row {
            path: PathBuf::from("/tmp/c.jsonl"),
            preview: String::from("Explain the codebase"),
            created_at: Some(now - Duration::hours(2)),
            updated_at: Some(now - Duration::hours(2)),
            cwd: None,
            git_branch: None,
        },
    ];
    state.all_rows = rows.clone();
    state.filtered_rows = rows;
    state.view_rows = Some(3);
    state.selected = 1;
    state.scroll_top = 0;
    state.update_view_rows(3);

    let metrics = rendering::calculate_column_metrics(&state.filtered_rows, state.show_all);

    let width: u16 = 80;
    let height: u16 = 6;
    let backend = VT100Backend::new(width, height);
    let mut terminal = Terminal::with_options(backend).expect("terminal");
    terminal.set_viewport_area(Rect::new(0, 0, width, height));

    {
        let mut frame = terminal.get_frame();
        let area = frame.area();
        let segments = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(area);
        rendering::render_column_headers(&mut frame, segments[0], &metrics);
        rendering::render_list(&mut frame, segments[1], &state, &metrics);
    }
    terminal.flush().expect("flush");

    let snapshot = terminal.backend().to_string();
    assert_snapshot!("resume_picker_table", snapshot);
}

#[test]
fn pageless_scrolling_deduplicates_and_keeps_order() {
    let loader: PageLoader = Arc::new(|_| {});
    let mut state = PickerState::new(
        PathBuf::from("/tmp"),
        FrameRequester::test_dummy(),
        loader,
        String::from("openai"),
        true,
        None,
    );

    state.reset_pagination();
    state.ingest_page(page(
        vec![
            make_item("/tmp/a.jsonl", "2025-01-03T00:00:00Z", "third"),
            make_item("/tmp/b.jsonl", "2025-01-02T00:00:00Z", "second"),
        ],
        Some(cursor_from_str(
            "2025-01-02T00-00-00|00000000-0000-0000-0000-000000000000",
        )),
        2,
        false,
    ));

    state.ingest_page(page(
        vec![
            make_item("/tmp/a.jsonl", "2025-01-03T00:00:00Z", "duplicate"),
            make_item("/tmp/c.jsonl", "2025-01-01T00:00:00Z", "first"),
        ],
        Some(cursor_from_str(
            "2025-01-01T00-00-00|00000000-0000-0000-0000-000000000001",
        )),
        2,
        false,
    ));

    state.ingest_page(page(
        vec![make_item(
            "/tmp/d.jsonl",
            "2024-12-31T23:00:00Z",
            "very old",
        )],
        None,
        1,
        false,
    ));

    let previews: Vec<_> = state
        .filtered_rows
        .iter()
        .map(|row| row.preview.as_str())
        .collect();
    assert_eq!(previews, vec!["third", "second", "first", "very old"]);

    let unique_paths = state
        .filtered_rows
        .iter()
        .map(|row| row.path.clone())
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(unique_paths.len(), 4);
}

#[test]
fn ensure_minimum_rows_prefetches_when_underfilled() {
    let recorded_requests: Arc<Mutex<Vec<PageLoadRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let request_sink = recorded_requests.clone();
    let loader: PageLoader = Arc::new(move |req: PageLoadRequest| {
        request_sink.lock().unwrap().push(req);
    });

    let mut state = PickerState::new(
        PathBuf::from("/tmp"),
        FrameRequester::test_dummy(),
        loader,
        String::from("openai"),
        true,
        None,
    );
    state.reset_pagination();
    state.ingest_page(page(
        vec![
            make_item("/tmp/a.jsonl", "2025-01-01T00:00:00Z", "one"),
            make_item("/tmp/b.jsonl", "2025-01-02T00:00:00Z", "two"),
        ],
        Some(cursor_from_str(
            "2025-01-03T00-00-00|00000000-0000-0000-0000-000000000000",
        )),
        2,
        false,
    ));

    assert!(recorded_requests.lock().unwrap().is_empty());
    state.ensure_minimum_rows_for_view(10);
    let guard = recorded_requests.lock().unwrap();
    assert_eq!(guard.len(), 1);
    assert!(guard[0].search_token.is_none());
}

#[test]
fn page_navigation_uses_view_rows() {
    let loader: PageLoader = Arc::new(|_| {});
    let mut state = PickerState::new(
        PathBuf::from("/tmp"),
        FrameRequester::test_dummy(),
        loader,
        String::from("openai"),
        true,
        None,
    );

    let mut items = Vec::new();
    for idx in 0..20 {
        let ts = format!("2025-01-{:02}T00:00:00Z", idx + 1);
        let preview = format!("item-{idx}");
        let path = format!("/tmp/item-{idx}.jsonl");
        items.push(make_item(&path, &ts, &preview));
    }

    state.reset_pagination();
    state.ingest_page(page(items, None, 20, false));
    state.update_view_rows(5);

    assert_eq!(state.selected, 0);
    block_on_future(async {
        state
            .handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
            .await
            .unwrap();
    });
    assert_eq!(state.selected, 5);

    block_on_future(async {
        state
            .handle_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE))
            .await
            .unwrap();
    });
    assert_eq!(state.selected, 10);

    block_on_future(async {
        state
            .handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE))
            .await
            .unwrap();
    });
    assert_eq!(state.selected, 5);
}

#[test]
fn up_at_bottom_does_not_scroll_when_visible() {
    let loader: PageLoader = Arc::new(|_| {});
    let mut state = PickerState::new(
        PathBuf::from("/tmp"),
        FrameRequester::test_dummy(),
        loader,
        String::from("openai"),
        true,
        None,
    );

    let mut items = Vec::new();
    for idx in 0..10 {
        let ts = format!("2025-02-{:02}T00:00:00Z", idx + 1);
        let preview = format!("item-{idx}");
        let path = format!("/tmp/item-{idx}.jsonl");
        items.push(make_item(&path, &ts, &preview));
    }

    state.reset_pagination();
    state.ingest_page(page(items, None, 10, false));
    state.update_view_rows(5);

    state.selected = state.filtered_rows.len().saturating_sub(1);
    state.ensure_selected_visible();

    let initial_top = state.scroll_top;
    assert_eq!(initial_top, state.filtered_rows.len().saturating_sub(5));

    block_on_future(async {
        state
            .handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .await
            .unwrap();
    });

    assert_eq!(state.scroll_top, initial_top);
    assert_eq!(state.selected, state.filtered_rows.len().saturating_sub(2));
}

#[test]
fn set_query_loads_until_match_and_respects_scan_cap() {
    let recorded_requests: Arc<Mutex<Vec<PageLoadRequest>>> = Arc::new(Mutex::new(Vec::new()));
    let request_sink = recorded_requests.clone();
    let loader: PageLoader = Arc::new(move |req: PageLoadRequest| {
        request_sink.lock().unwrap().push(req);
    });

    let mut state = PickerState::new(
        PathBuf::from("/tmp"),
        FrameRequester::test_dummy(),
        loader,
        String::from("openai"),
        true,
        None,
    );
    state.reset_pagination();
    state.ingest_page(page(
        vec![make_item(
            "/tmp/start.jsonl",
            "2025-01-01T00:00:00Z",
            "alpha",
        )],
        Some(cursor_from_str(
            "2025-01-02T00-00-00|00000000-0000-0000-0000-000000000000",
        )),
        1,
        false,
    ));
    recorded_requests.lock().unwrap().clear();

    state.set_query("target".to_string());
    let first_request = {
        let guard = recorded_requests.lock().unwrap();
        assert_eq!(guard.len(), 1);
        guard[0].clone()
    };

    state
        .handle_background_event(BackgroundEvent::PageLoaded {
            request_token: first_request.request_token,
            search_token: first_request.search_token,
            page: Ok(page(
                vec![make_item("/tmp/beta.jsonl", "2025-01-02T00:00:00Z", "beta")],
                Some(cursor_from_str(
                    "2025-01-03T00-00-00|00000000-0000-0000-0000-000000000001",
                )),
                5,
                false,
            )),
        })
        .unwrap();

    let second_request = {
        let guard = recorded_requests.lock().unwrap();
        assert_eq!(guard.len(), 2);
        guard[1].clone()
    };
    assert!(state.search_state.is_active());
    assert!(state.filtered_rows.is_empty());

    state
        .handle_background_event(BackgroundEvent::PageLoaded {
            request_token: second_request.request_token,
            search_token: second_request.search_token,
            page: Ok(page(
                vec![make_item(
                    "/tmp/match.jsonl",
                    "2025-01-03T00:00:00Z",
                    "target log",
                )],
                Some(cursor_from_str(
                    "2025-01-04T00-00-00|00000000-0000-0000-0000-000000000002",
                )),
                7,
                false,
            )),
        })
        .unwrap();

    assert!(!state.filtered_rows.is_empty());
    assert!(!state.search_state.is_active());

    recorded_requests.lock().unwrap().clear();
    state.set_query("missing".to_string());
    let active_request = {
        let guard = recorded_requests.lock().unwrap();
        assert_eq!(guard.len(), 1);
        guard[0].clone()
    };

    state
        .handle_background_event(BackgroundEvent::PageLoaded {
            request_token: second_request.request_token,
            search_token: second_request.search_token,
            page: Ok(page(Vec::new(), None, 0, false)),
        })
        .unwrap();
    assert_eq!(recorded_requests.lock().unwrap().len(), 1);

    state
        .handle_background_event(BackgroundEvent::PageLoaded {
            request_token: active_request.request_token,
            search_token: active_request.search_token,
            page: Ok(page(Vec::new(), None, 3, true)),
        })
        .unwrap();

    assert!(state.filtered_rows.is_empty());
    assert!(!state.search_state.is_active());
    assert!(state.pagination.reached_scan_cap);
}
