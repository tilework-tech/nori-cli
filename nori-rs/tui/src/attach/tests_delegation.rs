fn delegation_event(seq: i64, status: &str) -> SessionEnvelope {
    serde_json::from_value(json!({
        "conversation_id":"parent-session", "agent":"mock-model", "seq":seq,
        "event":{"source":"nori", "event":{"event_type":"delegation_changed", "event":{
            "request_id":"delegation-one", "parent_id":"parent-session", "child_id":"child-session",
            "parent_agent":"mock-model", "agent":"mock-model-alt", "task":"Review the parser",
            "status":status, "depth":1, "result":if status == "completed" { Some("Parser reviewed") } else { None }
        }}}
    })).unwrap()
}

#[test]
fn delegation_command_targets_selected_parent_and_preserves_task() {
    use crossterm::event::KeyEvent;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("parent-session".into()), tx);
    runtime.state.connected = true;
    runtime.state.role_changed("controller");
    runtime
        .composer
        .handle_paste("/delegate mock-model-alt Review the parser carefully".into());
    runtime.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let request = rx.try_recv().unwrap();
    assert_eq!(request["method"], "_nori/session/delegate");
    assert_eq!(
        request["params"],
        json!({"session_id":"parent-session","agent":"mock-model-alt","task":"Review the parser carefully"})
    );
}

