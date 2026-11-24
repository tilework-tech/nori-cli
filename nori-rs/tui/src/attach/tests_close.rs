/// The ledger matches refusals by text; the request ID is the adapter
/// connection's own and never seen by attach.
fn adapter_error(session: &str, seq: i64, request: i64, text: &str) -> SessionEnvelope {
    SessionEnvelope {
        conversation_id: session.into(),
        agent: "mock-model".into(),
        seq,
        event: nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
            request_id: nori_protocol::acp::v1::RequestId::Number(request),
            response: Err(nori_protocol::acp::v1::Error::new(-32004, text.to_string())),
        }),
    }
}

fn stream(runtime: &mut super::runtime::Runtime, event: SessionEnvelope) {
    runtime
        .connection_event(crate::host_client::ConnectionEvent::Message(json!({
            "jsonrpc":"2.0", "method":"_nori/session/event",
            "params":serde_json::to_value(event).unwrap()
        })))
        .unwrap();
}

fn refuse(runtime: &mut super::runtime::Runtime, close: &serde_json::Value, text: &str) {
    runtime
        .connection_event(crate::host_client::ConnectionEvent::Message(json!({
            "jsonrpc": "2.0",
            "id": close["id"],
            "error": {"code": -32603, "message": format!("Failed to close ACP session: {text}")},
        })))
        .unwrap();
}

#[derive(Debug, Clone, Copy)]
enum Arrival {
    AdapterFirst,
    ReplyFirst,
    ReplyOnly,
}

#[test]
fn a_refused_close_shows_one_refusal_whichever_report_arrives_first() {
    for arrival in [Arrival::AdapterFirst, Arrival::ReplyFirst, Arrival::ReplyOnly] {
        let (mut runtime, mut rx) = runtime_on_session_one();
        let close = sent_for(&mut runtime, &mut rx, "/close");
        match arrival {
            Arrival::AdapterFirst => {
                stream(&mut runtime, adapter_error("one", 1, 7, "sample refusal"));
                refuse(&mut runtime, &close, "sample refusal");
            }
            Arrival::ReplyFirst => {
                refuse(&mut runtime, &close, "sample refusal");
                stream(&mut runtime, adapter_error("one", 1, 7, "sample refusal"));
            }
            Arrival::ReplyOnly => refuse(&mut runtime, &close, "sample refusal"),
        }
        let shown = screen(&runtime.state);
        assert_eq!(
            shown.matches("sample refusal").count(),
            1,
            "{arrival:?}\n{shown}"
        );
        assert!(
            shown.contains("Failed to close the session: Failed to close ACP session: sample refusal"),
            "{shown}"
        );
    }
}

#[test]
fn a_session_kept_after_a_refused_close_can_be_closed_again() {
    let (mut runtime, mut rx) = runtime_on_session_one();
    let close = sent_for(&mut runtime, &mut rx, "/close");
    refuse(&mut runtime, &close, "sample refusal");
    stream(&mut runtime, adapter_error("one", 1, 7, "sample refusal"));
    assert_eq!(runtime.state.selected.as_deref(), Some("one"));
    assert!(
        !screen(&runtime.state).contains("Session ended"),
        "{}",
        screen(&runtime.state)
    );

    let retry = sent_for(&mut runtime, &mut rx, "/close");
    assert_eq!(retry["method"], "session/close");
    assert_eq!(retry["params"], json!({"sessionId": "one"}));
    refuse(&mut runtime, &retry, "sample refusal");
    stream(&mut runtime, adapter_error("one", 2, 8, "sample refusal"));
    assert_eq!(screen(&runtime.state).matches("sample refusal").count(), 2);
}

#[derive(Debug, Clone, Copy)]
enum Unrelated {
    BeforeRefusal,
    BeforeRelease,
    AfterShownRefusal,
}

#[test]
fn other_adapter_errors_around_a_close_still_appear() {
    for case in [
        Unrelated::BeforeRefusal,
        Unrelated::BeforeRelease,
        Unrelated::AfterShownRefusal,
    ] {
        let (mut runtime, mut rx) = runtime_on_session_one();
        let close = sent_for(&mut runtime, &mut rx, "/close");
        match case {
            Unrelated::BeforeRefusal => {
                stream(&mut runtime, adapter_error("one", 1, 8, "sample unrelated failure"));
                refuse(&mut runtime, &close, "sample refusal");
            }
            Unrelated::BeforeRelease => {
                stream(&mut runtime, adapter_error("one", 1, 8, "sample unrelated failure"));
                runtime
                    .connection_event(crate::host_client::ConnectionEvent::Message(json!({
                        "jsonrpc": "2.0", "id": close["id"], "result": {}
                    })))
                    .unwrap();
            }
            Unrelated::AfterShownRefusal => {
                refuse(&mut runtime, &close, "sample refusal");
                stream(&mut runtime, adapter_error("one", 1, 8, "sample unrelated failure"));
            }
        }
        let shown = screen(&runtime.state);
        assert_eq!(
            shown.matches("sample unrelated failure").count(),
            1,
            "{case:?}\n{shown}"
        );
    }
}

