fn listed_sessions() -> AttachState {
    let mut state = AttachState {
        connected: true,
        selected: Some("root-session".into()),
        ..Default::default()
    };
    state.role_changed("controller");
    state
        .replace_sessions(json!({"sessions":[
            {"session_id":"root-session","agent":"claude-code","status":"working","family_status":"working",
             "title":"Greeting in three words","reply_preview":"Hi there, friend! Anything else you would like me to help with today?",
             "model":"opus 5.5","idle_seconds":125,"context_used":38000,"context_size":475000},
            {"session_id":"child-session","agent":"codex","status":"working","title":"inspect the parser recovery path",
             "parent":"root-session","source":"session","reply_preview":"Reading src/parser.rs","model":"gpt-5.5",
             "idle_seconds":4,"context_used":84320,"context_size":272000},
            {"session_id":"fresh-session","agent":"gemini","status":"starting","title":"say hi in 3 words","idle_seconds":0}
        ]}))
        .unwrap();
    state
}

fn runtime_with_list() -> (
    super::runtime::Runtime,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(None, tx);
    runtime.state = listed_sessions();
    runtime.state.selected = None;
    runtime.state.list_open = true;
    (runtime, rx)
}

/// The title on the list row the marker points at.
fn marked_title(runtime: &super::runtime::Runtime) -> String {
    let rendered = screen_area(&runtime.state, &runtime.composer, 120, 24);
    rendered
        .lines()
        .find_map(|line| {
            line.strip_prefix("› ").map(|rest| {
                rest.split("  ")
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            })
        })
        .unwrap_or_else(|| panic!("a marked row in {rendered}"))
}

#[test]
fn j_and_k_move_through_the_attach_list_and_letters_do_not_filter() {
    let (mut runtime, _rx) = runtime_with_list();
    assert!(marked_title(&runtime).contains("Greeting in three words"));
    key(&mut runtime, KeyCode::Char('j'), KeyModifiers::NONE);
    assert!(
        marked_title(&runtime).contains("inspect the parser"),
        "j moves down"
    );
    key(&mut runtime, KeyCode::Char('k'), KeyModifiers::NONE);
    assert!(
        marked_title(&runtime).contains("Greeting in three words"),
        "k moves up"
    );
    key(&mut runtime, KeyCode::Char('x'), KeyModifiers::NONE);
    let rendered = screen_area(&runtime.state, &runtime.composer, 120, 24);
    for title in [
        "Greeting in three words",
        "inspect the parser",
        "say hi in 3 words",
    ] {
        assert!(
            rendered.contains(title),
            "a letter does not filter: {rendered}"
        );
    }
}

#[test]
fn with_a_draft_letters_edit_the_draft_and_arrows_move_the_list() {
    let (mut runtime, _rx) = runtime_with_list();
    runtime.composer.handle_paste("fix".into());
    for character in ['j', '/', 'k'] {
        key(&mut runtime, KeyCode::Char(character), KeyModifiers::NONE);
    }
    assert!(
        marked_title(&runtime).contains("Greeting in three words"),
        "letters left the list alone"
    );
    assert!(
        !runtime.state.picker.search_active,
        "/ was typed, not a search"
    );
    // The attach loop flushes quickly typed keys once they stop arriving.
    std::thread::sleep(crate::ComposerInput::recommended_flush_delay());
    runtime.composer.flush_paste_burst_if_due();
    let rendered = screen_area(&runtime.state, &runtime.composer, 120, 24);
    assert!(
        rendered.contains("fixj/k"),
        "the letters went to the draft: {rendered}"
    );
    key(&mut runtime, KeyCode::Down, KeyModifiers::NONE);
    assert!(
        marked_title(&runtime).contains("inspect the parser"),
        "down still moves"
    );
}

#[test]
fn slash_searches_the_attach_list_and_esc_leaves_search_before_closing() {
    let (mut runtime, _rx) = runtime_with_list();
    key(&mut runtime, KeyCode::Char('/'), KeyModifiers::NONE);
    for character in "say".chars() {
        key(&mut runtime, KeyCode::Char(character), KeyModifiers::NONE);
    }
    let rendered = screen_area(&runtime.state, &runtime.composer, 120, 24);
    assert!(rendered.contains("say hi in 3 words"), "{rendered}");
    assert!(
        !rendered.contains("inspect the parser"),
        "typing filters while searching: {rendered}"
    );
    assert!(
        runtime.composer.is_empty(),
        "search text stays out of the composer"
    );

    key(&mut runtime, KeyCode::Esc, KeyModifiers::NONE);
    assert!(runtime.state.list_open, "the first esc only leaves search");
    let rendered = screen_area(&runtime.state, &runtime.composer, 120, 24);
    assert!(
        rendered.contains("inspect the parser"),
        "the filter is gone: {rendered}"
    );

    key(&mut runtime, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!runtime.state.list_open, "the second esc closes the list");
}

#[test]
fn the_attach_list_hints_name_j_k_and_slash() {
    let (runtime, _rx) = runtime_with_list();
    let rendered = screen_area(&runtime.state, &runtime.composer, 120, 24);
    assert!(rendered.contains("↑↓/j/k move"), "{rendered}");
    assert!(rendered.contains("/ search"), "{rendered}");
}

#[test]
fn a_waiting_rows_second_line_says_what_it_asks_for() {
    let mut state = AttachState {
        connected: true,
        ..Default::default()
    };
    state.replace_sessions(json!({"sessions":[
        {"session_id":"asking","agent":"mock-model","status":"waiting_on_you","title":"Fix the parser",
         "reply_preview":"Let me run the tests first.","waiting_request":"approve: cargo test -p parser","idle_seconds":3},
        {"session_id":"answering","agent":"mock-model","status":"ready","title":"Spike transport",
         "reply_preview":"The transport spike is done.","waiting_request":"approve: stale","idle_seconds":9}
    ]})).unwrap();
    let rendered = screen_area(&state, &crate::ComposerInput::for_attachment(), 120, 24);
    let below = |title: &str| {
        let lines: Vec<&str> = rendered.lines().collect();
        let at = lines
            .iter()
            .position(|line| line.contains(title) && line.contains("mock-model"))
            .unwrap_or_else(|| panic!("{title} in {rendered}"));
        lines[at + 1].to_string()
    };
    assert!(
        below("Fix the parser").contains("approve: cargo test -p parser"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("Let me run the tests first."),
        "{rendered}"
    );
    assert!(
        below("Spike transport").contains("The transport spike is done."),
        "{rendered}"
    );
    assert!(
        !rendered.contains("approve: stale"),
        "only a waiting row shows its request: {rendered}"
    );
}

#[test]
fn the_attach_list_shows_two_rows_per_session_at_each_layout_width() {
    let state = listed_sessions();
    for width in [58, 80, 87, 120] {
        insta::assert_snapshot!(
            format!("attach_two_row_sessions_{width}"),
            screen_area(&state, &crate::ComposerInput::for_attachment(), width, 24)
        );
    }
}
