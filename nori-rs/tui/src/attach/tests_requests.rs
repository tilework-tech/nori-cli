fn permission_event(seq: i64) -> SessionEnvelope {
    use nori_protocol::acp::v1 as acp;
    let request: acp::RequestPermissionRequest = serde_json::from_value(json!({
        "sessionId":"agent-session", "toolCall":{"toolCallId":"tool", "title":"Write a file"},
        "options":[
            {"optionId":"allow", "name":"Allow once", "kind":"allow_once"},
            {"optionId":"reject", "name":"Reject", "kind":"reject_once"}
        ]
    }))
    .unwrap();
    SessionEnvelope {
        conversation_id: "parent-session".into(),
        agent: "mock-model".into(),
        seq,
        event: nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Request {
            request_id: acp::RequestId::Str("permission".into()),
            request: acp::AgentRequest::RequestPermissionRequest(request),
        }),
    }
}

/// A controller with `my draft` typed, after `event` arrived from the host
/// and its request has been on screen for a while.
fn runtime_with_draft(
    event: SessionEnvelope,
) -> (
    super::runtime::Runtime,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    let (runtime, rx, clock) = runtime_with_fresh_request(event);
    clock.pass_grace();
    (runtime, rx)
}

/// A controller with `my draft` typed, at the moment `event` arrives from
/// the host. Time moves only through the returned clock.
fn runtime_with_fresh_request(
    event: SessionEnvelope,
) -> (
    super::runtime::Runtime,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
    crate::answer_guard::ManualClock,
) {
    let clock = crate::answer_guard::ManualClock::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime =
        super::runtime::Runtime::with_clock(Some("parent-session".into()), tx, clock.clock());
    runtime.state.connected = true;
    runtime.state.role_changed("controller");
    runtime.state.list_open = false;
    runtime.composer.handle_paste("my draft".into());
    host_event(&mut runtime, event);
    while rx.try_recv().is_ok() {}
    (runtime, rx, clock)
}

fn host_event(runtime: &mut super::runtime::Runtime, event: SessionEnvelope) {
    runtime
        .connection_event(crate::host_client::ConnectionEvent::Message(json!({
            "jsonrpc":"2.0", "method":"_nori/session/event",
            "params":serde_json::to_value(event).unwrap()
        })))
        .unwrap();
}

#[test]
fn option_keys_typed_as_a_permission_appears_do_not_answer_it() {
    for (code, option) in [
        (KeyCode::Char('1'), "allow"),
        (KeyCode::Char('2'), "reject"),
        (KeyCode::Enter, "allow"),
    ] {
        let (mut runtime, mut rx, clock) = runtime_with_fresh_request(permission_event(1));
        clock.advance(
            crate::answer_guard::REQUEST_ANSWER_GRACE - std::time::Duration::from_millis(1),
        );
        key(&mut runtime, code, KeyModifiers::NONE);
        assert!(rx.try_recv().is_err(), "{code:?} answered too early");
        resolve_permission_view_check(&runtime, code);

        clock.pass_grace();
        key(&mut runtime, code, KeyModifiers::NONE);
        let sent = rx
            .try_recv()
            .expect("an answer is sent once the grace ends");
        assert_eq!(sent["method"], "_nori/session/respond", "{code:?}: {sent}");
        assert_eq!(
            sent["params"]["response"]["outcome"]["optionId"], option,
            "{code:?}: {sent}"
        );
    }
}

/// The request is still on screen and the draft is untouched.
fn resolve_permission_view_check(
    runtime: &super::runtime::Runtime,
    code: crossterm::event::KeyCode,
) {
    assert!(
        runtime.state.request_on_screen(),
        "{code:?} closed the request"
    );
    resolve_draft_check(runtime, code);
}

fn resolve_draft_check(runtime: &super::runtime::Runtime, code: crossterm::event::KeyCode) {
    let area = ratatui::layout::Rect::new(0, 0, 60, runtime.composer.desired_height(60));
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    runtime.composer.render_ref(area, &mut buffer);
    let text: String = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        text.contains("my draft"),
        "{code:?} changed the draft:\n{text}"
    );
    assert!(
        !text.contains("my draft1") && !text.contains("my draft2"),
        "{code:?} typed into the draft:\n{text}"
    );
}

#[test]
fn option_keys_typed_as_a_delegation_appears_do_not_answer_it() {
    let (mut runtime, mut rx, clock) =
        runtime_with_fresh_request(delegation_event(1, "awaiting_approval"));
    key(&mut runtime, KeyCode::Char('1'), KeyModifiers::NONE);
    assert!(rx.try_recv().is_err(), "1 answered too early");

    clock.pass_grace();
    key(&mut runtime, KeyCode::Char('1'), KeyModifiers::NONE);
    let sent = rx
        .try_recv()
        .expect("an answer is sent once the grace ends");
    assert_eq!(sent["method"], "_nori/session/delegation/resolve", "{sent}");
    assert_eq!(sent["params"]["approved"], true, "{sent}");
}

