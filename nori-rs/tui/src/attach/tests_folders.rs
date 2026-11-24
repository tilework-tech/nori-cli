/// A controlling runtime connected to a host, launched from `cwd`, whose
/// `initialize` reply carries `capabilities`.
fn connected_runtime(
    cwd: &std::path::Path,
    capabilities: serde_json::Value,
) -> (
    super::runtime::Runtime,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    use crate::host_client::ConnectionEvent;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(None, tx);
    runtime.cwd = cwd.to_path_buf();
    runtime
        .connection_event(ConnectionEvent::Connected)
        .unwrap();
    let initialize = rx.try_recv().unwrap();
    runtime
        .connection_event(ConnectionEvent::Message(json!({
            "jsonrpc": "2.0",
            "id": initialize["id"],
            "result": {"capabilities": capabilities, "control": {"role": "controller"}},
        })))
        .unwrap();
    while rx.try_recv().is_ok() {}
    (runtime, rx)
}

fn submit(runtime: &mut super::runtime::Runtime, text: &str) {
    use crossterm::event::KeyEvent;
    runtime.composer.handle_paste(text.into());
    runtime.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

fn press_tab(runtime: &mut super::runtime::Runtime) {
    use crossterm::event::KeyEvent;
    runtime.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
}

fn folder_host() -> serde_json::Value {
    json!({"nori_sessions": {"version": 1, "session_cwd": true}})
}

#[test]
fn new_session_starts_in_the_attaching_folder_or_the_named_one() {
    let terminal = tempfile::tempdir().unwrap();
    let other = terminal.path().join("other");
    std::fs::create_dir(&other).unwrap();
    let (mut runtime, mut rx) = connected_runtime(terminal.path(), folder_host());
    let mut new_params = |text: &str| {
        submit(&mut runtime, text);
        let request = rx
            .try_recv()
            .unwrap_or_else(|_| panic!("{text} sends a request"));
        assert_eq!(request["method"], "_nori/session/new", "{text}");
        request["params"].clone()
    };
    let here = terminal.path().display().to_string();
    let there = other.display().to_string();
    assert_eq!(new_params("/new"), json!({"cwd": here}));
    assert_eq!(new_params("/new other"), json!({"cwd": there}));
    assert_eq!(new_params("/new ./other/"), json!({"cwd": there}));
    assert_eq!(
        new_params("/new other custom-agent"),
        json!({"cwd": there, "agent": "custom-agent"})
    );
    assert_eq!(
        new_params("/new custom-agent"),
        json!({"cwd": here, "agent": "custom-agent"})
    );
    let home = dirs::home_dir().unwrap().display().to_string();
    assert_eq!(new_params("/new ~"), json!({"cwd": home}));
}

#[test]
fn a_named_host_starts_plain_new_sessions_in_its_own_folder() {
    let terminal = tempfile::tempdir().unwrap();
    std::fs::create_dir(terminal.path().join("other")).unwrap();
    let (mut runtime, mut rx) = connected_runtime(terminal.path(), folder_host());
    runtime.remote = true;
    submit(&mut runtime, "/new");
    assert_eq!(rx.try_recv().unwrap()["params"], json!({}));
    submit(&mut runtime, "/new ./other");
    assert_eq!(
        rx.try_recv().unwrap()["params"],
        json!({"cwd": terminal.path().join("other").display().to_string()})
    );
    // This machine's folders say nothing about the host's, so a bare word
    // names an agent.
    submit(&mut runtime, "/new other");
    assert_eq!(rx.try_recv().unwrap()["params"], json!({"agent": "other"}));
}

#[test]
fn a_host_without_session_folders_starts_plain_sessions_and_refuses_a_folder() {
    let terminal = tempfile::tempdir().unwrap();
    std::fs::create_dir(terminal.path().join("other")).unwrap();
    let (mut runtime, mut rx) =
        connected_runtime(terminal.path(), json!({"nori_sessions": {"version": 1}}));
    submit(&mut runtime, "/new");
    assert_eq!(rx.try_recv().unwrap()["params"], json!({}));
    submit(&mut runtime, "/new custom-agent");
    assert_eq!(
        rx.try_recv().unwrap()["params"],
        json!({"agent": "custom-agent"})
    );
    submit(&mut runtime, "/new other");
    assert!(
        rx.try_recv().is_err(),
        "a folder the host cannot honour is not sent"
    );
    assert!(
        screen(&runtime.state).contains("does not support session folders"),
        "{}",
        screen(&runtime.state)
    );
}

#[test]
fn tab_completes_the_new_session_folder_like_a_shell() {
    let terminal = tempfile::tempdir().unwrap();
    for folder in ["alpha", "alps", "other", ".hidden", "al pine"] {
        std::fs::create_dir(terminal.path().join(folder)).unwrap();
    }
    std::fs::write(terminal.path().join("otherfile"), "").unwrap();
    let (mut runtime, _rx) = connected_runtime(terminal.path(), folder_host());

    runtime.composer.handle_paste("/new ot".into());
    press_tab(&mut runtime);
    assert_eq!(runtime.composer.text(), "/new other/");

    runtime.composer.clear();
    runtime.composer.handle_paste("/new al".into());
    press_tab(&mut runtime);
    assert_eq!(runtime.composer.text(), "/new alp");
    let rendered = screen(&runtime.state);
    assert!(
        rendered.contains("alpha/") && rendered.contains("alps/"),
        "ambiguous candidates are listed: {rendered}"
    );
    assert!(!rendered.contains(".hidden"), "{rendered}");

    runtime.composer.clear();
    runtime.composer.handle_paste("/new .h".into());
    press_tab(&mut runtime);
    assert_eq!(runtime.composer.text(), "/new .hidden/");
}

fn find_paths_host() -> serde_json::Value {
    json!({"nori_sessions": {"version": 1, "session_cwd": true, "find_paths": true}})
}

/// Put session `one`, working in `cwd`, on screen.
fn select_session(runtime: &mut super::runtime::Runtime, cwd: &str) {
    runtime
        .state
        .replace_sessions(json!({"sessions":[{
            "session_id":"one","agent":"mock-model","status":"ready","cwd":cwd
        }]}))
        .unwrap();
    runtime.state.selected = Some("one".into());
    runtime.state.list_open = false;
}

/// The one request sent since the last call.
fn sent(rx: &mut tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>) -> serde_json::Value {
    let mut requests: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(requests.len(), 1, "{requests:#?}");
    requests.remove(0)
}

fn reply(runtime: &mut super::runtime::Runtime, request: &serde_json::Value, result: serde_json::Value) {
    runtime
        .connection_event(crate::host_client::ConnectionEvent::Message(json!({
            "jsonrpc": "2.0", "id": request["id"], "result": result,
        })))
        .unwrap();
}

#[test]
fn tab_completes_the_new_session_folder_from_the_selected_sessions_folder_on_the_host() {
    // Nothing exists in the attaching folder: completions come from the host.
    let terminal = tempfile::tempdir().unwrap();
    let (mut runtime, mut rx) = connected_runtime(terminal.path(), find_paths_host());
    select_session(&mut runtime, "/srv/parser");

    runtime.composer.handle_paste("/new ot".into());
    press_tab(&mut runtime);
    let request = sent(&mut rx);
    assert_eq!(
        (request["method"].clone(), request["params"].clone()),
        (
            json!("_nori/session/find_paths"),
            json!({"sessionId": "one", "query": "ot", "kind": "folders", "limit": 50})
        )
    );
    assert_eq!(runtime.composer.text(), "/new ot");
    reply(&mut runtime, &request, json!({"root": "/srv/parser", "matches": [{"path": "other/"}]}));
    assert_eq!(runtime.composer.text(), "/new other/");

    runtime.composer.clear();
    runtime.composer.handle_paste("/new src/al".into());
    press_tab(&mut runtime);
    let request = sent(&mut rx);
    reply(
        &mut runtime,
        &request,
        json!({"root": "/srv/parser", "matches": [{"path": "src/alpha/"}, {"path": "src/alps/"}]}),
    );
    assert_eq!(runtime.composer.text(), "/new src/alp");
    let rendered = screen(&runtime.state);
    assert!(
        rendered.contains("alpha/  alps/"),
        "ambiguous candidates are listed: {rendered}"
    );
}

#[test]
fn a_folder_completion_is_dropped_when_the_text_changed_meanwhile() {
    let terminal = tempfile::tempdir().unwrap();
    let (mut runtime, mut rx) = connected_runtime(terminal.path(), find_paths_host());
    runtime.composer.handle_paste("/new ot".into());
    press_tab(&mut runtime);
    let request = sent(&mut rx);
    assert_eq!(request["params"]["sessionId"], serde_json::Value::Null);
    runtime.composer.handle_paste("x".into());
    reply(&mut runtime, &request, json!({"root": "/srv", "matches": [{"path": "other/"}]}));
    assert_eq!(runtime.composer.text(), "/new otx");
}

#[test]
fn at_mentions_list_files_from_the_selected_sessions_folder_on_the_host() {
    let terminal = tempfile::tempdir().unwrap();
    let (mut runtime, mut rx) = connected_runtime(terminal.path(), find_paths_host());
    select_session(&mut runtime, "/srv/parser");
    type_keys(&mut runtime, "@lex");
    // The event loop sends what the last flushed key typed.
    runtime.search_files();
    let requests: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert_eq!(
        requests
            .iter()
            .map(|request| request["params"].clone())
            .collect::<Vec<_>>(),
        vec![json!({"sessionId": "one", "query": "l", "kind": "files", "limit": 20})],
        "one search is in flight; later queries wait for it"
    );
    reply(
        &mut runtime,
        &requests[0],
        json!({"root": "/srv/parser", "matches": [{"path": "src/lexer.rs", "indices": [4]}]}),
    );
    let request = sent(&mut rx);
    assert_eq!(
        request["params"],
        json!({"sessionId": "one", "query": "lex", "kind": "files", "limit": 20}),
        "only the newest query follows the answered one"
    );
    reply(
        &mut runtime,
        &request,
        json!({"root": "/srv/parser", "matches": [{"path": "src/lexer.rs", "indices": [4, 5, 6]}]}),
    );
    let rendered = runtime_screen(&runtime, 18);
    assert!(rendered.contains("src/lexer.rs"), "{rendered}");
    assert!(rx.try_recv().is_err());
}

#[test]
fn at_mentions_send_nothing_to_a_host_without_find_paths() {
    let terminal = tempfile::tempdir().unwrap();
    let (mut runtime, mut rx) = connected_runtime(terminal.path(), folder_host());
    select_session(&mut runtime, "/srv/parser");
    type_keys(&mut runtime, "@lex");
    runtime.search_files();
    assert_eq!(sent_methods(&mut rx), Vec::<String>::new());
}

#[test]
fn new_session_starts_in_the_selected_sessions_folder() {
    let terminal = tempfile::tempdir().unwrap();
    let session = tempfile::tempdir().unwrap();
    std::fs::create_dir(session.path().join("sub")).unwrap();
    let folder = session.path().display().to_string();
    let (mut runtime, mut rx) = connected_runtime(terminal.path(), find_paths_host());
    select_session(&mut runtime, &folder);
    let mut new_params = |runtime: &mut super::runtime::Runtime, text: &str| {
        submit(runtime, text);
        let request = sent(&mut rx);
        assert_eq!(request["method"], "_nori/session/new", "{text}");
        request["params"].clone()
    };
    let sub = session.path().join("sub").display().to_string();
    assert_eq!(new_params(&mut runtime, "/new"), json!({"cwd": folder}));
    assert_eq!(new_params(&mut runtime, "/new sub"), json!({"cwd": sub}));
    assert_eq!(
        new_params(&mut runtime, "/new custom-agent"),
        json!({"cwd": folder, "agent": "custom-agent"})
    );

    // A named host's session folder is not on this machine: a lone word
    // is a folder only when it looks like a path.
    runtime.remote = true;
    select_session(&mut runtime, "/srv/parser");
    assert_eq!(new_params(&mut runtime, "/new"), json!({"cwd": "/srv/parser"}));
    assert_eq!(
        new_params(&mut runtime, "/new sub"),
        json!({"cwd": "/srv/parser", "agent": "sub"})
    );
    assert_eq!(new_params(&mut runtime, "/new sub/"), json!({"cwd": "/srv/parser/sub"}));
    assert_eq!(new_params(&mut runtime, "/new .."), json!({"cwd": "/srv/parser/.."}));
}

#[test]
fn the_header_shows_the_sessions_folder() {
    let mut state = AttachState::default();
    state
        .replace_sessions(json!({"sessions":[{
            "session_id":"one","agent":"mock-model","status":"ready",
            "title":"Fix the parser","cwd":"/work/parser-service",
            "reply_preview":"Patched the lexer"
        }]}))
        .unwrap();
    state.selected = Some("one".into());
    state.list_open = false;
    insta::assert_snapshot!("attached_session_folder_header", screen(&state));
}

#[test]
fn a_host_serving_find_paths_names_home_and_its_own_folders_itself() {
    let terminal = tempfile::tempdir().unwrap();
    let (mut runtime, mut rx) = connected_runtime(terminal.path(), find_paths_host());
    runtime.remote = true;
    let mut new_params = |runtime: &mut super::runtime::Runtime, text: &str| {
        submit(runtime, text);
        let request = sent(&mut rx);
        assert_eq!(request["method"], "_nori/session/new", "{text}");
        request["params"].clone()
    };
    // `~` is the host's home, which this machine cannot know.
    assert_eq!(new_params(&mut runtime, "/new ~"), json!({"cwd": "~"}));
    assert_eq!(
        new_params(&mut runtime, "/new ~/orchard"),
        json!({"cwd": "~/orchard"})
    );
    // With no session on screen, a relative folder is under the host's own.
    assert_eq!(
        new_params(&mut runtime, "/new ./greenhouse"),
        json!({"cwd": "./greenhouse"})
    );

    select_session(&mut runtime, "/srv/parser");
    assert_eq!(
        new_params(&mut runtime, "/new ~/orchard custom-agent"),
        json!({"cwd": "~/orchard", "agent": "custom-agent"})
    );
    assert_eq!(new_params(&mut runtime, "/new sub/"), json!({"cwd": "/srv/parser/sub"}));

    // A local host shares this machine's home but still names it itself.
    runtime.remote = false;
    assert_eq!(new_params(&mut runtime, "/new ~"), json!({"cwd": "~"}));
}

#[test]
fn an_older_named_host_gets_home_expanded_by_the_terminal() {
    let terminal = tempfile::tempdir().unwrap();
    let (mut runtime, mut rx) = connected_runtime(terminal.path(), folder_host());
    runtime.remote = true;
    submit(&mut runtime, "/new ~/orchard");
    let home = dirs::home_dir().unwrap().join("orchard");
    assert_eq!(
        sent(&mut rx)["params"],
        json!({"cwd": home.display().to_string()})
    );
}

#[test]
fn a_cut_short_folder_list_completes_no_further_than_what_was_typed() {
    let terminal = tempfile::tempdir().unwrap();
    let (mut runtime, mut rx) = connected_runtime(terminal.path(), find_paths_host());
    select_session(&mut runtime, "/srv/parser");
    runtime.composer.handle_paste("/new ".into());
    press_tab(&mut runtime);
    let request = sent(&mut rx);
    let limit = request["params"]["limit"].as_u64().unwrap();
    // The host sorts before cutting the list short, so a `b…` folder past
    // the limit shares nothing with these.
    let matches: Vec<_> = (0..limit)
        .map(|index| json!({"path": format!("apple{index:02}/")}))
        .collect();
    reply(&mut runtime, &request, json!({"root": "/srv/parser", "matches": matches}));
    assert_eq!(runtime.composer.text(), "/new ");
    let rendered = screen(&runtime.state);
    assert!(rendered.contains("apple00/"), "{rendered}");
}

#[test]
fn a_malformed_file_search_reply_does_not_stop_later_searches() {
    let terminal = tempfile::tempdir().unwrap();
    let (mut runtime, mut rx) = connected_runtime(terminal.path(), find_paths_host());
    select_session(&mut runtime, "/srv/parser");
    type_keys(&mut runtime, "@l");
    runtime.search_files();
    let request = sent(&mut rx);
    let _ = runtime.connection_event(crate::host_client::ConnectionEvent::Message(json!({
        "jsonrpc": "2.0", "id": request["id"], "result": {"matches": "not a list"},
    })));
    type_keys(&mut runtime, "e");
    runtime.search_files();
    assert_eq!(
        sent(&mut rx)["params"]["query"],
        json!("le"),
        "the next query is still sent"
    );
}
