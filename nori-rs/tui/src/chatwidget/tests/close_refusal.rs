//! A refused `/close` reaches the widget twice: once as the adapter's error
//! response on the session stream and once as the host's command error. The
//! person sees one refusal either way round.

use super::*;
use pretty_assertions::assert_eq;

const REFUSAL: &str = "harbor close declined";

fn adapter_refusal() -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
        request_id: nori_protocol::acp::v1::RequestId::Number(41),
        response: Err(nori_protocol::acp::v1::Error::new(-32004, REFUSAL)),
    })
}

/// The host's reply text for a refused close: its `{error:#}` rendering of
/// the adapter error under the connection's context.
fn host_refusal() -> anyhow::Error {
    let adapter = anyhow::Error::new(nori_protocol::acp::v1::Error::new(-32004, REFUSAL))
        .context("Failed to close ACP session");
    anyhow::anyhow!("{adapter:#}")
}

fn close_target(chat: &mut ChatWidget) {
    chat.managed_conversation_id = Some(nori_harness::ConversationId::new());
}

/// Submits `/close` and plays the App's routing of the refused reply into
/// `SessionCloseFailed` (without its generation check, which the PTY test
/// covers). Returns the text of every history cell the widget printed.
async fn refuse_close(chat: &mut ChatWidget, adapter_first: bool) -> Vec<String> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    chat.app_event_tx = AppEventSender::new(tx);
    chat.dispatch_command(crate::slash_command::SlashCommand::Close);
    let Some(AppEvent::HostRequest { reply, .. }) = rx.recv().await else {
        panic!("/close did not ask the host");
    };
    let generation = chat.session_generation;
    let mut shown = Vec::new();
    let collect = |event: AppEvent, shown: &mut Vec<String>| {
        if let AppEvent::InsertHistoryCell(cell) = event {
            shown.push(lines_to_single_string(&cell.display_lines(80)));
        }
    };
    if adapter_first {
        chat.handle_session_event(generation, adapter_refusal());
    }
    reply.deliver(Err(host_refusal()));
    loop {
        match rx.recv().await {
            Some(AppEvent::SessionCloseFailed { message, .. }) => {
                chat.on_session_close_failed(message);
                break;
            }
            Some(event) => collect(event, &mut shown),
            None => panic!("the close failure never arrived"),
        }
    }
    if !adapter_first {
        chat.handle_session_event(generation, adapter_refusal());
    }
    while let Ok(event) = rx.try_recv() {
        collect(event, &mut shown);
    }
    shown
}

fn refusals(shown: &[String]) -> usize {
    shown.iter().map(|text| text.matches(REFUSAL).count()).sum()
}

#[tokio::test]
async fn a_refused_close_shows_one_refusal_in_either_order() {
    for adapter_first in [true, false] {
        let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
        close_target(&mut chat);
        assert_eq!(
            refusals(&refuse_close(&mut chat, adapter_first).await),
            1,
            "adapter first: {adapter_first}"
        );
    }
}

#[tokio::test]
async fn an_error_held_during_a_close_shows_once_the_close_succeeds() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    close_target(&mut chat);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    chat.app_event_tx = AppEventSender::new(tx);
    chat.dispatch_command(crate::slash_command::SlashCommand::Close);
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
            request_id: nori_protocol::acp::v1::RequestId::Number(44),
            response: Err(nori_protocol::acp::v1::Error::new(
                -32603,
                "tide list failed",
            )),
        }),
    );
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
            request_id: nori_protocol::acp::v1::RequestId::Number(45),
            response: Ok(nori_protocol::acp::v1::AgentResponse::CloseSessionResponse(
                nori_protocol::acp::v1::CloseSessionResponse::new(),
            )),
        }),
    );
    let shown: Vec<String> = drain_insert_history(&mut rx)
        .iter()
        .map(|lines| lines_to_single_string(lines))
        .collect();
    assert_eq!(
        shown
            .iter()
            .map(|t| t.matches("tide list failed").count())
            .sum::<usize>(),
        1
    );
}

#[tokio::test]
async fn an_unrelated_error_after_a_refused_close_still_shows() {
    for adapter_first in [true, false] {
        let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
        close_target(&mut chat);
        let mut shown = refuse_close(&mut chat, adapter_first).await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        chat.app_event_tx = AppEventSender::new(tx);
        let generation = chat.session_generation;
        chat.handle_session_event(
            generation,
            nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
                request_id: nori_protocol::acp::v1::RequestId::Number(42),
                response: Err(nori_protocol::acp::v1::Error::new(
                    -32603,
                    "tide list failed",
                )),
            }),
        );
        shown.extend(
            drain_insert_history(&mut rx)
                .iter()
                .map(|lines| lines_to_single_string(lines)),
        );
        assert_eq!(refusals(&shown), 1);
        assert_eq!(
            shown
                .iter()
                .filter(|t| t.contains("tide list failed"))
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn a_second_refused_close_shows_its_own_refusal() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    close_target(&mut chat);
    let first = refuse_close(&mut chat, false).await;
    let second = refuse_close(&mut chat, true).await;
    assert_eq!((refusals(&first), refusals(&second)), (1, 1));
}

#[tokio::test]
async fn an_unrelated_error_during_a_refused_close_still_shows() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    close_target(&mut chat);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    chat.app_event_tx = AppEventSender::new(tx);
    chat.dispatch_command(crate::slash_command::SlashCommand::Close);
    let Some(AppEvent::HostRequest { reply, .. }) = rx.recv().await else {
        panic!("/close did not ask the host");
    };
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
            request_id: nori_protocol::acp::v1::RequestId::Number(43),
            response: Err(nori_protocol::acp::v1::Error::new(
                -32603,
                "tide list failed",
            )),
        }),
    );
    chat.handle_session_event(generation, adapter_refusal());
    reply.deliver(Err(host_refusal()));
    let mut shown = Vec::new();
    while let Some(event) = rx.recv().await {
        match event {
            AppEvent::SessionCloseFailed { message, .. } => {
                chat.on_session_close_failed(message);
                break;
            }
            AppEvent::InsertHistoryCell(cell) => {
                shown.push(lines_to_single_string(&cell.display_lines(80)));
            }
            _ => {}
        }
    }
    shown.extend(
        drain_insert_history(&mut rx)
            .iter()
            .map(|lines| lines_to_single_string(lines)),
    );
    assert_eq!(
        shown
            .iter()
            .map(|t| t.matches("tide list failed").count())
            .sum::<usize>(),
        1
    );
    assert_eq!(refusals(&shown), 1);
}