#[test]
fn opening_a_session_that_already_waits_restarts_the_grace_period() {
    let (mut runtime, mut rx, clock) = runtime_with_fresh_request(permission_event(1));
    runtime
        .state
        .replace_sessions(json!({"sessions":[
            {"session_id":"parent-session","agent":"mock-model","status":"waiting_on_you","title":"Parent task"},
            {"session_id":"other-session","agent":"mock-model","status":"ready","title":"Other task"}
        ]}))
        .unwrap();
    runtime.state.selected = Some("other-session".into());
    clock.pass_grace();
    // The jump to the waiting session shows its request for the first time.
    key(&mut runtime, KeyCode::Down, KeyModifiers::ALT);
    while rx.try_recv().is_ok() {}
    assert_eq!(runtime.state.selected.as_deref(), Some("parent-session"));

    key(&mut runtime, KeyCode::Char('1'), KeyModifiers::NONE);
    assert!(
        rx.try_recv().is_err(),
        "1 answered a request just brought on screen"
    );

    clock.pass_grace();
    key(&mut runtime, KeyCode::Char('1'), KeyModifiers::NONE);
    let sent = rx
        .try_recv()
        .expect("an answer is sent once the grace ends");
    assert_eq!(sent["method"], "_nori/session/respond", "{sent}");
}

#[test]
fn a_delegation_arriving_over_a_permission_gets_its_own_grace_period() {
    let (mut runtime, mut rx, clock) = runtime_with_fresh_request(permission_event(1));
    clock.pass_grace();
    host_event(&mut runtime, delegation_event(2, "awaiting_approval"));
    while rx.try_recv().is_ok() {}

    key(&mut runtime, KeyCode::Char('1'), KeyModifiers::NONE);
    assert!(
        rx.try_recv().is_err(),
        "1 answered a delegation that just appeared"
    );

    clock.pass_grace();
    key(&mut runtime, KeyCode::Char('1'), KeyModifiers::NONE);
    let sent = rx
        .try_recv()
        .expect("an answer is sent once the grace ends");
    assert_eq!(sent["method"], "_nori/session/delegation/resolve", "{sent}");
}

#[test]
fn esc_and_ctrl_c_act_on_a_request_during_the_grace_period() {
    let (mut runtime, mut rx, _clock) = runtime_with_fresh_request(permission_event(1));
    key(&mut runtime, KeyCode::Esc, KeyModifiers::NONE);
    let sent = rx.try_recv().expect("esc answers at once");
    assert_eq!(
        sent["params"]["response"]["outcome"]["optionId"], "reject",
        "{sent}"
    );

    let (mut runtime, mut rx, _clock) = runtime_with_fresh_request(permission_event(1));
    key(&mut runtime, KeyCode::Char('c'), KeyModifiers::CONTROL);
    let sent = rx.try_recv().expect("ctrl+c answers at once");
    assert_eq!(
        sent["params"]["response"]["outcome"]["outcome"], "cancelled",
        "{sent}"
    );
}

fn assert_permission_answer_keeps_draft(code: KeyCode, option: &str) {
    let (mut runtime, mut rx) = runtime_with_draft(permission_event(1));
    key(&mut runtime, code, KeyModifiers::NONE);
    let sent = rx.try_recv().expect("an answer is sent");
    assert_eq!(sent["method"], "_nori/session/respond", "{sent}");
    assert_eq!(
        sent["params"]["response"]["outcome"]["optionId"], option,
        "{sent}"
    );
    assert!(rx.try_recv().is_err(), "the draft is not sent as a prompt");
    resolve_permission(&mut runtime);
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    assert!(
        rendered.contains("my draft"),
        "the draft stays:\n{rendered}"
    );
}

#[test]
fn enter_answers_a_pending_permission_and_keeps_the_draft() {
    assert_permission_answer_keeps_draft(KeyCode::Enter, "allow");
}

#[test]
fn a_number_answers_a_pending_permission_and_keeps_the_draft() {
    assert_permission_answer_keeps_draft(KeyCode::Char('2'), "reject");
}

fn assert_delegation_approval_keeps_draft(code: KeyCode) {
    let (mut runtime, mut rx) = runtime_with_draft(delegation_event(1, "awaiting_approval"));
    key(&mut runtime, code, KeyModifiers::NONE);
    let sent = rx.try_recv().expect("an answer is sent");
    assert_eq!(sent["method"], "_nori/session/delegation/resolve", "{sent}");
    assert_eq!(sent["params"]["approved"], true, "{sent}");
    assert!(rx.try_recv().is_err(), "the draft is not sent as a prompt");
    settle_delegation(&mut runtime, "running");
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    assert!(
        rendered.contains("my draft"),
        "the draft stays:\n{rendered}"
    );
}

