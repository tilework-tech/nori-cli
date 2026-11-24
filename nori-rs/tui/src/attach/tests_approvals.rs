// `/approvals` in an attached terminal: the host modes for the session on
// screen, set by the controller only, with the header following each
// session's announced mode.

fn announced(session: &str, seq: i64, mode: nori_protocol::ApprovalMode) -> SessionEnvelope {
    SessionEnvelope {
        conversation_id: session.into(),
        agent: "test-agent".into(),
        seq,
        event: nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ApprovalModeChanged(
            nori_protocol::SessionApprovalModes {
                current: mode,
                available: vec![
                    nori_protocol::ApprovalMode::Ask,
                    nori_protocol::ApprovalMode::Agent,
                    nori_protocol::ApprovalMode::ApproveAll,
                ],
            },
        )),
    }
}

fn mode_requests(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) -> Vec<serde_json::Value> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|message| message["method"] == "session/set_mode")
        .map(|message| message["params"].clone())
        .collect()
}

fn press(runtime: &mut super::runtime::Runtime, code: KeyCode) {
    runtime.key(crossterm::event::KeyEvent::new(code, KeyModifiers::NONE));
}

fn on_session(runtime: &mut super::runtime::Runtime, session: &str) {
    runtime
        .state
        .apply(announced(session, 1, nori_protocol::ApprovalMode::Agent))
        .unwrap();
    runtime.state.selected = Some(session.into());
}

#[test]
fn the_approvals_menu_changes_the_mode_of_the_session_on_screen() {
    let (mut runtime, mut rx) = attached("controller");
    on_session(&mut runtime, "session-alpha");

    submit(&mut runtime, "/approvals");
    let menu = runtime_screen(&runtime, 20);
    press(&mut runtime, KeyCode::Down);
    press(&mut runtime, KeyCode::Enter);
    let before_confirming = mode_requests(&mut rx);
    let confirmation = runtime_screen(&runtime, 20);
    press(&mut runtime, KeyCode::Enter);

    insta::assert_snapshot!("attached_approvals_menu", menu);
    insta::assert_snapshot!("attached_approve_all_confirmation", confirmation);
    assert_eq!(before_confirming, Vec::<serde_json::Value>::new());
    assert_eq!(
        mode_requests(&mut rx),
        vec![json!({"sessionId": "session-alpha", "modeId": "approve-all"})]
    );
    assert!(
        !runtime_screen(&runtime, 20).contains("Approvals for this session"),
        "the menu closes after a choice"
    );
}

#[test]
fn escape_closes_the_approvals_menu_without_a_change() {
    let (mut runtime, mut rx) = attached("controller");
    on_session(&mut runtime, "session-alpha");

    submit(&mut runtime, "/approvals");
    press(&mut runtime, KeyCode::Esc);

    assert_eq!(mode_requests(&mut rx), Vec::<serde_json::Value>::new());
    assert!(!runtime_screen(&runtime, 20).contains("Approvals for this session"));
}

#[test]
fn a_mode_named_after_approvals_is_set_directly() {
    let (mut runtime, mut rx) = attached("controller");
    on_session(&mut runtime, "session-alpha");

    submit(&mut runtime, "/approvals ask");
    submit(&mut runtime, "/approvals approve-all");
    let asked_first = mode_requests(&mut rx);
    press(&mut runtime, KeyCode::Enter);
    runtime.composer.clear();
    submit(&mut runtime, "/approvals sometimes");

    assert_eq!(
        asked_first,
        vec![json!({"sessionId": "session-alpha", "modeId": "ask"})]
    );
    assert_eq!(
        mode_requests(&mut rx),
        vec![json!({"sessionId": "session-alpha", "modeId": "approve-all"})]
    );
    let notice = runtime.state.notice.clone().unwrap_or_default();
    assert!(notice.contains("ask, agent or approve-all"), "{notice}");
}

