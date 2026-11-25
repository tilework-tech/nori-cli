//! `/compact` on a session the host runs asks that host to compact that
//! session; the widget never compacts it in process.

use super::*;
use crate::host_client::HostCall;
use pretty_assertions::assert_eq;

/// Act as the App: hand the widget's own harness actions back to it, and
/// return the host calls that result. Nothing may reach an in-process
/// harness.
fn host_calls(
    chat: &mut ChatWidget,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Vec<HostCall> {
    let mut calls = Vec::new();
    let mut actions = Vec::new();
    let take = |event, actions: &mut Vec<_>, calls: &mut Vec<_>| match event {
        AppEvent::HostRequest { call, .. } => calls.push(call),
        AppEvent::HarnessAction(action) => actions.push(action),
        AppEvent::HarnessActionFailed(message) => panic!("{message}"),
        _ => {}
    };
    while let Ok(event) = rx.try_recv() {
        take(event, &mut actions, &mut calls);
    }
    for action in std::mem::take(&mut actions) {
        chat.submit_harness_action(action);
    }
    while let Ok(event) = rx.try_recv() {
        take(event, &mut actions, &mut calls);
    }
    assert!(actions.is_empty(), "sent {actions:?} in process");
    calls
}

#[tokio::test]
async fn compact_asks_the_host_to_compact_the_session_on_screen() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let id = nori_harness::ConversationId::new();
    chat.managed_conversation_id = Some(id);

    chat.dispatch_command(crate::slash_command::SlashCommand::Compact);

    let sessions: Vec<String> = host_calls(&mut chat, &mut rx)
        .into_iter()
        .map(|call| match call {
            HostCall::Compact { session_id } => session_id,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(sessions, vec![id.to_string()]);
}

#[tokio::test]
async fn compact_without_a_host_session_stays_in_process() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.dispatch_command(crate::slash_command::SlashCommand::Compact);

    let mut compacts = 0;
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HarnessAction(crate::app_event::HarnessAction::Compact) => compacts += 1,
            AppEvent::HostRequest { call, .. } => panic!("sent {call:?} to a host"),
            _ => {}
        }
    }
    assert_eq!(compacts, 1);
}
