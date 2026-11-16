use super::*;

fn delegation(status: &str) -> nori_protocol::SessionEvent {
    serde_json::from_value(serde_json::json!({"source":"nori","event":{"event_type":"delegation_changed","event":{
        "request_id":"delegation-one","parent_id":"00000000-0000-0000-0000-000000000001",
        "child_id":"00000000-0000-0000-0000-000000000002","parent_agent":"mock-model","agent":"mock-model-alt",
        "task":"Review the parser","depth":1,"status":status,"result":null
    }}})).unwrap()
}

#[test]
fn local_delegation_approval_identifies_parent_child_and_start_decline() {
    use crate::render::renderable::Renderable;
    let (mut chat, _rx, _) = make_chatwidget_manual();
    chat.config.multi_session = true;
    let generation = chat.session_generation();
    chat.handle_session_event(generation, delegation("awaiting_approval"));
    let area = ratatui::layout::Rect::new(0, 0, 100, chat.bottom_pane.desired_height(100));
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    chat.bottom_pane.render(area, &mut buffer);
    let rendered = (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("Start") && rendered.contains("Decline"),
        "{rendered}"
    );
    assert!(
        rendered.contains("mock-model-alt") && rendered.contains("parent"),
        "{rendered}"
    );
    assert!(
        rendered.contains("00000000-0000-0000-0000-000000000001")
            && rendered.contains("00000000-0000-0000-0000-000000000002"),
        "approval identifies both sessions: {rendered}"
    );
    insta::assert_snapshot!("local_delegation_approval", rendered);
}

#[test]
fn child_shutdown_preserves_terminal_and_history() {
    let (mut chat, mut rx, _) = make_chatwidget_manual();
    chat.config.multi_session = true;
    let id =
        nori_harness::ConversationId::from_string("00000000-0000-0000-0000-000000000002").unwrap();
    chat.managed_conversation_id = Some(id);
    chat.delegated_child = true;
    while rx.try_recv().is_ok() {}
    chat.handle_session_event(
        chat.session_generation(),
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionEnded(
            nori_protocol::SessionEnded {
                reason: nori_protocol::SessionEndReason::Shutdown,
                message: None,
            },
        )),
    );
    while let Ok(event) = rx.try_recv() {
        assert!(
            !matches!(
                event,
                AppEvent::ExitRequest | AppEvent::SessionClosed { .. }
            ),
            "a completed child must not exit or replace the visible session"
        );
    }
}

#[test]
fn local_delegate_rejects_flag_off_without_prompting_agent() {
    let (mut chat, mut rx, _) = make_chatwidget_manual();
    chat.submit_user_message_text("/delegate mock-model-alt inspect parser".into());
    let mut output = String::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            output.push_str(&lines_to_single_string(&cell.display_lines(120)));
        } else {
            assert!(
                !matches!(event, AppEvent::HarnessAction(_) | AppEvent::NewSession),
                "flag-off command cannot submit work"
            );
        }
    }
    assert!(output.contains("multi_session"), "{output}");
}

#[test]
fn forked_parent_refreshes_pending_approval_target() {
    let (mut chat, mut rx, _) = make_chatwidget_manual();
    chat.config.multi_session = true;
    let generation = chat.session_generation();
    chat.handle_session_event(generation, delegation("awaiting_approval"));
    let nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::DelegationChanged(mut changed)) =
        delegation("awaiting_approval")
    else {
        panic!("delegation fixture");
    };
    changed.parent_id = "00000000-0000-0000-0000-000000000003".into();
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::DelegationChanged(changed)),
    );
    chat.handle_key_event(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('1'),
        crossterm::event::KeyModifiers::NONE,
    ));
    let mut approved = None;
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::ResolveDelegation { parent, .. } = event {
            approved = Some(parent.to_string());
        }
    }
    assert_eq!(
        approved.as_deref(),
        Some("00000000-0000-0000-0000-000000000003")
    );
}

#[test]
fn delegation_result_and_parent_continuation_preserve_composer_draft() {
    let (mut chat, _rx, _) = make_chatwidget_manual();
    chat.config.multi_session = true;
    chat.set_composer_text("/sessions".into());
    let generation = chat.session_generation();
    chat.handle_session_event(generation, delegation("completed"));
    chat.on_task_started();
    chat.handle_client_event(crate::presentation::ClientEvent::MessageDelta(
        crate::presentation::MessageDelta {
            stream: crate::presentation::MessageStream::User,
            delta: "[Nori delegation result] Child completed".into(),
            message_id: Some("delegation-result".into()),
        },
    ));
    chat.on_task_complete(Some("Result received".into()));
    assert_eq!(chat.composer_text(), "/sessions");
}