#[test]
fn delegation_approval_names_both_sessions_and_cannot_be_answered_by_watcher() {
    use crossterm::event::KeyEvent;
    for (key, approved) in [('1', true), ('2', false)] {
        let clock = crate::answer_guard::ManualClock::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut runtime =
            super::runtime::Runtime::with_clock(Some("parent-session".into()), tx, clock.clock());
        runtime.state.connected = true;
        host_event(&mut runtime, delegation_event(1, "awaiting_approval"));
        clock.pass_grace();
        let rendered = screen(&runtime.state);
        assert!(
            rendered.contains("mock-model-alt") && rendered.contains("Review the parser"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("child-session"),
            "the prompt names the child, not its ID: {rendered}"
        );
        assert!(
            rendered.contains("start it") && rendered.contains("decline"),
            "{rendered}"
        );
        if approved {
            insta::assert_snapshot!("delegation_approval_watcher", rendered);
        }
        runtime.key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
        assert!(
            rx.try_recv().is_err(),
            "watcher must not resolve delegation"
        );
        runtime.state.role_changed("controller");
        runtime.key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
        let request = rx.try_recv().unwrap();
        assert_eq!(request["method"], "_nori/session/delegation/resolve");
        assert_eq!(
            request["params"],
            json!({"session_id":"parent-session","request_id":"delegation-one","approved":approved})
        );
    }
}

#[test]
fn delegation_result_replaces_approval_and_replay_is_not_duplicated() {
    let mut state = AttachState {
        connected: true,
        selected: Some("parent-session".into()),
        list_open: false,
        ..Default::default()
    };
    state
        .apply(delegation_event(1, "awaiting_approval"))
        .unwrap();
    state.apply(delegation_event(2, "completed")).unwrap();
    state.apply(delegation_event(2, "completed")).unwrap();
    let rendered = screen(&state);
    assert!(rendered.contains("Parser reviewed"), "{rendered}");
    assert!(
        rendered.contains("result from mock-model-alt") && !rendered.contains("child-session"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("1 Start"),
        "completed delegation must not retain approval"
    );
    assert_eq!(
        state
            .transcript_text("parent-session")
            .matches("Parser reviewed")
            .count(),
        1
    );
    insta::assert_snapshot!("delegation_result", rendered);
}

#[test]
fn next_waiting_opens_actual_child_and_keeps_parent_identity_in_list() {
    use crossterm::event::KeyEvent;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("parent-session".into()), tx);
    runtime.state.connected = true;
    runtime.state.replace_sessions(json!({"sessions":[
        {"session_id":"parent-session","agent":"mock-model","status":"ready","title":"Parent task","parent":null,"depth":0},
        {"session_id":"child-session","agent":"mock-model-alt","status":"waiting_on_you","title":"Child task","parent":"parent-session","depth":1}
    ]})).unwrap();
    runtime.state.list_open = true;
    let rendered = screen(&runtime.state);
    assert!(
        rendered.contains("Parent task") && rendered.contains("Child task"),
        "{rendered}"
    );
    assert!(rendered.contains("└─ "), "{rendered}");
    assert!(!rendered.contains("(parent"), "{rendered}");
    insta::assert_snapshot!("delegation_child_list", rendered);
    runtime.key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert_eq!(runtime.state.selected.as_deref(), Some("child-session"));
    assert!(!runtime.state.list_open);
    let request = rx.try_recv().unwrap();
    assert_eq!(request["method"], "_nori/session/subscribe");
    assert_eq!(request["params"]["session_id"], "child-session");
}

#[test]
fn the_list_header_summarises_statuses_like_the_local_list() {
    let mut state = AttachState {
        connected: true,
        ..Default::default()
    };
    state.replace_sessions(json!({"sessions":[
        {"session_id":"root-one","agent":"mock-model","status":"working","title":"Ship tree picker"},
        {"session_id":"root-two","agent":"mock-model","status":"ready","title":"Spike transport"},
        {"session_id":"child-one","agent":"mock-model-alt","status":"waiting_on_you","title":"Port picker","parent":"root-one","depth":1,"source":"session"}
    ]})).unwrap();
    let rendered = screen_area(&state, &crate::ComposerInput::for_attachment(), 90, 30);
    assert!(
        rendered.contains("Sessions 3 · 1 waiting on you · 1 working · 1 ready"),
        "{rendered}"
    );
}

#[test]
fn session_tree_keeps_children_under_their_parent_with_aligned_columns() {
    let mut state = AttachState {
        connected: true,
        ..Default::default()
    };
    state.replace_sessions(json!({"sessions":[
        {"session_id":"root-one","agent":"mock-model","status":"working","title":"Ship tree picker","age_seconds":300},
        {"session_id":"root-two","agent":"mock-model","status":"ready","title":"Spike transport","age_seconds":200},
        {"session_id":"child-one","agent":"mock-model-alt","status":"waiting_on_you","title":"Port picker","parent":"root-one","depth":1,"source":"session","age_seconds":100},
        {"session_id":"grandchild","agent":"mock-model","status":"failed","title":"Write snapshot tests","parent":"child-one","depth":2,"source":"session","age_seconds":50},
        {"session_id":"child-two","agent":"mock-model","status":"ready","title":"Update docs","parent":"root-one","depth":1,"source":"session","age_seconds":20}
    ]})).unwrap();
    let rendered = screen_area(&state, &crate::ComposerInput::for_attachment(), 90, 30);
    let titles: Vec<_> = rendered
        .lines()
        .skip_while(|line| !line.contains("Sessions 5"))
        .filter_map(|line| {
            ["Ship", "Port", "Write", "Update", "Spike"]
                .iter()
                .find(|word| line.contains(*word) && line.contains("mock-model"))
                .map(|word| {
                    let at = line.find("mock-model").expect("agent column");
                    (*word, unicode_width::UnicodeWidthStr::width(&line[..at]))
                })
        })
        .collect();
    assert_eq!(
        titles.iter().map(|(word, _)| *word).collect::<Vec<_>>(),
        ["Ship", "Port", "Write", "Update", "Spike"],
        "children follow their parent: {rendered}"
    );
    assert!(
        titles.iter().all(|(_, column)| *column == titles[0].1),
        "the AGENT column does not move: {rendered}"
    );
    insta::assert_snapshot!("delegation_session_tree", rendered);
}

#[test]
fn child_view_shows_breadcrumb_and_alt_up_returns_to_parent() {
    use crossterm::event::KeyEvent;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("child-session".into()), tx);
    runtime.state.connected = true;
    runtime.state.replace_sessions(json!({"sessions":[
        {"session_id":"parent-session","agent":"mock-model","status":"working","title":"Parent task","depth":0},
        {"session_id":"child-session","agent":"mock-model-alt","status":"waiting_on_you","title":"Child task","parent":"parent-session","depth":1,"source":"session"}
    ]})).unwrap();
    runtime.state.list_open = false;
    let rendered = screen(&runtime.state);
    let breadcrumb = rendered
        .lines()
        .find(|line| line.contains("›"))
        .unwrap_or_else(|| panic!("breadcrumb missing:\n{rendered}"));
    assert!(
        breadcrumb.contains("Parent task") && breadcrumb.contains("Child task"),
        "{rendered}"
    );
    assert!(breadcrumb.contains("alt+up"), "{rendered}");
    insta::assert_snapshot!("delegation_child_breadcrumb", rendered);
    runtime.key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(runtime.state.selected.as_deref(), Some("parent-session"));
    let request = rx.try_recv().unwrap();
    assert_eq!(request["method"], "_nori/session/subscribe");
    assert_eq!(request["params"]["session_id"], "parent-session");
}

