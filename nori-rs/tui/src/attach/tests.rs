use nori_protocol::SessionEnvelope;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::state::AttachState;

fn message(session: &str, seq: i64, text: &str) -> SessionEnvelope {
    use nori_protocol::acp::v1 as acp;
    let chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text)))
        .message_id("answer");
    let notification = acp::SessionNotification::new(
        "agent-session",
        acp::SessionUpdate::AgentMessageChunk(chunk),
    );
    SessionEnvelope {
        conversation_id: session.into(),
        agent: "test-agent".into(),
        seq,
        event: nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
            acp::AgentNotification::SessionNotification(notification),
        )),
    }
}

#[test]
fn replay_continues_the_conversation_without_duplicate_chunks() {
    let mut state = AttachState::default();
    state.apply(message("one", 1, "Hello ")).unwrap();
    state.apply(message("one", 1, "Hello ")).unwrap();
    state.apply(message("one", 2, "world")).unwrap();
    assert_eq!(state.transcript_text("one"), "Hello world");
    assert_eq!(state.cursors.get("one"), Some(&2));
}

#[test]
fn replay_gaps_are_visible_and_never_advance_the_cursor() {
    let mut state = AttachState::default();
    state.apply(message("one", 1, "Hello")).unwrap();
    assert!(state.apply(message("one", 3, "lost middle")).is_err());
    assert_eq!(state.cursors.get("one"), Some(&1));
}

#[test]
fn watcher_cannot_submit_and_handoff_enables_the_same_draft() {
    let mut state = AttachState::default();
    state.role_changed("watcher");
    assert!(state.prompt("one", "my draft").is_err());
    state.role_changed("controller");
    assert!(state.prompt("one", "my draft").is_ok());
    assert!(
        state.transcript_text("one").is_empty(),
        "only the host's canonical echo renders a prompt"
    );
}

#[test]
fn independent_session_sequences_do_not_drop_each_others_events() {
    let mut state = AttachState::default();
    state.apply(message("one", 1, "Alpha")).unwrap();
    state.apply(message("two", 1, "Beta")).unwrap();
    assert_eq!(state.cursors.get("one"), Some(&1));
    assert_eq!(state.cursors.get("two"), Some(&1));
    assert_eq!(state.transcript_text("one"), "Alpha");
    assert_eq!(state.transcript_text("two"), "Beta");
}

#[test]
fn unresolved_permission_replays_and_remote_answer_clears_it() {
    use nori_protocol::acp::v1 as acp;
    let mut state = AttachState::default();
    let request: acp::RequestPermissionRequest = serde_json::from_value(json!({
        "sessionId":"agent-session", "toolCall":{"toolCallId":"tool", "title":"Write a file"},
        "options":[{"optionId":"allow", "name":"Allow once", "kind":"allow_once"}]
    }))
    .unwrap();
    let request_id = acp::RequestId::Str("permission".into());
    state
        .apply(SessionEnvelope {
            conversation_id: "one".into(),
            agent: "test-agent".into(),
            seq: 1,
            event: nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Request {
                request_id: request_id.clone(),
                request: acp::AgentRequest::RequestPermissionRequest(request),
            }),
        })
        .unwrap();
    assert_eq!(state.permission_count("one"), 1);
    state.connected = true;
    state.selected = Some("one".into());
    state.list_open = false;
    state.role_changed("controller");
    insta::assert_snapshot!("attached_permission_controller", screen(&state));
    state.role_changed("watcher");
    insta::assert_snapshot!("attached_permission_watcher", screen(&state));
    state
        .apply(SessionEnvelope {
            conversation_id: "one".into(),
            agent: "test-agent".into(),
            seq: 2,
            event: nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::RequestResolved(
                nori_protocol::RequestResolved {
                    request_id,
                    answered_by: "another controller".into(),
                },
            )),
        })
        .unwrap();
    assert_eq!(state.permission_count("one"), 0);
}

fn screen(state: &AttachState) -> String {
    screen_with_composer(state, &crate::ComposerInput::for_attachment())
}

