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
        assert!(shown.contains("Failed to close the session"), "{shown}");
    }
}

#[test]
fn a_session_kept_after_a_refused_close_can_be_closed_again() {
    let (mut runtime, mut rx) = runtime_on_session_one();
    let close = sent_for(&mut runtime, &mut rx, "/close");
    refuse(&mut runtime, &close, "sample refusal");
    stream(&mut runtime, adapter_error("one", 1, 7, "sample refusal"));
    assert!(screen(&runtime.state).contains("One"), "{}", screen(&runtime.state));

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
    assert!(
        !on_two.contains("sample refusal") || on_two.contains("one"),
        "a refusal seen on another session names its own:\n{on_two}"
    );

    runtime.open_session("one".into());
    let on_one = screen(&runtime.state);
    assert_eq!(on_one.matches("sample refusal").count(), 1, "{on_one}");
}
