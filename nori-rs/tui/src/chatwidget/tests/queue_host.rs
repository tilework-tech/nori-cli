//! A session on the host resumes and takes back its queued messages through
//! the host, so any terminal attached to that host can do the same.

use super::*;
use crate::app_event_sender::HostReply;
use crate::host_client::HostCall;
use nori_protocol::QueuePause;
use pretty_assertions::assert_eq;
use serde_json::json;

fn host_session(chat: &mut ChatWidget) -> String {
    let id = nori_harness::ConversationId::new();
    chat.managed_conversation_id = Some(id);
    id.to_string()
}

fn show_paused(chat: &mut ChatWidget) {
    let generation = chat.session_generation;
    let entries = vec![
        nori_protocol::QueueEntry {
            id: "a".into(),
            text: "alpha".into(),
            editable: true,
        },
        nori_protocol::QueueEntry {
            id: "b".into(),
            text: "bravo".into(),
            editable: true,
        },
    ];
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::QueueChanged(
            nori_protocol::QueueSnapshot {
                prompts: entries.iter().map(|entry| entry.text.clone()).collect(),
                entries,
                paused: Some(QueuePause::Interrupted),
            },
        )),
    );
}

fn press(chat: &mut ChatWidget, code: KeyCode) {
    chat.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
}

/// The only host request the widget sent since the last look, and no
/// in-process harness action.
fn one_host_request(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> (HostCall, HostReply) {
    let mut requests = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HostRequest { call, reply } => requests.push((call, reply)),
            AppEvent::HarnessAction(action) => panic!("sent {action:?} in process"),
            _ => {}
        }
    }
    assert_eq!(
        requests.len(),
        1,
        "{:?}",
        requests.iter().map(|(call, _)| call).collect::<Vec<_>>()
    );
    requests.pop().unwrap()
}

/// Answer `reply` with `result` and hand the widget the reclaim outcome the
/// App would route back to it.
async fn answer(
    chat: &mut ChatWidget,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    reply: HostReply,
    result: anyhow::Result<serde_json::Value>,
) {
    reply.deliver(result);
    let event = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Some(AppEvent::QueuedPromptReclaimed {
                    event_id,
                    action,
                    outcome,
                    ..
                }) => return (event_id, action, outcome),
                Some(_) => {}
                None => panic!("the widget stopped sending"),
            }
        }
    })
    .await
    .unwrap();
    chat.on_queued_prompt_reclaimed(event.0, event.1, event.2);
}

#[tokio::test]
async fn enter_resumes_the_paused_queue_on_the_host() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let session = host_session(&mut chat);
    show_paused(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Enter);

    let (call, _) = one_host_request(&mut rx);
    assert!(
        matches!(&call, HostCall::ResumeQueue { session_id } if *session_id == session),
        "{call:?}"
    );
}

#[tokio::test]
async fn editing_takes_the_message_from_the_host_and_restores_it_once_taken() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let session = host_session(&mut chat);
    show_paused(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Enter);
    let (call, reply) = one_host_request(&mut rx);
    assert!(
        matches!(&call, HostCall::TakeQueued { session_id, entry_id, .. }
            if *session_id == session && entry_id == "b"),
        "{call:?}"
    );
    assert_eq!(
        chat.composer_text(),
        "",
        "nothing is restored before the host answers"
    );
    answer(&mut chat, &mut rx, reply, Ok(json!({"text": "bravo"}))).await;

    assert_eq!(chat.composer_text(), "bravo");
}

#[tokio::test]
async fn a_message_the_host_already_sent_is_not_restored() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    show_paused(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Enter);
    let (_, reply) = one_host_request(&mut rx);
    answer(&mut chat, &mut rx, reply, Ok(json!({"text": null}))).await;

    assert_eq!(chat.composer_text(), "");
    let history = drain_insert_history(&mut rx)
        .into_iter()
        .map(|lines| lines_to_single_string(&lines))
        .collect::<String>();
    assert!(history.contains("already started"), "{history}");
}

#[tokio::test]
async fn a_failed_take_restores_nothing_and_releases_the_message() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    show_paused(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Enter);
    let (_, reply) = one_host_request(&mut rx);
    answer(
        &mut chat,
        &mut rx,
        reply,
        Err(anyhow::anyhow!("host went away")),
    )
    .await;
    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Enter);

    assert_eq!(chat.composer_text(), "");
    let (call, _) = one_host_request(&mut rx);
    assert!(
        matches!(call, HostCall::TakeQueued { .. }),
        "the message can be picked again: {call:?}"
    );
}

/// The next host request the widget sends, waiting for it.
async fn next_host_request(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> (HostCall, HostReply) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Some(AppEvent::HostRequest { call, reply }) => return (call, reply),
                Some(_) => {}
                None => panic!("the widget stopped sending"),
            }
        }
    })
    .await
    .unwrap()
}

fn take_key(call: &HostCall) -> String {
    match call {
        HostCall::TakeQueued { request_key, .. } => request_key.clone(),
        other => panic!("not a take: {other:?}"),
    }
}

#[tokio::test]
async fn a_take_whose_reply_was_lost_is_asked_again_with_its_key_and_restored() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    show_paused(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Up);
    press(&mut chat, KeyCode::Enter);
    let (first, reply) = one_host_request(&mut rx);
    reply.deliver(Err(crate::app::ConnectionLost::for_tests().into()));
    let (again, reply) = next_host_request(&mut rx).await;
    answer(&mut chat, &mut rx, reply, Ok(json!({"text": "bravo"}))).await;

    assert_eq!(take_key(&again), take_key(&first));
    assert_eq!(chat.composer_text(), "bravo");
}

#[tokio::test]
async fn a_joined_host_without_queue_control_is_not_asked() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    chat.set_host_connection(crate::host_client::HostConnection {
        features: crate::host_client::HostFeatures {
            remote: true,
            ..crate::host_client::HostFeatures::default()
        },
        ..crate::host_client::HostConnection::owned()
    });
    show_paused(&mut chat);
    while rx.try_recv().is_ok() {}

    press(&mut chat, KeyCode::Enter);

    let mut calls = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::HostRequest { call, .. } = event {
            calls.push(format!("{call:?}"));
        }
    }
    assert_eq!(calls, Vec::<String>::new());
}
