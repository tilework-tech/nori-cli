use super::*;

/// Deliver a prompt completion through the real client-event entry point.
fn deliver_completion(chat: &mut ChatWidget, failure: Option<crate::presentation::TurnFailure>) {
    chat.handle_client_event(crate::presentation::ClientEvent::PromptCompleted(
        crate::presentation::PromptCompleted {
            stop_reason: nori_protocol::acp::v1::StopReason::Cancelled,
            last_agent_message: None,
            failure,
        },
    ));
}

fn history_text(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    drain_insert_history(rx)
        .iter()
        .map(|cell| lines_to_single_string(cell))
        .collect()
}

/// Scan the app-event stream for the next loop re-fire, if any.
fn next_loop_iteration(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Option<(i32, i32)> {
    while let Ok(ev) = rx.try_recv() {
        if let AppEvent::LoopIteration {
            remaining, total, ..
        } = ev
        {
            return Some((remaining, total));
        }
    }
    None
}

fn replay_message_update(
    update: nori_protocol::acp::v1::SessionUpdate,
) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
        nori_protocol::acp::v1::AgentNotification::SessionNotification(
            nori_protocol::acp::v1::SessionNotification::new("session", update),
        ),
    ))
}

fn replay_text_chunk(
    stream: crate::presentation::MessageStream,
    message_id: &str,
    text: &str,
) -> nori_protocol::SessionEvent {
    let chunk = nori_protocol::acp::v1::ContentChunk::new(
        nori_protocol::acp::v1::ContentBlock::Text(nori_protocol::acp::v1::TextContent::new(text)),
    )
    .message_id(message_id);
    let update = match stream {
        crate::presentation::MessageStream::User => {
            nori_protocol::acp::v1::SessionUpdate::UserMessageChunk(chunk)
        }
        crate::presentation::MessageStream::Answer => {
            nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(chunk)
        }
        crate::presentation::MessageStream::Reasoning => {
            nori_protocol::acp::v1::SessionUpdate::AgentThoughtChunk(chunk)
        }
    };
    replay_message_update(update)
}

#[test]
fn replayed_turns_render_as_separate_ordered_history() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;

    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayStarted(
            nori_protocol::ReplayStarted {
                source: nori_protocol::ReplaySource::Agent,
            },
        )),
    );
    for event in [
        replay_text_chunk(
            crate::presentation::MessageStream::User,
            "user-1",
            "First question",
        ),
        replay_text_chunk(
            crate::presentation::MessageStream::Answer,
            "answer-1",
            "First ",
        ),
        replay_text_chunk(
            crate::presentation::MessageStream::Answer,
            "answer-1",
            "answer",
        ),
        replay_text_chunk(
            crate::presentation::MessageStream::User,
            "user-2",
            "Second question",
        ),
        replay_text_chunk(
            crate::presentation::MessageStream::Answer,
            "answer-2",
            "Second answer",
        ),
    ] {
        chat.handle_session_event(generation, event);
    }
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayFinished),
    );

    let expected = [
        "First question",
        "First answer",
        "Second question",
        "Second answer",
    ];
    let rendered = drain_insert_history(&mut rx)
        .into_iter()
        .map(|lines| lines_to_single_string(&lines))
        .collect::<Vec<_>>();
    let message_cells = rendered
        .iter()
        .filter(|cell| expected.iter().any(|message| cell.contains(message)))
        .collect::<Vec<_>>();

    assert_eq!(message_cells.len(), expected.len(), "{rendered:#?}");
    for (index, expected_message) in expected.iter().enumerate() {
        assert!(
            message_cells[index].contains(expected_message),
            "{rendered:#?}"
        );
        for other_message in expected
            .iter()
            .filter(|message| message != &expected_message)
        {
            assert!(
                !message_cells[index].contains(other_message),
                "{rendered:#?}"
            );
        }
        assert_eq!(
            rendered
                .iter()
                .map(|cell| cell.matches(expected_message).count())
                .sum::<usize>(),
            1
        );
    }
    insta::assert_snapshot!(message_cells.into_iter().cloned().collect::<String>(), @r"
› First question


• First answer


› Second question


• Second answer
");
}

