/// A runtime attached to a host, past its `initialize`, with the session list
/// closed so the composer has the screen.
fn attached(
    role: &str,
) -> (
    super::runtime::Runtime,
    tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>,
) {
    let cwd = std::env::temp_dir();
    let (mut runtime, rx) = connected_runtime(&cwd, folder_host());
    if role != "controller" {
        change_role(&mut runtime, role);
    }
    runtime.state.list_open = false;
    runtime.state.notice = None;
    (runtime, rx)
}

fn change_role(runtime: &mut super::runtime::Runtime, role: &str) {
    runtime
        .connection_event(crate::host_client::ConnectionEvent::Message(json!({
            "jsonrpc": "2.0",
            "method": "_nori/control/changed",
            "params": {"role": role},
        })))
        .unwrap();
}

fn type_keys(runtime: &mut super::runtime::Runtime, text: &str) {
    use crossterm::event::KeyEvent;
    for character in text.chars() {
        runtime.key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        std::thread::sleep(crate::ComposerInput::recommended_flush_delay());
        runtime.composer.flush_paste_burst_if_due();
    }
}

/// Returns false when the terminal detaches.
fn press_enter(runtime: &mut super::runtime::Runtime) -> bool {
    use crossterm::event::KeyEvent;
    runtime.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
}

fn runtime_screen(runtime: &super::runtime::Runtime, height: u16) -> String {
    screen_area(&runtime.state, &runtime.composer, 90, height)
}

