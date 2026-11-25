//! Interrupt keys and typed commands on a host this terminal joined: only a
//! controller interrupts, and a cancel already on its way is never resent.

use super::*;
use crate::host_client::HostConnection;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use nori_protocol::ControlRole;
use nori_protocol::ExternalTurnState;
use pretty_assertions::assert_eq;

const DRAFT: &str = "synthetic draft about the lighthouse";

fn joined(role: ControlRole, connected: bool) -> HostConnection {
    HostConnection {
        role,
        connected,
        stopped: None,
        features: crate::host_client::HostFeatures::default(),
    }
}

fn turn(state: ExternalTurnState) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ExternalTurnChanged(
        nori_protocol::ExternalTurn {
            id: "synthetic-turn".to_owned(),
            state,
        },
    ))
}

/// A joined session on screen with a turn running that another terminal
/// started.
fn on_a_running_turn(role: ControlRole, connected: bool) -> (ChatWidget, Receiver) {
    let (mut chat, rx, _op_rx) = make_chatwidget_manual();
    chat.managed_conversation_id = Some(nori_harness::ConversationId::new());
    chat.set_host_connection(joined(role, connected));
    let generation = chat.session_generation;
    chat.handle_session_event(generation, turn(ExternalTurnState::Working));
    (chat, rx)
}

type Receiver = tokio::sync::mpsc::UnboundedReceiver<AppEvent>;

fn press(chat: &mut ChatWidget, code: KeyCode, modifiers: KeyModifiers) {
    chat.handle_key_event(KeyEvent::new(code, modifiers));
}

fn ctrl_c(chat: &mut ChatWidget) {
    press(chat, KeyCode::Char('c'), KeyModifiers::CONTROL);
}

/// What the terminal printed, and how many interrupts it sent or failed.
fn outcome(rx: &mut Receiver) -> (String, usize) {
    let mut text = String::new();
    let mut requests = 0;
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::InsertHistoryCell(cell) => {
                text.push_str(&lines_to_single_string(&cell.display_lines(80)));
            }
            AppEvent::HostRequest { .. } => requests += 1,
            // An interrupt aimed at a harness this terminal does not run
            // fails instead of reaching the host.
            AppEvent::HarnessAction(crate::app_event::HarnessAction::Cancel)
            | AppEvent::HarnessActionFailed(_) => requests += 1,
            _ => {}
        }
    }
    (text, requests)
}

#[test]
fn a_watchers_interrupt_keys_never_reach_the_host() {
    let (mut chat, mut rx) = on_a_running_turn(ControlRole::Watcher, true);
    let _ = outcome(&mut rx);
    chat.set_composer_text(DRAFT.to_owned());

    ctrl_c(&mut chat);
    press(&mut chat, KeyCode::Esc, KeyModifiers::NONE);
    ctrl_c(&mut chat);

    let (text, requests) = outcome(&mut rx);
    assert_eq!((requests, chat.composer_text()), (0, DRAFT.to_owned()));
    assert!(text.contains("Alt+T"), "{text}");
    assert!(!text.contains("Left the host"), "{text}");
}

#[test]
fn an_empty_watcher_is_told_why_and_stays_joined() {
    let (mut chat, mut rx) = on_a_running_turn(ControlRole::Watcher, true);
    let _ = outcome(&mut rx);

    ctrl_c(&mut chat);

    let (text, requests) = outcome(&mut rx);
    assert_eq!(requests, 0);
    assert!(text.contains("Alt+T"), "{text}");
    assert!(!text.contains("Left the host"), "{text}");
}

#[test]
fn a_reconnecting_controller_does_not_send_an_interrupt() {
    let (mut chat, mut rx) = on_a_running_turn(ControlRole::Controller, false);
    let _ = outcome(&mut rx);
    chat.set_composer_text(DRAFT.to_owned());

    ctrl_c(&mut chat);

    let (text, requests) = outcome(&mut rx);
    assert_eq!((requests, chat.composer_text()), (0, DRAFT.to_owned()));
    assert!(text.contains("Reconnecting"), "{text}");
}

#[test]
fn a_controllers_second_interrupt_keeps_the_pending_cancel_and_the_draft() {
    let (mut chat, mut rx) = on_a_running_turn(ControlRole::Controller, true);
    let _ = outcome(&mut rx);
    chat.set_composer_text(DRAFT.to_owned());

    ctrl_c(&mut chat);
    assert_eq!(outcome(&mut rx).1, 1);
    let generation = chat.session_generation;
    chat.handle_session_event(generation, turn(ExternalTurnState::CancelRequested));
    ctrl_c(&mut chat);

    let (text, requests) = outcome(&mut rx);
    assert_eq!((requests, chat.composer_text()), (0, DRAFT.to_owned()));
    assert!(!text.contains("Left the host"), "{text}");
}

#[test]
fn ctrl_c_on_an_empty_composer_leaves_the_host_while_a_cancel_is_unconfirmed() {
    let (mut chat, mut rx) = on_a_running_turn(ControlRole::Controller, true);
    let _ = outcome(&mut rx);
    ctrl_c(&mut chat);
    assert_eq!(outcome(&mut rx).1, 1);
    let generation = chat.session_generation;
    chat.handle_session_event(generation, turn(ExternalTurnState::Unconfirmed));
    let _ = outcome(&mut rx);

    ctrl_c(&mut chat);

    let (text, requests) = outcome(&mut rx);
    assert_eq!(requests, 0);
    assert!(text.contains("Left the host"), "{text}");
}

#[test]
fn an_unknown_approval_mode_keeps_the_typed_command() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.managed_conversation_id = Some(nori_harness::ConversationId::new());
    chat.set_host_connection(joined(ControlRole::Controller, true));

    chat.set_composer_text("/approvals sometimes".to_owned());
    press(&mut chat, KeyCode::Enter, KeyModifiers::NONE);

    let (text, requests) = outcome(&mut rx);
    assert_eq!(
        (requests, chat.composer_text()),
        (0, "/approvals sometimes".to_owned())
    );
    assert!(text.contains("unknown mode sometimes"), "{text}");
}

#[test]
fn an_idle_watchers_esc_prints_nothing() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.managed_conversation_id = Some(nori_harness::ConversationId::new());
    chat.set_host_connection(joined(ControlRole::Watcher, true));
    let _ = outcome(&mut rx);

    press(&mut chat, KeyCode::Esc, KeyModifiers::NONE);

    assert_eq!(outcome(&mut rx), (String::new(), 0));
}

#[test]
fn ctrl_c_on_an_empty_composer_leaves_a_stopped_host() {
    let (mut chat, mut rx) = on_a_running_turn(ControlRole::Controller, true);
    chat.set_host_connection(HostConnection {
        stopped: Some("The host stopped.".to_owned()),
        ..joined(ControlRole::Controller, false)
    });
    let _ = outcome(&mut rx);

    ctrl_c(&mut chat);

    let (text, requests) = outcome(&mut rx);
    assert_eq!(requests, 0);
    assert!(text.contains("Left the host"), "{text}");
}