fn screen_with_composer(state: &AttachState, composer: &crate::ComposerInput) -> String {
    let area = ratatui::layout::Rect::new(0, 0, 90, 18);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    super::render::render(state, composer, area, &mut buffer);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn attached_screens_identify_control_and_reconnection() {
    let mut state = AttachState {
        connected: true,
        ..Default::default()
    };
    state.role_changed("controller");
    let controlling = screen(&state);
    assert!(controlling.contains("CONTROLLING"), "{controlling}");
    assert!(
        controlling.contains("/new"),
        "empty host must offer a way to start"
    );
    insta::assert_snapshot!("attached_empty_controller", controlling);
    state.role_changed("watcher");
    let watching = screen(&state);
    assert!(
        watching.contains("WATCHING") && watching.contains("read-only"),
        "{watching}"
    );
    insta::assert_snapshot!("attached_watcher", watching);
    state.connected = false;
    let reconnecting = screen(&state);
    assert!(reconnecting.contains("RECONNECTING"), "{reconnecting}");
    insta::assert_snapshot!("attached_reconnecting", reconnecting);
}

#[test]
fn host_session_list_renders_titles_agents_and_status() {
    let mut state = AttachState {
        connected: true,
        ..Default::default()
    };
    state.role_changed("controller");
    state.replace_sessions(json!({"sessions":[
        {"session_id":"session-alpha","agent":"mock-model","status":"ready","title":"Review the parser","age_seconds":120},
        {"session_id":"session-beta","agent":"other-agent","status":"waiting_on_you","title":"Update tests","age_seconds":60}
    ]})).unwrap();
    let rendered = screen(&state);
    assert!(
        rendered.contains("Review the parser") && rendered.contains("waiting on you"),
        "{rendered}"
    );
    insta::assert_snapshot!("attached_host_sessions", rendered);
}

#[test]
fn attachment_composer_submits_frontend_commands_as_text() {
    for text in [
        "/new",
        "/new custom-agent",
        "/close",
        "/resume",
        "/cancel",
        "/detach",
        "/quit",
        "/sessions",
    ] {
        let mut composer = crate::ComposerInput::for_attachment();
        composer.handle_paste(text.into());
        let action = composer.input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        assert!(
            matches!(action, crate::ComposerAction::Submitted(actual) if actual == text),
            "{text} must reach the attachment frontend"
        );
    }
}

#[test]
fn resumed_transcript_replaces_the_existing_presentation_without_repeating_turns() {
    let mut state = AttachState::default();
    state.apply(message("one", 1, "Original answer")).unwrap();
    let marker = |seq, event| SessionEnvelope {
        conversation_id: "one".into(),
        agent: "test-agent".into(),
        seq,
        event: nori_protocol::SessionEvent::Nori(event),
    };
    state
        .apply(marker(
            2,
            nori_protocol::NoriEvent::SessionStarted(nori_protocol::SessionStarted {
                transcript_id: Some("one".into()),
                acp_session_id: "agent-session".into(),
                cwd: "/tmp".into(),
                transcript_path: None,
                history_log_id: 0,
                history_entry_count: 0,
                config_options: vec![],
            }),
        ))
        .unwrap();
    state
        .apply(marker(
            3,
            nori_protocol::NoriEvent::ReplayStarted(nori_protocol::ReplayStarted {
                source: nori_protocol::ReplaySource::Agent,
            }),
        ))
        .unwrap();
    state
        .apply(marker(
            4,
            nori_protocol::NoriEvent::ReplayStarted(nori_protocol::ReplayStarted {
                source: nori_protocol::ReplaySource::Transcript,
            }),
        ))
        .unwrap();
    state.apply(message("one", 5, "Original answer")).unwrap();
    state
        .apply(marker(6, nori_protocol::NoriEvent::ReplayFinished))
        .unwrap();
    state.apply(message("one", 7, "New live answer")).unwrap();
    assert_eq!(
        state.transcript_text("one"),
        "Original answer\n\nNew live answer"
    );
    assert_eq!(state.cursors.get("one"), Some(&7));
}

#[test]
fn failed_session_resume_hotkey_keeps_the_selected_host_identity() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("running-other".into()), tx);
    runtime.state.connected = true;
    runtime.state.role_changed("controller");
    runtime.state.list_open = true;
    runtime.state.replace_sessions(json!({"sessions":[{"session_id":"one","agent":"mock-model","status":"failed","title":"Recover this"}]})).unwrap();
    runtime.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
    let request = rx.try_recv().expect("Ctrl+R sends a host resume request");
    assert_eq!(request["method"], "_nori/session/resume");
    assert_eq!(request["params"]["session_id"], "one");
    runtime.state.list_open = false;
    runtime.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert!(
        screen(&runtime.state).contains("Recover this"),
        "Left on an empty composer opens the session list"
    );
}

#[test]
fn ending_or_cancelling_a_turn_removes_its_permission_choices() {
    use nori_protocol::acp::v1 as acp;
    for phase in [
        nori_protocol::SessionPhase::Idle,
        nori_protocol::SessionPhase::Cancelling {
            request_id: acp::RequestId::Str("turn".into()),
        },
    ] {
        let mut state = AttachState::default();
        let request = serde_json::from_value(json!({"sessionId":"agent-session", "toolCall":{"toolCallId":"tool","title":"Write"},"options":[{"optionId":"allow","name":"Allow","kind":"allow_once"}]})).unwrap();
        state
            .apply(SessionEnvelope {
                conversation_id: "one".into(),
                agent: "test".into(),
                seq: 1,
                event: nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Request {
                    request_id: acp::RequestId::Str("permission".into()),
                    request: acp::AgentRequest::RequestPermissionRequest(request),
                }),
            })
            .unwrap();
        assert_eq!(state.permission_count("one"), 1);
        state
            .apply(SessionEnvelope {
                conversation_id: "one".into(),
                agent: "test".into(),
                seq: 2,
                event: nori_protocol::SessionEvent::Nori(
                    nori_protocol::NoriEvent::SessionPhaseChanged(phase),
                ),
            })
            .unwrap();
        assert_eq!(state.permission_count("one"), 0);
    }
}