#[test]
fn long_names_keep_the_current_name_whole_before_the_parent_key() {
    let state = {
        let mut state = AttachState {
            connected: true,
            selected: Some("child-session".into()),
            list_open: false,
            ..Default::default()
        };
        state.replace_sessions(json!({"sessions":[
            {"session_id":"parent-session","agent":"mock-model","status":"working","title":"Coordinate the quarterly release notes for every product"},
            {"session_id":"child-session","agent":"mock-model-alt","status":"waiting_on_you","title":"Verify every link in the generated changelog","parent":"parent-session","source":"session"}
        ]})).unwrap();
        state.list_open = false;
        state
    };
    let rendered = screen(&state);
    let breadcrumb = rendered
        .lines()
        .find(|line| line.contains("›"))
        .unwrap_or_else(|| panic!("breadcrumb missing:\n{rendered}"));
    assert!(
        breadcrumb.contains("Verify every link in the generated changelog · waiting on you"),
        "{rendered}"
    );
    assert!(
        !breadcrumb.contains("alt+up"),
        "the key gave way: {rendered}"
    );
}

#[test]
fn alt_up_in_a_top_level_session_stays_put() {
    use crossterm::event::KeyEvent;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("parent-session".into()), tx);
    runtime.state.connected = true;
    runtime.state.replace_sessions(json!({"sessions":[
        {"session_id":"parent-session","agent":"mock-model","status":"ready","title":"Parent task"}
    ]})).unwrap();
    runtime.state.list_open = false;
    runtime.key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(runtime.state.selected.as_deref(), Some("parent-session"));
    assert!(rx.try_recv().is_err(), "nothing to subscribe to");
}

#[test]
fn alt_up_in_a_top_level_session_keeps_the_composer() {
    use crossterm::event::KeyEvent;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("parent-session".into()), tx);
    runtime.state.connected = true;
    runtime.state.replace_sessions(json!({"sessions":[
        {"session_id":"parent-session","agent":"mock-model","status":"ready","title":"Parent task"}
    ]})).unwrap();
    runtime.state.list_open = false;
    runtime.composer.set_text("an earlier prompt".to_string());
    runtime
        .composer
        .input(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    runtime.composer.clear();

    runtime.key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(runtime.composer.text(), "", "no history recall");

    runtime.composer.set_text("half typed".to_string());
    runtime.key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(runtime.composer.text(), "half typed");
}

#[test]
fn delivered_result_prompt_reads_as_one_line_in_attached_parent() {
    use nori_protocol::acp::v1 as acp;
    let mut state = AttachState {
        connected: true,
        selected: Some("parent-session".into()),
        list_open: false,
        ..Default::default()
    };
    state.apply(delegation_event(1, "completed")).unwrap();
    let nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::DelegationChanged(done)) =
        delegation_event(1, "completed").event
    else {
        panic!("delegation fixture");
    };
    let prompt = nori_protocol::delegation_result_prompt(&done);
    let chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(prompt)))
        .message_id("delivery");
    state
        .apply(SessionEnvelope {
            conversation_id: "parent-session".into(),
            agent: "mock-model".into(),
            seq: 2,
            event: nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
                    "agent-session",
                    acp::SessionUpdate::UserMessageChunk(chunk),
                )),
            )),
        })
        .unwrap();
    let rendered = screen(&state);
    assert_eq!(
        rendered.matches("Parser reviewed").count(),
        1,
        "the result shows once, in its block: {rendered}"
    );
    assert!(
        rendered.contains("delegation result from mock-model-alt"),
        "{rendered}"
    );
    assert!(!rendered.contains("child-session"), "{rendered}");
}

