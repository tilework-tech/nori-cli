/// A controlling runtime showing session `one`, which works in `/work/one`.
fn runtime_on_session_one() -> (
    super::runtime::Runtime,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    use super::connection::ConnectionEvent;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("one".into()), tx);
    runtime
        .connection_event(ConnectionEvent::Connected)
        .unwrap();
    let initialize = rx.try_recv().unwrap();
    runtime
        .connection_event(ConnectionEvent::Message(json!({
            "jsonrpc": "2.0",
            "id": initialize["id"],
            "result": {"capabilities": {"nori_sessions": {"session_cwd": true}}, "control": {"role": "controller"}},
        })))
        .unwrap();
    runtime
        .state
        .replace_sessions(json!({"sessions":[{"session_id":"one","agent":"mock-model","status":"failed","title":"One","cwd":"/work/one"}]}))
        .unwrap();
    while rx.try_recv().is_ok() {}
    (runtime, rx)
}

fn sent_for(runtime: &mut super::runtime::Runtime, rx: &mut tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>, command: &str) -> serde_json::Value {
    use crossterm::event::KeyEvent;
    runtime.composer.handle_paste(command.into());
    runtime.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    std::iter::from_fn(|| rx.try_recv().ok())
        .find(|message| message["method"] != "_nori/session/list")
        .unwrap_or_else(|| panic!("{command} sent a request"))
}

#[test]
fn session_commands_use_standard_acp_methods() {
    let (mut runtime, mut rx) = runtime_on_session_one();

    let close = sent_for(&mut runtime, &mut rx, "/close");
    let cancel = sent_for(&mut runtime, &mut rx, "/cancel");
    let resume = sent_for(&mut runtime, &mut rx, "/resume");

    assert_eq!(close["method"], "session/close");
    assert_eq!(close["params"], json!({"sessionId": "one"}));
    assert_eq!(cancel["method"], "session/cancel");
    assert_eq!(cancel["params"], json!({"sessionId": "one"}));
    assert!(cancel.get("id").is_none(), "cancel is a notification: {cancel}");
    assert_eq!(resume["method"], "session/resume");
    assert_eq!(
        resume["params"],
        json!({"sessionId": "one", "cwd": "/work/one", "mcpServers": []})
    );
}

#[test]
fn a_host_request_is_never_taken_for_the_reply_to_a_command() {
    use super::connection::ConnectionEvent;
    let (mut runtime, mut rx) = runtime_on_session_one();
    let close = sent_for(&mut runtime, &mut rx, "/close");

    runtime
        .connection_event(ConnectionEvent::Message(json!({
            "jsonrpc": "2.0",
            "id": close["id"],
            "method": "session/request_permission",
            "params": {"sessionId": "one", "toolCall": {"toolCallId": "t"}, "options": []},
        })))
        .unwrap();
    runtime
        .connection_event(ConnectionEvent::Message(json!({
            "jsonrpc": "2.0",
            "id": close["id"],
            "error": {"code": -32603, "message": "close went wrong"},
        })))
        .unwrap();

    assert!(
        screen(&runtime.state).contains("close went wrong"),
        "the real reply still reaches its command: {}",
        screen(&runtime.state)
    );
}