#[test]
fn old_subscription_failure_does_not_block_the_new_selection() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("expired".into()), tx);
    runtime.state.connected = true;
    runtime.state.role_changed("controller");
    runtime.subscribe_selected();
    let old = rx.try_recv().unwrap();
    runtime.state.selected = Some("healthy".into());
    runtime.subscribe_selected();
    runtime
        .connection_event(super::connection::ConnectionEvent::Message(json!({
            "jsonrpc":"2.0", "id":old["id"], "error":{"code":-32000,"message":"Replay expired"}
        })))
        .unwrap();
    assert!(
        runtime.state.replay_error.is_none(),
        "a previous selection must not block the healthy session"
    );
    assert!(
        runtime.state.notice.is_none(),
        "the error belongs to the old selection"
    );
}

#[test]
fn session_list_uses_the_configured_binding_instead_of_the_default() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("one".into()), tx);
    runtime.set_session_list_binding(nori_config::HotkeyBinding::from_str("alt+g"));
    runtime.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::ALT));
    assert!(
        !runtime.state.list_open,
        "rebound default must not open the list"
    );
    runtime.key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT));
    assert!(runtime.state.list_open, "configured binding opens the list");
}

#[test]
fn current_session_row_keeps_short_id_visible() {
    let mut state = AttachState {
        connected: true,
        selected: Some("abcd1234-session".into()),
        ..Default::default()
    };
    state.replace_sessions(json!({"sessions":[
        {"session_id":"abcd1234-session","agent":"mock-model","status":"ready","title":"Review parser","age_seconds":120}
    ]})).unwrap();
    let rendered = screen(&state);
    let row = rendered
        .lines()
        .find(|line| line.contains("Review parser") && line.contains("2m"))
        .unwrap();
    assert!(row.contains("abcd1234"), "{rendered}");
    assert!(row.contains("current"), "{rendered}");
    insta::assert_snapshot!("attached_current_session_id", rendered);
}

#[test]
fn terminal_connection_failure_remains_visible_and_blocks_requests() {
    use super::connection::ConnectionEvent;
    use super::runtime::Runtime;
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    let (requests, mut received) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = Runtime::new(Some("one".into()), requests);
    runtime
        .state
        .apply(message("one", 1, "Preserved history"))
        .unwrap();
    runtime
        .connection_event(ConnectionEvent::Stopped(
            "Host restarted; sessions were lost. Re-run nori attach.".into(),
        ))
        .unwrap();
    runtime.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    runtime.key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT));
    runtime.composer.handle_paste("My draft".into());
    runtime.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    runtime.refresh_list();
    runtime.subscribe_selected();
    assert!(received.try_recv().is_err());
    assert_eq!(runtime.state.transcript_text("one"), "Preserved history");
    assert!(!runtime.composer.is_empty());
    let rendered = screen(&runtime.state);
    assert!(
        rendered.contains("Host restarted") && rendered.contains("nori attach"),
        "{rendered}"
    );
    assert!(
        !rendered.contains("RECONNECTING") && !rendered.contains("sessions continue"),
        "{rendered}"
    );
    insta::assert_snapshot!("attached_host_restarted", rendered);
}

#[test]
fn stopped_explanation_remains_visible_above_a_tall_draft() {
    let state = AttachState {
        stopped: Some("Host restarted; sessions were lost. Re-run nori attach.".into()),
        ..Default::default()
    };
    let mut composer = crate::ComposerInput::for_attachment();
    composer.handle_paste("Preserved draft line\n".repeat(30));
    let rendered = screen_with_composer(&state, &composer);
    assert!(
        rendered.contains("Host restarted") && rendered.contains("nori attach"),
        "{rendered}"
    );
    assert!(!composer.is_empty());
    insta::assert_snapshot!("attached_host_restarted_with_draft", rendered);
}

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
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
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
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    for (key, approved) in [('1', true), ('2', false)] {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut runtime = super::runtime::Runtime::new(Some("parent-session".into()), tx);
        runtime.state.connected = true;
        runtime
            .state
            .apply(delegation_event(1, "awaiting_approval"))
            .unwrap();
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
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
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
    let rendered = screen(&state);
    let titles: Vec<_> = rendered
        .lines()
        .filter_map(|line| {
            ["Ship", "Port", "Write", "Update", "Spike"]
                .iter()
                .find(|word| line.contains(*word))
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
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
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
fn alt_up_in_a_top_level_session_stays_put() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
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
fn next_waiting_uses_the_configured_binding() {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
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
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut runtime = super::runtime::Runtime::new(Some("parent-session".into()), tx);
    runtime.state.connected = true;
    runtime.state.role_changed("controller");
    runtime
        .state
        .apply(delegation_event(1, "awaiting_approval"))
        .unwrap();
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
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
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
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
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
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;
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