#[test]
fn a_fork_gets_no_parent_breadcrumb_or_parent_key() {
    use crossterm::event::KeyEvent;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("fork-session".into()), tx);
    runtime.state.connected = true;
    runtime.state.replace_sessions(json!({"sessions":[
        {"session_id":"parent-session","agent":"mock-model","status":"closed","title":"Parent task"},
        {"session_id":"fork-session","agent":"mock-model","status":"ready","title":"Forked task","parent":"parent-session","source":"fork"}
    ]})).unwrap();
    runtime.state.list_open = false;
    let rendered = screen(&runtime.state);
    assert!(!rendered.contains("alt+up"), "{rendered}");
    assert!(!rendered.contains("Parent task"), "{rendered}");
    runtime.key(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT));
    assert_eq!(runtime.state.selected.as_deref(), Some("fork-session"));
    assert!(rx.try_recv().is_err());
}

#[test]
fn next_waiting_uses_the_configured_binding() {
    use crossterm::event::KeyEvent;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("parent-session".into()), tx);
    runtime.set_next_waiting_binding(nori_config::HotkeyBinding::from_str("alt+w"));
    runtime.state.connected = true;
    runtime.state.replace_sessions(json!({"sessions":[
        {"session_id":"parent-session","agent":"mock-model","status":"ready","title":"Parent task"},
        {"session_id":"child-session","agent":"mock-model-alt","status":"waiting_on_you","title":"Child task","parent":"parent-session"}
    ]})).unwrap();
    runtime.key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    assert_eq!(runtime.state.selected.as_deref(), Some("parent-session"));
    runtime.key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::ALT));
    assert_eq!(runtime.state.selected.as_deref(), Some("child-session"));
}

#[test]
fn refreshed_delegation_completion_removes_an_unseen_approval() {
    use crossterm::event::KeyEvent;
    let clock = crate::answer_guard::ManualClock::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime =
        super::runtime::Runtime::with_clock(Some("parent-session".into()), tx, clock.clock());
    runtime.state.connected = true;
    runtime.state.role_changed("controller");
    host_event(&mut runtime, delegation_event(1, "awaiting_approval"));
    clock.pass_grace();
    let nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::DelegationChanged(completed)) =
        delegation_event(2, "completed").event
    else {
        panic!("delegation fixture");
    };
    runtime.state.replace_sessions(json!({"sessions":[{"session_id":"parent-session","agent":"mock-model","status":"ready","title":"Parent task","delegations":[completed]}]})).unwrap();
    runtime.key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
    assert!(
        rx.try_recv().is_err(),
        "list refresh must remove stale approval actions before parent replay arrives"
    );
    assert!(!screen(&runtime.state).contains("1 Start"));
}

#[test]
fn next_waiting_refreshes_stale_rows_and_opens_new_child_without_second_key() {
    use crossterm::event::KeyEvent;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("parent-session".into()), tx);
    runtime.state.connected = true;
    runtime.state.replace_sessions(json!({"sessions":[
        {"session_id":"parent-session","agent":"mock-model","status":"ready","title":"Parent task"}
    ]})).unwrap();
    runtime.key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    let refresh = rx
        .try_recv()
        .expect("navigation refreshes stale session rows");
    assert_eq!(refresh["method"], "_nori/session/list");
    runtime.connection_event(super::connection::ConnectionEvent::Message(json!({
        "id":refresh["id"],"result":{"sessions":[
            {"session_id":"parent-session","agent":"mock-model","status":"ready"},
            {"session_id":"child-session","agent":"mock-model","status":"waiting_on_you","parent":"parent-session","depth":1}
        ]}
    }))).unwrap();
    assert_eq!(runtime.state.selected.as_deref(), Some("child-session"));
    let subscribe = rx.try_recv().unwrap();
    assert_eq!(subscribe["method"], "_nori/session/subscribe");
    assert_eq!(subscribe["params"]["session_id"], "child-session");
    assert!(
        rx.try_recv().is_err(),
        "one navigation sends one subscription"
    );
}

#[test]
fn deferred_next_waiting_keeps_a_new_draft_in_its_parent_session() {
    use crossterm::event::KeyEvent;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("parent-session".into()), tx);
    runtime.state.connected = true;
    runtime.state.role_changed("controller");
    runtime
        .state
        .replace_sessions(json!({"sessions":[
            {"session_id":"parent-session","agent":"mock-model","status":"ready"}
        ]}))
        .unwrap();
    runtime.key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    let refresh = rx.try_recv().unwrap();
    runtime.composer.handle_paste("Parent-only draft".into());
    runtime.connection_event(super::connection::ConnectionEvent::Message(json!({
        "id":refresh["id"],"result":{"sessions":[
            {"session_id":"parent-session","agent":"mock-model","status":"ready"},
            {"session_id":"child-session","agent":"mock-model","status":"waiting_on_you","parent":"parent-session","depth":1}
        ]}
    }))).unwrap();
    assert_eq!(runtime.state.selected.as_deref(), Some("parent-session"));
    assert!(rx.try_recv().is_err(), "draft prevents deferred navigation");
    runtime.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let prompt = rx.try_recv().unwrap();
    assert_eq!(prompt["method"], "_nori/session/prompt");
    assert_eq!(prompt["params"]["session_id"], "parent-session");
}

