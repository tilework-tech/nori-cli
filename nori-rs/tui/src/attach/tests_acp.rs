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
    let mut prompting = message("one", 1, "");
    prompting.event = nori_protocol::SessionEvent::Nori(
        nori_protocol::NoriEvent::SessionPhaseChanged(nori_protocol::SessionPhase::Prompting {
            request_id: nori_protocol::acp::v1::RequestId::Str("owned".into()),
        }),
    );
    runtime.state.apply(prompting).unwrap();
    let cancel = sent_for(&mut runtime, &mut rx, "/cancel");
    let resume = sent_for(&mut runtime, &mut rx, "/resume");

    assert_eq!(close["method"], "session/close");
    assert_eq!(close["params"], json!({"sessionId": "one"}));
    assert_eq!(cancel["method"], "session/cancel");
    assert_eq!(cancel["params"], json!({"sessionId": "one"}));
    assert!(
        cancel.get("id").is_none(),
        "cancel is a notification: {cancel}"
    );
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

fn external_change(
    session: &str,
    seq: i64,
    id: &str,
    state: nori_protocol::ExternalTurnState,
) -> SessionEnvelope {
    SessionEnvelope {
        conversation_id: session.into(),
        agent: "test-agent".into(),
        seq,
        event: nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ExternalTurnChanged(
            nori_protocol::ExternalTurn {
                id: id.into(),
                state,
            },
        )),
    }
}

#[test]
fn attached_external_turn_shows_cancel_progress_and_only_accepts_its_own_completion() {
    use nori_protocol::ExternalTurnState::*;
    let (mut runtime, _rx) = runtime_on_session_one();
    let state = &mut runtime.state;
    state
        .apply(message("one", 1, "Checking the sample files."))
        .unwrap();
    state
        .apply(external_change("one", 2, "a", Working))
        .unwrap();
    state
        .apply(external_change("one", 3, "a", CancelRequested))
        .unwrap();
    assert!(screen(state).contains("Cancel requested"));
    insta::assert_snapshot!("attached_external_cancel_requested", screen(state));
    state
        .apply(external_change("one", 4, "a", Unconfirmed))
        .unwrap();
    assert!(screen(state).contains("Cancel requested · unconfirmed"));
    state
        .apply(external_change("two", 1, "a", Cancelled))
        .unwrap();
    state
        .apply(external_change("one", 5, "older", Cancelled))
        .unwrap();
    assert!(!screen(state).contains("confirmed cancellation"));
    state.apply(external_change("one", 6, "a", Idle)).unwrap();
    assert!(screen(state).contains("Agent is idle. Cancellation was not confirmed."));
    state
        .apply(external_change("one", 7, "b", Working))
        .unwrap();
    state
        .apply(external_change("one", 8, "b", CancelRequested))
        .unwrap();
    state
        .apply(external_change("one", 9, "b", Cancelled))
        .unwrap();
    assert!(screen(state).contains("Agent confirmed cancellation."));
    assert!(!screen(state).contains("Cancel requested"));
}