#[test]
fn stale_session_events_do_not_reach_the_current_widget() {
    let (old_chat, _old_rx, _old_op_rx) = make_chatwidget_manual();
    let stale_generation = old_chat.session_generation;
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let current_generation = chat.session_generation;

    chat.handle_session_event(
        stale_generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::Notice(
            nori_protocol::Notice {
                message: "stale session output".to_string(),
            },
        )),
    );
    chat.handle_session_event(
        current_generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::Notice(
            nori_protocol::Notice {
                message: "current session output".to_string(),
            },
        )),
    );

    let rendered = history_text(&mut rx);
    assert!(!rendered.contains("stale session output"), "{rendered}");
    assert!(rendered.contains("current session output"), "{rendered}");

    chat.handle_session_event(
        stale_generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionEnded(
            nori_protocol::SessionEnded {
                reason: nori_protocol::SessionEndReason::Shutdown,
                message: None,
            },
        )),
    );

    let stale_terminal_events = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        stale_terminal_events
            .iter()
            .all(|event| !matches!(event, AppEvent::ExitRequest)),
        "a stale shutdown must not exit the replacement session: {stale_terminal_events:#?}"
    );

    chat.handle_session_event(
        current_generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionEnded(
            nori_protocol::SessionEnded {
                reason: nori_protocol::SessionEndReason::Shutdown,
                message: None,
            },
        )),
    );
    let current_terminal_events = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();
    assert_eq!(
        current_terminal_events
            .iter()
            .filter(|event| matches!(event, AppEvent::ExitRequest))
            .count(),
        1,
        "the current shutdown must still exit: {current_terminal_events:#?}"
    );
}

#[tokio::test]
async fn session_shutdown_uses_live_handle_without_a_conversation_id() {
    let temp = tempfile::tempdir().expect("create session directory");
    let config = nori_config::NoriConfig {
        active_agent: "mock-model".to_string(),
        cwd: temp.path().to_path_buf(),
        nori_home: temp.path().to_path_buf(),
        ..Default::default()
    };
    let mut session =
        nori_harness::runtime::launch_session(nori_harness::runtime::SessionLaunchSpec {
            config: std::sync::Arc::new(config),
            cli_version: "tui-test".to_string(),
            session_context: None,
            initial_context: None,
            resume: None,
        });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while !matches!(
            session.events.recv().await,
            Some(nori_protocol::SessionEvent::Nori(
                nori_protocol::NoriEvent::SessionStarted(_)
            ))
        ) {}
    })
    .await
    .expect("mock session should start");

    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    assert_eq!(chat.conversation_id(), None);
    chat.harness_handle = Some(session.handle.clone());

    chat.shutdown_harness_session();

    let ended = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            if let Some(nori_protocol::SessionEvent::Nori(
                nori_protocol::NoriEvent::SessionEnded(ended),
            )) = session.events.recv().await
            {
                break ended;
            }
        }
    })
    .await
    .expect("shutdown should reach the live harness session");
    assert_eq!(ended.reason, nori_protocol::SessionEndReason::Shutdown);
}

/// A transient (retryable) turn failure must leave the loop armed: the next
/// iteration fires when the turn completes.
#[test]
fn loop_survives_retryable_failure() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);

    deliver_completion(&mut chat, Some(crate::presentation::TurnFailure::Retryable));

    assert_eq!(next_loop_iteration(&mut rx), Some((4, 10)));
}

/// A fatal turn failure disarms the loop before it can re-fire.
#[test]
fn loop_stops_on_fatal_failure() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.first_prompt_text = Some("hi".to_string());
    chat.loop_remaining = Some(5);
    chat.loop_total = Some(10);

    deliver_completion(&mut chat, Some(crate::presentation::TurnFailure::Fatal));

    assert_eq!(chat.loop_remaining, None);
    assert_eq!(next_loop_iteration(&mut rx), None);
}

/// A turn that ended in a failure must not also render the generic
/// "Conversation interrupted" cell — the surfaced error already explains it.
#[test]
fn failure_completion_does_not_add_interrupted_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    deliver_completion(&mut chat, Some(crate::presentation::TurnFailure::Fatal));

    assert!(
        !history_text(&mut rx).contains("Conversation interrupted"),
        "a failure completion should not add the interrupted cell"
    );
}

/// A genuine user cancellation (no failure) still renders the interrupted cell.
#[test]
fn user_cancellation_adds_interrupted_cell() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    deliver_completion(&mut chat, None);

    assert!(
        history_text(&mut rx).contains("Conversation interrupted"),
        "a clean user cancellation should add the interrupted cell"
    );
}