#[test]
fn explicit_session_list_navigation_cancels_deferred_next_waiting() {
    use crossterm::event::KeyEvent;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("parent-session".into()), tx);
    runtime.state.connected = true;
    runtime
        .state
        .replace_sessions(json!({"sessions":[
            {"session_id":"parent-session","agent":"mock-model","status":"ready"}
        ]}))
        .unwrap();
    runtime.key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT));
    let refresh = rx.try_recv().unwrap();
    runtime.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT));
    runtime.connection_event(super::connection::ConnectionEvent::Message(json!({
        "id":refresh["id"],"result":{"sessions":[
            {"session_id":"parent-session","agent":"mock-model","status":"ready"},
            {"session_id":"child-session","agent":"mock-model","status":"waiting_on_you","parent":"parent-session","depth":1}
        ]}
    }))).unwrap();
    assert_eq!(runtime.state.selected.as_deref(), Some("parent-session"));
    assert!(
        runtime.state.list_open,
        "explicit list navigation remains open"
    );
    assert!(
        rx.try_recv().is_err(),
        "refresh cannot override explicit navigation"
    );
}

fn family() -> serde_json::Value {
    json!({"sessions":[
        {"session_id":"root-session","agent":"mock-model","status":"ready","family_status":"waiting_on_you","title":"Ship tree-row picker","depth":0},
        {"session_id":"child-session","agent":"mock-model-alt","status":"working","family_status":"waiting_on_you","title":"Port picker","parent":"root-session","depth":1,"source":"session"},
        {"session_id":"grandchild-session","agent":"mock-model","status":"waiting_on_you","family_status":"waiting_on_you","title":"Write snapshot tests","parent":"child-session","depth":2,"source":"session"},
        {"session_id":"other-session","agent":"mock-model","status":"ready","family_status":"ready","title":"Spike transport","depth":0}
    ]})
}

fn viewing(selected: &str) -> AttachState {
    let mut state = AttachState {
        connected: true,
        selected: Some(selected.into()),
        ..Default::default()
    };
    state.replace_sessions(family()).unwrap();
    state.list_open = false;
    state
}

#[test]
fn a_top_level_session_says_when_a_grandchild_waits_on_you() {
    let rendered = screen(&viewing("root-session"));
    let waiting = rendered
        .lines()
        .find(|line| line.contains("is waiting on you"))
        .unwrap_or_else(|| panic!("waiting line missing:\n{rendered}"));
    assert!(
        waiting.contains("● Port picker › Write snapshot tests is waiting on you"),
        "{rendered}"
    );
    assert!(
        waiting.trim_end().ends_with("alt+down to jump"),
        "{rendered}"
    );
    let mut state = viewing("root-session");
    state.list_open = true;
    let list = screen_area(&state, &crate::ComposerInput::for_attachment(), 90, 30);
    let row = |title: &str| {
        list.lines()
            .skip_while(|line| !line.contains("Sessions 4"))
            .find(|line| line.contains(title))
            .unwrap_or_else(|| panic!("{title} row in:\n{list}"))
            .to_string()
    };
    assert!(
        row("Ship tree-row picker").contains("waiting on you"),
        "{list}"
    );
    assert!(row("Port picker").contains("working"), "{list}");
    assert!(row("Spike transport").contains("ready"), "{list}");
}

#[test]
fn the_waiting_line_names_another_waiting_session_never_the_one_on_screen() {
    let mut state = AttachState {
        connected: true,
        selected: Some("grandchild-session".into()),
        ..Default::default()
    };
    let mut sessions = family();
    for row in sessions["sessions"].as_array_mut().unwrap() {
        if row["session_id"] == "other-session" {
            row["status"] = json!("waiting_on_you");
            row["family_status"] = json!("waiting_on_you");
        }
    }
    state.replace_sessions(sessions).unwrap();
    state.list_open = false;
    let rendered = screen(&state);
    let waiting = rendered
        .lines()
        .find(|line| line.contains("is waiting on you"))
        .unwrap_or_else(|| panic!("waiting line missing:\n{rendered}"));
    assert!(
        waiting.contains("● Spike transport is waiting on you"),
        "{rendered}"
    );
    assert!(!waiting.contains("Write snapshot tests"), "{rendered}");
}