#[test]
fn a_number_approves_a_pending_delegation_and_keeps_the_draft() {
    assert_delegation_approval_keeps_draft(KeyCode::Char('1'));
}

#[test]
fn with_no_request_shown_enter_still_sends_the_draft() {
    let (mut runtime, mut rx) = runtime_with_draft(delegation_event(1, "running"));
    key(&mut runtime, KeyCode::Enter, KeyModifiers::NONE);
    let sent = rx.try_recv().expect("the draft is sent");
    assert_eq!(sent["method"], "_nori/session/prompt", "{sent}");
    assert_eq!(sent["params"]["prompt"][0]["text"], "my draft");
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    assert!(
        !rendered.contains("my draft"),
        "the sent draft leaves the composer"
    );
}

#[test]
fn enter_approves_a_pending_delegation_and_keeps_the_draft() {
    assert_delegation_approval_keeps_draft(KeyCode::Enter);
}

#[test]
fn a_request_in_another_session_leaves_enter_to_the_draft() {
    let (mut runtime, mut rx) = runtime_with_draft(permission_event(1));
    runtime
        .state
        .replace_sessions(json!({"sessions":[
            {"session_id":"parent-session","agent":"mock-model","status":"waiting_on_you","title":"Parent task"},
            {"session_id":"other-session","agent":"mock-model","status":"ready","title":"Other task"}
        ]}))
        .unwrap();
    runtime.state.selected = Some("other-session".into());
    key(&mut runtime, KeyCode::Enter, KeyModifiers::NONE);
    let sent = rx.try_recv().expect("the draft is sent");
    assert_eq!(sent["method"], "_nori/session/prompt", "{sent}");
    assert_eq!(sent["params"]["session_id"], "other-session", "{sent}");
    assert_eq!(sent["params"]["prompt"][0]["text"], "my draft");
    assert!(
        rx.try_recv().is_err(),
        "the other session's request is not answered"
    );
}

#[test]
fn while_a_request_shows_typing_leaves_both_the_request_and_the_draft_alone() {
    let (mut runtime, mut rx) = runtime_with_draft(permission_event(1));
    for character in " fix bugs".chars() {
        key(&mut runtime, KeyCode::Char(character), KeyModifiers::NONE);
    }
    key(&mut runtime, KeyCode::Enter, KeyModifiers::SHIFT);
    assert!(rx.try_recv().is_err(), "nothing is answered or sent");
    key(&mut runtime, KeyCode::Enter, KeyModifiers::NONE);
    let sent = rx.try_recv().expect("plain enter still answers");
    assert_eq!(
        sent["params"]["response"]["outcome"]["optionId"], "allow",
        "{sent}"
    );
    resolve_permission(&mut runtime);
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    assert!(rendered.contains("my draft"), "{rendered}");
    assert!(
        !rendered.contains("fix"),
        "the request kept the keys:\n{rendered}"
    );
}

/// The host's answer to the permission request in `parent-session`.
fn resolve_permission(runtime: &mut super::runtime::Runtime) {
    runtime
        .state
        .apply(SessionEnvelope {
            conversation_id: "parent-session".into(),
            agent: "mock-model".into(),
            seq: runtime
                .state
                .cursors
                .get("parent-session")
                .copied()
                .unwrap_or(0)
                + 1,
            event: nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::RequestResolved(
                nori_protocol::RequestResolved {
                    request_id: nori_protocol::acp::v1::RequestId::Str("permission".into()),
                    answered_by: "client".into(),
                },
            )),
        })
        .unwrap();
}

/// The host's answer to an approved or declined delegation.
fn settle_delegation(runtime: &mut super::runtime::Runtime, status: &str) {
    let seq = runtime
        .state
        .cursors
        .get("parent-session")
        .copied()
        .unwrap_or(0)
        + 1;
    runtime.state.apply(delegation_event(seq, status)).unwrap();
}

#[test]
fn a_request_on_screen_hides_the_composer_and_its_cursor() {
    let (mut runtime, _rx) = runtime_with_draft(permission_event(1));
    let area = ratatui::layout::Rect::new(0, 0, 90, 18);
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    let cursor = super::render::cursor_position(&runtime.state, &runtime.composer, area);
    assert!(!rendered.contains("my draft"), "{rendered}");
    assert_eq!(cursor, None);
    resolve_permission(&mut runtime);
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    let cursor = super::render::cursor_position(&runtime.state, &runtime.composer, area);
    assert!(rendered.contains("my draft"), "{rendered}");
    assert!(cursor.is_some());
}