#[test]
fn attached_external_cancel_respects_role_and_observed_activity() {
    use crossterm::event::KeyEvent;
    use nori_protocol::ExternalTurnState::*;
    let (mut runtime, mut rx) = runtime_on_session_one();
    let interrupt = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    runtime
        .state
        .apply(external_change("one", 1, "sample", Working))
        .unwrap();
    runtime.composer.set_text("Keep my draft".into());
    runtime.state.role_changed("watcher");
    runtime.key(interrupt);
    assert!(rx.try_recv().is_err());
    assert_eq!(runtime.composer.text(), "Keep my draft");
    runtime.state.role_changed("controller");
    runtime.key(interrupt);
    let cancel = rx
        .try_recv()
        .expect("Ctrl+C interrupts even when the composer contains a draft");
    assert_eq!(cancel["method"], "_nori/session/cancel_observed");
    assert_eq!(
        cancel["params"],
        json!({"sessionId":"one","observedTurnId":"sample"})
    );
    assert!(cancel["id"].is_number());
    assert_eq!(runtime.composer.text(), "Keep my draft");
    assert!(screen(&runtime.state).contains("Cancel requested · unconfirmed"));
    for (seq, status) in [(2, CancelRequested), (3, Unconfirmed)] {
        runtime.key(interrupt);
        assert!(rx.try_recv().is_err(), "duplicate key must stay local");
        runtime
            .state
            .apply(external_change("one", seq, "sample", status))
            .unwrap();
        assert_eq!(runtime.composer.text(), "Keep my draft");
    }
    runtime.key(interrupt);
    assert!(rx.try_recv().is_err());
    assert_eq!(runtime.composer.text(), "Keep my draft");
    runtime
        .state
        .apply(external_change("one", 4, "sample", Idle))
        .unwrap();
    runtime.composer.clear();
    runtime.key(interrupt);
    assert!(rx.try_recv().is_err());
    assert!(screen(&runtime.state).contains("No active turn to cancel"));
}

#[test]
fn attach_can_cancel_its_just_submitted_prompt_before_phase_delivery() {
    let (mut runtime, mut rx) = runtime_on_session_one();
    let prompt = sent_for(&mut runtime, &mut rx, "Check the sample files.");
    assert_eq!(prompt["method"], "_nori/session/prompt");
    let cancel = sent_for(&mut runtime, &mut rx, "/cancel");
    assert_eq!(cancel["method"], "session/cancel");
    assert_eq!(cancel["params"], json!({"sessionId":"one"}));
}

#[test]
fn reconnect_without_a_prompt_phase_leaves_the_attached_session_idle() {
    use super::connection::ConnectionEvent;
    use crossterm::event::KeyEvent;
    let (mut runtime, mut rx) = runtime_on_session_one();
    sent_for(&mut runtime, &mut rx, "Check these examples.");
    runtime
        .connection_event(ConnectionEvent::Disconnected("offline".into()))
        .unwrap();
    runtime
        .connection_event(ConnectionEvent::Connected)
        .unwrap();
    let initialize = rx.try_recv().unwrap();
    runtime
        .connection_event(ConnectionEvent::Message(json!({
            "jsonrpc":"2.0", "id":initialize["id"], "result":{"control":{"role":"controller"}}
        })))
        .unwrap();
    while rx.try_recv().is_ok() {}
    runtime.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(rx.try_recv().is_err());
    assert!(screen(&runtime.state).contains("No active turn to cancel."));
}