#[test]
fn a_grandchild_breadcrumb_shows_the_path_from_its_top_level_session() {
    let rendered = screen_at(&viewing("grandchild-session"), 140);
    let breadcrumb = rendered
        .lines()
        .find(|line| line.contains("alt+up parent"))
        .unwrap_or_else(|| panic!("breadcrumb missing:\n{rendered}"));
    assert!(
        breadcrumb.contains(
            "mock-model · Ship tree-row picker › mock-model-alt · Port picker › mock-model · Write snapshot tests"
        ),
        "{rendered}"
    );
    for width in [100, 80] {
        let rendered = screen_at(&viewing("grandchild-session"), width);
        let breadcrumb = rendered
            .lines()
            .find(|line| line.contains("› mock-model · Write"))
            .unwrap_or_else(|| panic!("breadcrumb missing:\n{rendered}"));
        assert!(
            breadcrumb.starts_with(" mock-model · Ship"),
            "@{width}: {rendered}"
        );
        assert!(
            breadcrumb.contains("› mock-model · Write snapshot tests · waiting on you"),
            "@{width}: {rendered}"
        );
    }
}

#[test]
fn an_untitled_session_is_called_new_session_not_an_id() {
    let mut state = AttachState {
        connected: true,
        selected: Some("24444c0e-1111-2222-3333-444444444444".into()),
        ..Default::default()
    };
    state
        .replace_sessions(json!({"sessions":[
            {"session_id":"24444c0e-1111-2222-3333-444444444444","agent":"mock-model","status":"ready","title":null}
        ]}))
        .unwrap();
    state.list_open = false;
    let rendered = screen(&state);
    assert!(
        rendered.contains("mock-model · new session · ready"),
        "{rendered}"
    );
    assert!(!rendered.contains("24444c0e"), "{rendered}");
}

fn key(
    runtime: &mut super::runtime::Runtime,
    code: crossterm::event::KeyCode,
    modifiers: crossterm::event::KeyModifiers,
) {
    runtime.key(crossterm::event::KeyEvent::new(code, modifiers));
}

#[test]
fn alt_down_with_a_draft_jumps_and_the_draft_waits_in_the_parent() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("root-session".into()), tx);
    runtime.state.connected = true;
    runtime.state.role_changed("controller");
    runtime.state.replace_sessions(family()).unwrap();
    runtime.state.list_open = false;
    runtime.composer.handle_paste("my draft".into());
    key(&mut runtime, KeyCode::Down, KeyModifiers::ALT);
    assert_eq!(
        runtime.state.selected.as_deref(),
        Some("grandchild-session")
    );
    assert!(
        runtime.composer.is_empty(),
        "the draft stays with the parent"
    );
    runtime.composer.handle_paste("child draft".into());
    key(&mut runtime, KeyCode::Up, KeyModifiers::ALT);
    key(&mut runtime, KeyCode::Up, KeyModifiers::ALT);
    assert_eq!(runtime.state.selected.as_deref(), Some("root-session"));
    while rx.try_recv().is_ok() {}
    key(&mut runtime, KeyCode::Enter, KeyModifiers::NONE);
    let prompt = rx.try_recv().unwrap();
    assert_eq!(prompt["method"], "_nori/session/prompt");
    assert_eq!(prompt["params"]["session_id"], "root-session");
    assert_eq!(prompt["params"]["prompt"][0]["text"], "my draft");
    key(&mut runtime, KeyCode::Down, KeyModifiers::ALT);
    assert_eq!(
        runtime.state.selected.as_deref(),
        Some("grandchild-session")
    );
    while rx.try_recv().is_ok() {}
    key(&mut runtime, KeyCode::Enter, KeyModifiers::NONE);
    let prompt = rx.try_recv().unwrap();
    assert_eq!(prompt["params"]["session_id"], "grandchild-session");
    assert_eq!(prompt["params"]["prompt"][0]["text"], "child draft");
}

