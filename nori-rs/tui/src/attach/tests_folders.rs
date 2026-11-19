/// A controlling runtime connected to a host, launched from `cwd`, whose
/// `initialize` reply carries `capabilities`.
fn connected_runtime(
    cwd: &std::path::Path,
    capabilities: serde_json::Value,
) -> (
    super::runtime::Runtime,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    use super::connection::ConnectionEvent;
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
    submit(&mut runtime, "/new other");
    assert_eq!(
        rx.try_recv().unwrap()["params"],
        json!({"cwd": terminal.path().join("other").display().to_string()})
    );
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