#[test]
fn a_close_refused_after_switching_sessions_is_shown_once_with_its_session() {
    let (mut runtime, mut rx) = runtime_on_session_one();
    runtime
        .state
        .replace_sessions(json!({"sessions":[
            {"session_id":"one","agent":"mock-model","status":"ready","title":"One","cwd":"/work/one"},
            {"session_id":"two","agent":"mock-model","status":"ready","title":"Two","cwd":"/work/two"}
        ]}))
        .unwrap();
    let close = sent_for(&mut runtime, &mut rx, "/close");
    runtime.state.selected = Some("two".into());
    refuse(&mut runtime, &close, "sample refusal");
    stream(&mut runtime, adapter_error("one", 1, 7, "sample refusal"));
    let on_two = screen(&runtime.state);
    assert_eq!(
        on_two.matches("Failed to close session one: Failed to close ACP session: sample refusal").count(),
        1,
        "{on_two}"
    );

    runtime.open_session("one".into());
    let on_one = screen(&runtime.state);
    assert_eq!(on_one.matches("sample refusal").count(), 1, "{on_one}");
}

#[test]
fn held_errors_come_before_the_refusal() {
    let (mut runtime, mut rx) = runtime_on_session_one();
    let close = sent_for(&mut runtime, &mut rx, "/close");
    stream(&mut runtime, adapter_error("one", 1, 8, "sample unrelated failure"));
    refuse(&mut runtime, &close, "sample refusal");
    let shown = screen(&runtime.state);
    let unrelated = shown.find("sample unrelated failure").expect("held error shown");
    let refusal = shown.find("Failed to close the session").expect("refusal shown");
    assert!(unrelated < refusal, "{shown}");
}

#[test]
fn a_held_error_appears_once_when_the_connection_drops_before_the_reply() {
    let (mut runtime, mut rx) = runtime_on_session_one();
    sent_for(&mut runtime, &mut rx, "/close");
    stream(&mut runtime, adapter_error("one", 1, 8, "sample unrelated failure"));
    runtime
        .connection_event(crate::host_client::ConnectionEvent::Disconnected("offline".into()))
        .unwrap();
    assert_eq!(
        screen(&runtime.state).matches("sample unrelated failure").count(),
        1,
        "{}",
        screen(&runtime.state)
    );
}

#[test]
fn a_second_close_waits_for_the_first() {
    let (mut runtime, mut rx) = runtime_on_session_one();
    let close = sent_for(&mut runtime, &mut rx, "/close");
    stream(&mut runtime, adapter_error("one", 1, 8, "sample unrelated failure"));
    runtime.composer.handle_paste("/close".into());
    runtime.key(crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok()).all(|sent| sent["method"] != "session/close"),
        "a second close was sent while the first was in flight"
    );
    assert!(screen(&runtime.state).contains("already closing"), "{}", screen(&runtime.state));

    refuse(&mut runtime, &close, "sample refusal");
    stream(&mut runtime, adapter_error("one", 2, 7, "sample refusal"));
    let shown = screen(&runtime.state);
    assert_eq!(shown.matches("sample unrelated failure").count(), 1, "{shown}");
    assert_eq!(shown.matches("sample refusal").count(), 1, "{shown}");
}

#[test]
fn a_refusal_without_its_stream_copy_does_not_hide_later_errors() {
    let (mut runtime, mut rx) = runtime_on_session_one();
    let close = sent_for(&mut runtime, &mut rx, "/close");
    refuse(&mut runtime, &close, "sample refusal");
    stream(&mut runtime, message("one", 1, "Working on the sample."));
    stream(&mut runtime, adapter_error("one", 2, 9, "sample refusal"));
    assert_eq!(
        screen(&runtime.state).matches("sample refusal").count(),
        2,
        "a later error is not the refusal's copy:\n{}",
        screen(&runtime.state)
    );
}

#[test]
fn a_close_that_cannot_be_sent_holds_nothing() {
    let (mut runtime, mut rx) = runtime_on_session_one();
    runtime.client.stop();
    runtime.composer.handle_paste("/close".into());
    runtime.key(crossterm::event::KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    while rx.try_recv().is_ok() {}
    runtime
        .state
        .apply(adapter_error("one", 1, 8, "sample unrelated failure"))
        .unwrap();
    assert!(screen(&runtime.state).contains("sample unrelated failure"));
}