#[test]
fn a_large_pasted_draft_survives_a_jump_and_back_whole() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("root-session".into()), tx);
    runtime.state.connected = true;
    runtime.state.role_changed("controller");
    runtime.state.replace_sessions(family()).unwrap();
    runtime.state.list_open = false;
    let pasted = "x".repeat(1500);
    runtime.composer.handle_paste(pasted.clone());
    key(&mut runtime, KeyCode::Down, KeyModifiers::ALT);
    key(&mut runtime, KeyCode::Up, KeyModifiers::ALT);
    key(&mut runtime, KeyCode::Up, KeyModifiers::ALT);
    assert_eq!(runtime.state.selected.as_deref(), Some("root-session"));
    while rx.try_recv().is_ok() {}
    key(&mut runtime, KeyCode::Enter, KeyModifiers::NONE);
    let prompt = rx.try_recv().unwrap();
    assert_eq!(prompt["params"]["prompt"][0]["text"], pasted.as_str());
}

#[test]
fn text_typed_before_choosing_a_session_goes_with_the_chosen_session() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(None, tx);
    runtime.state.connected = true;
    runtime.state.role_changed("controller");
    runtime.state.replace_sessions(family()).unwrap();
    runtime.composer.handle_paste("first words".into());
    runtime.state.list_open = true;
    runtime.composer.clear();
    runtime.composer.handle_paste("first words".into());
    runtime.open_session("other-session".into());
    while rx.try_recv().is_ok() {}
    key(&mut runtime, KeyCode::Enter, KeyModifiers::NONE);
    let prompt = rx.try_recv().unwrap();
    assert_eq!(prompt["params"]["session_id"], "other-session");
    assert_eq!(prompt["params"]["prompt"][0]["text"], "first words");
}

#[test]
fn a_top_level_header_shows_its_family_status() {
    let rendered = screen(&viewing("root-session"));
    assert!(
        rendered.contains("mock-model · Ship tree-row picker · waiting on you"),
        "{rendered}"
    );
}

#[test]
fn done_reads_apart_from_closed_in_the_session_list() {
    let mut state = AttachState {
        connected: true,
        ..Default::default()
    };
    state
        .replace_sessions(json!({"sessions":[
            {"session_id":"done-session","agent":"mock-model","status":"done","title":"Finished child"},
            {"session_id":"closed-session","agent":"mock-model","status":"closed","title":"Closed by you"}
        ]}))
        .unwrap();
    state.list_open = true;
    let area = ratatui::layout::Rect::new(0, 0, 90, 18);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    super::render::render(
        &state,
        &crate::ComposerInput::for_attachment(),
        area,
        &mut buffer,
    );
    let style_of = |title: &str, word: &str| {
        (0..area.height)
            .find_map(|y| {
                let cells: Vec<&str> = (0..area.width).map(|x| buffer[(x, y)].symbol()).collect();
                let row = cells.concat();
                if !row.contains(title) {
                    return None;
                }
                let x = (0..cells.len())
                    .find(|x| cells[*x..].concat().starts_with(&format!("{word} ")))?;
                Some(buffer[(u16::try_from(x).unwrap(), y)].style())
            })
            .unwrap_or_else(|| panic!("{word} on the {title} row"))
    };
    let done = style_of("Finished child", "done");
    let closed = style_of("Closed by you", "closed");
    assert_eq!(
        closed.fg,
        Some(ratatui::style::Color::DarkGray),
        "closed stays muted: {closed:?}"
    );
    assert_ne!(done.fg, closed.fg, "done reads apart from closed: {done:?}");
}

#[test]
fn a_finished_child_ends_by_pointing_back_to_its_parent() {
    let mut state = viewing("child-session");
    let mut sessions = family();
    sessions["sessions"][1]["status"] = json!("done");
    state.replace_sessions(sessions).unwrap();
    state.list_open = false;
    state
        .apply(SessionEnvelope {
            conversation_id: "child-session".into(),
            agent: "mock-model-alt".into(),
            seq: 1,
            event: nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionEnded(
                nori_protocol::SessionEnded {
                    reason: nori_protocol::SessionEndReason::Closed,
                    message: None,
                },
            )),
        })
        .unwrap();
    let rendered = screen(&state);
    assert!(
        rendered.contains("This child finished its task · alt+up to its parent"),
        "{rendered}"
    );
    assert!(!rendered.contains("/new to start"), "{rendered}");
    let mut sessions = family();
    sessions["sessions"][1]["status"] = json!("closed");
    state.replace_sessions(sessions).unwrap();
    state.list_open = false;
    let closed = screen(&state);
    assert!(
        closed.contains("This child session ended · alt+up to its parent"),
        "{closed}"
    );
    assert!(!closed.contains("finished"), "{closed}");
    assert!(!closed.contains("/new to start"), "{closed}");
}