#[test]
fn a_delegation_approval_on_screen_hides_the_composer() {
    let (runtime, _rx) = runtime_with_draft(delegation_event(1, "awaiting_approval"));
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    assert!(!rendered.contains("my draft"), "{rendered}");
}

#[test]
fn the_request_hint_names_every_key_that_answers_it() {
    let (runtime, _rx) = runtime_with_draft(permission_event(1));
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    assert!(rendered.contains("esc reject"), "{rendered}");
    assert!(rendered.contains("ctrl+c cancel"), "{rendered}");
}

#[test]
fn the_delegation_approval_hint_names_esc_and_ctrl_c() {
    let (runtime, _rx) = runtime_with_draft(delegation_event(1, "awaiting_approval"));
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    assert!(rendered.contains("esc or ctrl+c declines"), "{rendered}");
}

#[test]
fn one_ctrl_c_cancels_a_pending_permission_and_keeps_the_draft() {
    let (mut runtime, mut rx) = runtime_with_draft(permission_event(1));
    runtime
        .state
        .apply(external_change(
            "parent-session",
            2,
            "example",
            nori_protocol::ExternalTurnState::Working,
        ))
        .unwrap();
    let stays_open = runtime.key(crossterm::event::KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ));
    assert!(stays_open, "ctrl+c on a request does not leave attach");
    let sent = rx.try_recv().expect("the request is cancelled");
    assert_eq!(sent["method"], "_nori/session/respond", "{sent}");
    assert_eq!(sent["params"]["request_id"], "permission", "{sent}");
    assert_eq!(
        sent["params"]["response"]["outcome"]["outcome"], "cancelled",
        "{sent}"
    );
    assert!(rx.try_recv().is_err(), "nothing else is sent");
    resolve_permission(&mut runtime);
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    assert!(rendered.contains("my draft"), "{rendered}");
}

#[test]
fn one_ctrl_c_declines_a_pending_delegation_and_keeps_the_draft() {
    let (mut runtime, mut rx) = runtime_with_draft(delegation_event(1, "awaiting_approval"));
    runtime
        .state
        .apply(external_change(
            "parent-session",
            2,
            "example",
            nori_protocol::ExternalTurnState::Working,
        ))
        .unwrap();
    key(&mut runtime, KeyCode::Char('c'), KeyModifiers::CONTROL);
    let sent = rx.try_recv().expect("the delegation is declined");
    assert_eq!(sent["method"], "_nori/session/delegation/resolve", "{sent}");
    assert_eq!(sent["params"]["approved"], false, "{sent}");
    assert!(rx.try_recv().is_err(), "nothing else is sent");
    settle_delegation(&mut runtime, "declined");
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    assert!(rendered.contains("my draft"), "{rendered}");
}

#[test]
fn esc_rejects_a_pending_permission() {
    let (mut runtime, mut rx) = runtime_with_draft(permission_event(1));
    let stays_open = runtime.key(crossterm::event::KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE,
    ));
    let sent = rx.try_recv().expect("the request is rejected");
    assert!(stays_open);
    assert_eq!(sent["method"], "_nori/session/respond", "{sent}");
    assert_eq!(
        sent["params"]["response"]["outcome"]["optionId"], "reject",
        "{sent}"
    );
    assert!(rx.try_recv().is_err(), "nothing else is sent");
    resolve_permission(&mut runtime);
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    assert!(rendered.contains("my draft"), "{rendered}");
}

#[test]
fn esc_declines_a_pending_delegation() {
    let (mut runtime, mut rx) = runtime_with_draft(delegation_event(1, "awaiting_approval"));
    let stays_open = runtime.key(crossterm::event::KeyEvent::new(
        KeyCode::Esc,
        KeyModifiers::NONE,
    ));
    let sent = rx.try_recv().expect("the delegation is declined");
    assert!(stays_open);
    assert_eq!(sent["method"], "_nori/session/delegation/resolve", "{sent}");
    assert_eq!(sent["params"]["approved"], false, "{sent}");
    assert!(rx.try_recv().is_err(), "nothing else is sent");
    settle_delegation(&mut runtime, "declined");
    let rendered = screen_with_composer(&runtime.state, &runtime.composer);
    assert!(rendered.contains("my draft"), "{rendered}");
}

#[test]
fn a_request_first_painted_after_a_slow_replay_batch_gets_its_full_grace() {
    let (mut runtime, mut rx, clock) = runtime_with_fresh_request(permission_event(1));
    let mut painted_request = None;
    clock.pass_grace();
    runtime.request_painted(&mut painted_request);
    key(&mut runtime, KeyCode::Enter, KeyModifiers::NONE);
    assert!(rx.try_recv().is_err());

    clock.pass_grace();
    runtime.request_painted(&mut painted_request);
    key(&mut runtime, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(rx.try_recv().unwrap()["method"], "_nori/session/respond");
}