#[test]
fn a_second_attached_interrupt_keeps_an_admitted_cancellation_pending() {
    let (mut runtime, mut rx) = runtime_on_session_one();
    let mut envelope = message("one", 1, "");
    envelope.event = nori_protocol::SessionEvent::Nori(
        nori_protocol::NoriEvent::SessionPhaseChanged(nori_protocol::SessionPhase::Cancelling {
            request_id: nori_protocol::acp::v1::RequestId::Str("local".into()),
        }),
    );
    runtime.state.apply(envelope).unwrap();
    runtime.key(crossterm::event::KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    assert!(rx.try_recv().is_err());
    assert!(screen(&runtime.state).contains("Cancel requested"));
    assert!(!screen(&runtime.state).contains("No active turn"));
    assert!(!screen(&runtime.state).contains("Ready. Send a prompt"));
}

#[test]
fn external_cancel_replies_are_bound_to_the_observation_and_never_fall_back() {
    use super::connection::ConnectionEvent;
    use nori_protocol::ExternalTurnState::*;
    let (mut runtime, mut rx) = runtime_on_session_one();
    runtime
        .state
        .apply(external_change("one", 1, "first", Working))
        .unwrap();
    let first = sent_for(&mut runtime, &mut rx, "/cancel");
    runtime
        .state
        .apply(external_change("one", 2, "first", Superseded))
        .unwrap();
    runtime
        .state
        .apply(external_change("one", 3, "second", Working))
        .unwrap();
    runtime
        .connection_event(ConnectionEvent::Message(json!({
            "id":first["id"], "result":{"sessionId":"one"}
        })))
        .unwrap();
    assert!(!screen(&runtime.state).contains("Cancel requested"));
    let second = sent_for(&mut runtime, &mut rx, "/cancel");
    assert_eq!(second["params"]["observedTurnId"], "second");
    runtime
        .connection_event(ConnectionEvent::Message(json!({
            "id":second["id"], "error":{"code":-32601, "message":"Method not found"}
        })))
        .unwrap();
    assert!(
        rx.try_recv().is_err(),
        "an unsupported host must never get an unscoped interrupt"
    );
    assert!(screen(&runtime.state).contains("Method not found"));
    assert!(!screen(&runtime.state).contains("Cancel requested"));
}

#[test]
fn interrupted_external_cancel_transport_allows_a_scoped_manual_retry_after_reconnect() {
    use super::connection::ConnectionEvent;
    use nori_protocol::ExternalTurnState::*;
    let (mut runtime, mut rx) = runtime_on_session_one();
    runtime
        .state
        .apply(external_change("one", 1, "retry", Working))
        .unwrap();
    let first = sent_for(&mut runtime, &mut rx, "/cancel");
    runtime
        .connection_event(ConnectionEvent::Disconnected("offline".into()))
        .unwrap();
    runtime
        .connection_event(ConnectionEvent::Connected)
        .unwrap();
    let initialize = rx.try_recv().unwrap();
    runtime
        .connection_event(ConnectionEvent::Message(json!({
            "id":initialize["id"], "result":{"control":{"role":"controller"}}
        })))
        .unwrap();
    for outbound in std::iter::from_fn(|| rx.try_recv().ok()) {
        assert_ne!(outbound["method"], "_nori/session/cancel_observed");
        assert_ne!(outbound["method"], "session/cancel");
    }
    assert!(screen(&runtime.state).contains("Cancel requested · unconfirmed"));
    let retry = sent_for(&mut runtime, &mut rx, "/cancel");
    assert_eq!(retry["method"], "_nori/session/cancel_observed");
    assert_eq!(
        retry["params"],
        json!({"sessionId":"one","observedTurnId":"retry"})
    );
    assert_ne!(retry["id"], first["id"]);
    runtime
        .connection_event(ConnectionEvent::Message(json!({
            "id":retry["id"], "result":{"sessionId":"one"}
        })))
        .unwrap();
    assert!(screen(&runtime.state).contains("Cancel requested"));
    assert!(!screen(&runtime.state).contains("Agent confirmed cancellation."));
}

#[test]
fn picker_interrupt_cannot_cancel_a_hidden_session_or_its_permission() {
    let (mut runtime, mut rx) = runtime_on_session_one();
    runtime
        .state
        .replace_sessions(json!({"sessions":[
            {"session_id":"one","agent":"sample","title":"First","status":"working"},
            {"session_id":"two","agent":"sample","title":"Second","status":"working"}
        ]}))
        .unwrap();
    for session in ["one", "two"] {
        runtime
            .state
            .apply(external_change(
                session,
                1,
                session,
                nori_protocol::ExternalTurnState::Working,
            ))
            .unwrap();
    }
    let mut permission = permission_event(2);
    permission.conversation_id = "one".into();
    runtime.state.apply(permission).unwrap();
    key(&mut runtime, KeyCode::Left, KeyModifiers::NONE);
    key(&mut runtime, KeyCode::Down, KeyModifiers::NONE);
    assert!(runtime.state.list_open);
    assert_eq!(runtime.state.selected.as_deref(), Some("one"));
    assert_eq!(runtime.state.picker.selected_item().unwrap().key, "two");
    while rx.try_recv().is_ok() {}
    key(&mut runtime, KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert!(
        rx.try_recv().is_err(),
        "the picker must never interrupt either session or answer the hidden permission"
    );
    assert_eq!(runtime.state.permission_count("one"), 1);
    assert!(runtime.state.list_open);
}