#[test]
fn a_delivered_result_fits_one_row_at_80_columns() {
    use nori_protocol::acp::v1 as acp;
    let mut state = AttachState {
        connected: true,
        selected: Some("parent-session".into()),
        list_open: false,
        ..Default::default()
    };
    let done: nori_protocol::Delegation = serde_json::from_value(json!({
        "request_id":"delegation-one", "parent_id":"parent-session", "child_id":"child-session",
        "parent_agent":"mock-model", "agent":"mock-model-alt",
        "task":"mock:request-permission:review-the-lexer-tokens",
        "status":"completed", "depth":1, "result":"Parser reviewed"
    }))
    .unwrap();
    let chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
        nori_protocol::delegation_result_prompt(&done),
    )))
    .message_id("delivery");
    state
        .apply(SessionEnvelope {
            conversation_id: "parent-session".into(),
            agent: "mock-model".into(),
            seq: 1,
            event: nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
                acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
                    "agent-session",
                    acp::SessionUpdate::UserMessageChunk(chunk),
                )),
            )),
        })
        .unwrap();
    let rendered = screen_at(&state, 80);
    let lines: Vec<&str> = rendered.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.contains("delegation result"))
        .unwrap_or_else(|| panic!("{rendered}"));
    assert!(lines[at].contains("mock:request"), "{rendered}");
    assert!(lines[at].contains("lexer-tokens · done"), "{rendered}");
    assert!(
        lines[at].trim_end().ends_with("lexer-tokens · done"),
        "the whole line sits on one row: {rendered}"
    );
}

fn shared_prefix_families() -> serde_json::Value {
    let task = |end: &str| {
        format!("mock:delegate mock-model mock:delegate mock-model mock:request-permission:{end}")
    };
    json!({"sessions":[
        {"session_id":"root-x","agent":"mock-model","status":"ready","family_status":"waiting_on_you","title":task("x"),"depth":0},
        {"session_id":"child-x","agent":"mock-model","status":"waiting_on_you","title":task("x"),"parent":"root-x","depth":1,"source":"session"},
        {"session_id":"root-y","agent":"mock-model","status":"ready","family_status":"waiting_on_you","title":task("y"),"depth":0},
        {"session_id":"child-y","agent":"mock-model","status":"waiting_on_you","title":task("y"),"parent":"root-y","depth":1,"source":"session"}
    ]})
}

#[test]
fn list_titles_sharing_a_long_prefix_keep_their_distinct_ends() {
    for width in [120, 80] {
        let mut state = AttachState {
            connected: true,
            selected: Some("child-x".into()),
            ..Default::default()
        };
        state.replace_sessions(shared_prefix_families()).unwrap();
        let rendered = screen_area(&state, &crate::ComposerInput::for_attachment(), width, 30);
        let list = &rendered[rendered.find("Session ").expect("list header")..];
        // Title rows in tree order: root-x, child-x, root-y, child-y.
        let rows: Vec<&str> = list
            .lines()
            .filter(|line| line.contains("● mock"))
            .collect();
        assert_eq!(rows.len(), 4, "{rendered}");
        for (row, end, other) in [
            (rows[0], ":x", ":y"),
            (rows[1], ":x", ":y"),
            (rows[2], ":y", ":x"),
            (rows[3], ":y", ":x"),
        ] {
            let title = row
                .split("  mock-model")
                .next()
                .unwrap_or_default()
                .trim_end()
                .trim_end_matches(" (current)");
            assert!(title.contains("mock"), "the start is kept: {row}");
            assert!(
                title.ends_with(end) && !title.contains(other),
                "the end that tells {row} apart is kept at {width}"
            );
            assert_eq!(row.matches('…').count(), 1, "cut once at {width}: {row}");
        }
        assert!(
            rows[1].contains("└─") && rows[1].contains("(current)"),
            "{rendered}"
        );
    }
}