#[test]
fn the_header_shows_the_mode_of_the_session_on_screen() {
    let (mut runtime, _rx) = attached("controller");
    runtime
        .state
        .apply(announced("one", 1, nori_protocol::ApprovalMode::Ask))
        .unwrap();
    runtime
        .state
        .apply(announced("two", 1, nori_protocol::ApprovalMode::ApproveAll))
        .unwrap();

    runtime.state.selected = Some("one".into());
    let first = runtime_screen(&runtime, 12);
    runtime.state.selected = Some("two".into());
    let second = runtime_screen(&runtime, 12);
    runtime
        .state
        .apply(announced("two", 2, nori_protocol::ApprovalMode::Agent))
        .unwrap();
    let changed = runtime_screen(&runtime, 12);

    assert!(first.contains("Approvals: Ask"), "{first}");
    assert!(second.contains("Approvals: Approve all"), "{second}");
    assert!(changed.contains("Approvals: Agent"), "{changed}");
}

#[test]
fn only_the_controller_can_change_a_mode() {
    let (mut runtime, mut rx) = attached("watcher");
    on_session(&mut runtime, "session-alpha");

    submit(&mut runtime, "/approvals ask");
    let refused = runtime.state.notice.clone().unwrap_or_default();
    change_role(&mut runtime, "controller");
    runtime.composer.clear();
    submit(&mut runtime, "/approvals ask");

    assert!(
        refused.contains("watching · Alt+T to take control"),
        "{refused}"
    );
    assert_eq!(
        mode_requests(&mut rx),
        vec![json!({"sessionId": "session-alpha", "modeId": "ask"})]
    );
}

#[test]
fn the_menu_closes_when_control_moves_away_or_another_session_opens() {
    let (mut runtime, mut rx) = attached("controller");
    on_session(&mut runtime, "session-alpha");
    runtime
        .state
        .apply(announced("session-beta", 1, nori_protocol::ApprovalMode::Agent))
        .unwrap();

    submit(&mut runtime, "/approvals");
    change_role(&mut runtime, "watcher");
    let after_losing_control = runtime_screen(&runtime, 20);
    change_role(&mut runtime, "controller");
    submit(&mut runtime, "/approvals");
    runtime.open_session("session-beta".into());
    let after_switching = runtime_screen(&runtime, 20);
    press(&mut runtime, KeyCode::Enter);

    assert!(!after_losing_control.contains(super::approvals::TITLE));
    assert!(!after_switching.contains(super::approvals::TITLE));
    assert_eq!(mode_requests(&mut rx), Vec::<serde_json::Value>::new());
}

#[test]
fn a_session_known_only_from_the_list_shows_its_mode() {
    let (mut runtime, _rx) = attached("controller");
    runtime
        .state
        .replace_sessions(json!({"sessions": [{
            "session_id": "session-gamma",
            "agent": "test-agent",
            "status": "ready",
            "title": null,
            "approval_mode": "approve-all",
        }]}))
        .unwrap();
    runtime.state.selected = Some("session-gamma".into());

    let header = runtime_screen(&runtime, 12);

    assert!(header.contains("Approvals: Approve all"), "{header}");
}

#[test]
fn ask_is_unavailable_when_the_host_does_not_offer_it() {
    let (mut runtime, mut rx) = attached("controller");
    runtime
        .state
        .replace_sessions(json!({"sessions": [{
            "session_id": "session-delta",
            "agent": "test-agent",
            "status": "ready",
            "title": null,
            "approval_mode": "agent",
            "approval_modes": ["agent", "approve-all"],
        }]}))
        .unwrap();
    runtime.state.selected = Some("session-delta".into());

    submit(&mut runtime, "/approvals ask");
    let typed = runtime.state.notice.clone().unwrap_or_default();
    runtime.composer.clear();
    submit(&mut runtime, "/approvals");
    let menu = runtime_screen(&runtime, 20);
    press(&mut runtime, KeyCode::Up);
    press(&mut runtime, KeyCode::Enter);

    assert!(typed.contains("no mode that asks"), "{typed}");
    assert!(menu.contains("Not available"), "{menu}");
    assert_eq!(mode_requests(&mut rx), Vec::<serde_json::Value>::new());
}
