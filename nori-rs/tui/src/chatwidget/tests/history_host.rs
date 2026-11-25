//! A session on the host records, recalls and searches prompt history
//! through the host, so every terminal on that host shares one history.

use super::*;
use crate::app_event_sender::HostReply;
use crate::host_client::HostCall;
use pretty_assertions::assert_eq;
use serde_json::json;

fn host_session(chat: &mut ChatWidget) -> String {
    let id = nori_harness::ConversationId::new();
    chat.managed_conversation_id = Some(id);
    id.to_string()
}

fn press(chat: &mut ChatWidget, code: KeyCode) {
    chat.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
}

/// Act as the App: hand the widget's own harness actions back to it, and
/// return the host requests that result. Nothing may reach an in-process
/// harness.
fn host_requests(
    chat: &mut ChatWidget,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Vec<(HostCall, HostReply)> {
    let mut requests = Vec::new();
    let mut actions = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HostRequest { call, reply } => requests.push((call, reply)),
            AppEvent::HarnessAction(action) => actions.push(action),
            _ => {}
        }
    }
    for action in actions {
        chat.submit_harness_action(action);
    }
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HostRequest { call, reply } => requests.push((call, reply)),
            AppEvent::HarnessActionFailed(message) => panic!("{message}"),
            _ => {}
        }
    }
    requests
}

/// Answer `reply` with `result` and hand the widget the event the App
/// would route back to it.
async fn answer(
    chat: &mut ChatWidget,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    reply: HostReply,
    result: serde_json::Value,
) {
    reply.deliver(Ok(result));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Some(AppEvent::HistoryEntryLoaded {
                    log_id,
                    offset,
                    entry,
                }) => return chat.on_history_entry_loaded(log_id, offset, entry),
                Some(AppEvent::HistorySearchLoaded(entries)) => {
                    return chat.on_history_search_loaded(entries);
                }
                Some(AppEvent::HistoryMetadataLoaded {
                    log_id,
                    entry_count,
                }) => return chat.on_history_metadata_loaded(log_id, entry_count),
                Some(_) => {}
                None => panic!("the widget stopped sending"),
            }
        }
    })
    .await
    .unwrap();
}

fn complete_turn(chat: &mut ChatWidget) {
    chat.handle_client_event(crate::presentation::ClientEvent::PromptCompleted(
        crate::presentation::PromptCompleted {
            stop_reason: nori_protocol::acp::v1::StopReason::EndTurn,
            last_agent_message: None,
            failure: None,
        },
    ));
}

/// The host's history, as the widget learns it when a turn ends.
async fn learn_history(
    chat: &mut ChatWidget,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
    entry_count: i64,
) {
    complete_turn(chat);
    let mut requests = host_requests(chat, rx);
    let position = requests
        .iter()
        .position(|(call, _)| matches!(call, HostCall::HistoryMetadata))
        .expect("the widget asks the host for its history");
    let (_, reply) = requests.remove(position);
    answer(
        chat,
        rx,
        reply,
        json!({"logId": 7, "entryCount": entry_count}),
    )
    .await;
}

#[tokio::test]
async fn a_sent_prompt_is_recorded_in_the_hosts_history() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let session = host_session(&mut chat);
    while rx.try_recv().is_ok() {}

    chat.submit_harness_action(crate::app_event::HarnessAction::AddHistory(
        "prune the roses".to_string(),
    ));

    let recorded = host_requests(&mut chat, &mut rx)
        .into_iter()
        .filter_map(|(call, _)| match call {
            HostCall::AddHistory { session_id, text } => Some((session_id, text)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(recorded, vec![(session, "prune the roses".to_string())]);
}

#[tokio::test]
async fn up_recalls_a_prompt_another_terminal_sent_to_the_host() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    while rx.try_recv().is_ok() {}
    learn_history(&mut chat, &mut rx, 3).await;

    press(&mut chat, KeyCode::Up);

    let mut requests = host_requests(&mut chat, &mut rx);
    assert_eq!(requests.len(), 1);
    let (call, reply) = requests.remove(0);
    assert!(
        matches!(
            call,
            HostCall::HistoryEntry {
                log_id: 7,
                offset: 2
            }
        ),
        "{call:?}"
    );
    answer(
        &mut chat,
        &mut rx,
        reply,
        json!({"entry": {"sessionId": "other", "ts": 1, "text": "water the basil"}}),
    )
    .await;
    assert_eq!(chat.composer_text(), "water the basil");
}

#[tokio::test]
async fn a_turn_that_adds_nothing_keeps_this_terminals_own_prompts() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    while rx.try_recv().is_ok() {}
    learn_history(&mut chat, &mut rx, 0).await;

    chat.bottom_pane
        .record_local_history_submission("dig a pond");
    learn_history(&mut chat, &mut rx, 0).await;
    press(&mut chat, KeyCode::Up);

    assert_eq!(chat.composer_text(), "dig a pond");
}

#[tokio::test]
async fn ctrl_r_searches_the_hosts_history() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    while rx.try_recv().is_ok() {}

    chat.handle_key_event(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));

    let mut requests = host_requests(&mut chat, &mut rx);
    assert_eq!(requests.len(), 1);
    let (call, reply) = requests.remove(0);
    assert!(
        matches!(call, HostCall::SearchHistory { max_results: 500 }),
        "{call:?}"
    );
    answer(
        &mut chat,
        &mut rx,
        reply,
        json!({"entries": [{"sessionId": "other", "ts": 1, "text": "harvest the squash"}]}),
    )
    .await;
    press(&mut chat, KeyCode::Enter);
    assert_eq!(chat.composer_text(), "harvest the squash");
}
