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
        .find(|line| line.contains("Review parser"))
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