fn sent_methods(rx: &mut tokio::sync::mpsc::UnboundedReceiver<serde_json::Value>) -> Vec<String> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter_map(|message| message["method"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn slash_opens_the_shared_command_menu_with_reasons_for_unavailable_commands() {
    let (mut runtime, _rx) = attached("controller");
    type_keys(&mut runtime, "/");
    let rendered = runtime_screen(&runtime, 30);
    for available in ["/new", "/sessions", "/delegate"] {
        assert!(rendered.contains(available), "{available}:\n{rendered}");
    }
    assert!(
        rendered.contains("/model") && rendered.contains("not available in nori attach yet"),
        "{rendered}"
    );
    insta::assert_snapshot!("attached_command_menu_controller", rendered);
}

#[test]
fn choosing_sessions_from_the_menu_opens_the_host_session_list() {
    let (mut runtime, mut rx) = attached("controller");
    type_keys(&mut runtime, "/ses");
    press_enter(&mut runtime);
    assert!(runtime.state.list_open, "{}", runtime_screen(&runtime, 18));
    assert!(runtime.composer.is_empty());
    assert_eq!(sent_methods(&mut rx), Vec::<String>::new());
}

#[test]
fn an_unavailable_command_is_refused_with_the_menu_reason_and_never_leaves_the_terminal() {
    let (mut runtime, mut rx) = attached("controller");
    runtime.state.selected = Some("session-alpha".into());
    for (typed, menu_choice, reason) in [
        ("/model", false, "not available in nori attach yet"),
        ("/model gpt-large", false, "not available in nori attach yet"),
        ("/compact", false, "not available on this host yet"),
        ("/comp", true, "not available on this host yet"),
    ] {
        runtime.state.notice = None;
        runtime.composer.clear();
        if menu_choice {
            type_keys(&mut runtime, typed);
            press_enter(&mut runtime);
        } else {
            submit(&mut runtime, typed);
        }
        let notice = runtime.state.notice.clone().unwrap_or_default();
        let name = typed.split(' ').next().unwrap_or(typed);
        assert!(
            notice.starts_with(name) && notice.contains(reason),
            "{typed}: {notice}"
        );
        assert_eq!(sent_methods(&mut rx), Vec::<String>::new(), "{typed}");
    }
}

#[test]
fn a_watcher_sees_controller_commands_greyed() {
    let (mut runtime, _rx) = attached("watcher");
    type_keys(&mut runtime, "/");
    let rendered = runtime_screen(&runtime, 30);
    assert!(
        rendered.contains("watching · Alt+T to take control"),
        "{rendered}"
    );
    insta::assert_snapshot!("attached_command_menu_watcher", rendered);
}

#[test]
fn a_watcher_is_refused_controller_commands_and_keeps_the_text() {
    let (mut runtime, mut rx) = attached("watcher");
    runtime.state.selected = Some("session-alpha".into());
    for command in ["/new", "/close", "/resume", "/delegate helper check the parser"] {
        runtime.state.notice = None;
        runtime.composer.clear();
        submit(&mut runtime, command);
        assert!(
            runtime
                .state
                .notice
                .as_deref()
                .is_some_and(|notice| notice.contains("watching · Alt+T to take control")),
            "{command}: {:?}",
            runtime.state.notice
        );
        assert_eq!(runtime.composer.text(), command);
        assert_eq!(sent_methods(&mut rx), Vec::<String>::new(), "{command}");
    }
}

#[test]
fn a_watcher_can_list_the_host_sessions_and_detach() {
    let (mut runtime, mut rx) = attached("watcher");
    submit(&mut runtime, "/sessions");
    assert!(runtime.state.list_open);
    runtime.state.list_open = false;
    type_keys(&mut runtime, "/qui");
    assert!(!press_enter(&mut runtime), "/quit detaches while watching");
    assert_eq!(sent_methods(&mut rx), Vec::<String>::new());
}

#[test]
fn taking_control_enables_controller_commands() {
    let (mut runtime, mut rx) = attached("watcher");
    change_role(&mut runtime, "controller");
    type_keys(&mut runtime, "/new");
    press_enter(&mut runtime);
    assert_eq!(sent_methods(&mut rx), vec!["_nori/session/new"]);
}

#[test]
fn an_open_menu_follows_a_role_change() {
    let (mut runtime, _rx) = attached("controller");
    type_keys(&mut runtime, "/");
    assert!(!runtime_screen(&runtime, 30).contains("Alt+T to take control"));
    change_role(&mut runtime, "watcher");
    let rendered = runtime_screen(&runtime, 30);
    assert!(
        rendered.contains("watching · Alt+T to take control"),
        "{rendered}"
    );
}

#[test]
fn while_reconnecting_host_commands_are_greyed_and_the_draft_is_kept() {
    let (mut runtime, mut rx) = attached("controller");
    runtime
        .connection_event(crate::host_client::ConnectionEvent::Disconnected(
            "connection reset".into(),
        ))
        .unwrap();
    while rx.try_recv().is_ok() {}
    type_keys(&mut runtime, "/ne");
    let rendered = runtime_screen(&runtime, 18);
    assert!(rendered.contains("reconnecting"), "{rendered}");
    runtime.composer.clear();
    submit(&mut runtime, "/new");
    assert_eq!(sent_methods(&mut rx), Vec::<String>::new());
    assert_eq!(runtime.composer.text(), "/new");
    assert!(
        runtime
            .state
            .notice
            .as_deref()
            .is_some_and(|notice| notice.starts_with("Reconnecting")),
        "{:?}",
        runtime.state.notice
    );
}

#[test]
fn delegate_with_arguments_reaches_the_host() {
    let (mut runtime, mut rx) = attached("controller");
    runtime.state.selected = Some("session-alpha".into());
    submit(&mut runtime, "/delegate helper check the parser");
    let request = rx.try_recv().unwrap();
    assert_eq!(request["method"], "_nori/session/delegate");
    assert_eq!(request["params"]["agent"], "helper");
    assert_eq!(request["params"]["task"], "check the parser");
}

#[test]
fn attach_aliases_keep_working() {
    let (mut runtime, _rx) = attached("controller");
    runtime.state.selected = Some("session-alpha".into());
    submit(&mut runtime, "/cancel");
    assert!(
        runtime
            .state
            .notice
            .as_deref()
            .is_some_and(|notice| notice.contains("No active turn")),
        "{:?}",
        runtime.state.notice
    );
    runtime.composer.handle_paste("/detach".into());
    assert!(!press_enter(&mut runtime), "/detach leaves the host");
}

#[test]
fn a_bare_agent_command_that_loosely_matches_a_builtin_still_reaches_the_agent() {
    let (mut runtime, mut rx) = attached("controller");
    runtime.state.selected = Some("session-alpha".into());
    type_keys(&mut runtime, "/review");
    press_enter(&mut runtime);
    let request = rx.try_recv().expect("/review is sent");
    assert_eq!(request["method"], "_nori/session/prompt");
    assert!(request["params"].to_string().contains("/review"), "{request}");
}

#[test]
fn the_first_escape_closes_the_menu_and_the_next_cancels_the_turn() {
    let (mut runtime, mut rx) = working_runtime();
    runtime.state.list_open = false;
    type_keys(&mut runtime, "/");
    let escape = |runtime: &mut super::runtime::Runtime| {
        runtime.key(crossterm::event::KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        ));
    };
    escape(&mut runtime);
    let rendered = runtime_screen(&runtime, 30);
    assert!(!rendered.contains("/delegate"), "{rendered}");
    assert_eq!(runtime.composer.text(), "/");
    assert_eq!(sent_methods(&mut rx), Vec::<String>::new());

    escape(&mut runtime);
    assert_eq!(
        sent_methods(&mut rx),
        vec!["_nori/session/cancel_observed"]
    );
}

#[test]
fn a_pasted_command_opens_a_menu_that_already_reflects_the_connection() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(None, tx);
    runtime.state.list_open = false;
    runtime.composer.handle_paste("/ne".into());
    let rendered = runtime_screen(&runtime, 18);
    assert!(rendered.contains("/new") && rendered.contains("reconnecting"), "{rendered}");
}

#[test]
fn an_agent_command_the_terminal_does_not_know_still_reaches_the_agent() {
    let (mut runtime, mut rx) = attached("controller");
    runtime.state.selected = Some("session-alpha".into());
    submit(&mut runtime, "/review the parser");
    let request = rx.try_recv().unwrap();
    assert_eq!(request["method"], "_nori/session/prompt");
    assert!(
        request["params"].to_string().contains("/review the parser"),
        "{request}"
    );
}
