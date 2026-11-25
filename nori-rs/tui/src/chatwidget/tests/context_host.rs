//! A session on the host reads its context report from the host, so
//! `/context` and the footer show that session's usage and the instruction
//! files on the host's own filesystem, whichever terminal asks.

use super::*;
use crate::app_event_sender::HostReply;
use crate::host_client::HostCall;
use crate::presentation::session_runtime::SessionUsageState;
use pretty_assertions::assert_eq;
use serde_json::json;

fn host_session(chat: &mut ChatWidget) -> String {
    let id = nori_harness::ConversationId::new();
    chat.managed_conversation_id = Some(id);
    id.to_string()
}

fn session_started(chat: &mut ChatWidget) {
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionStarted(
            nori_protocol::SessionStarted {
                transcript_id: None,
                acp_session_id: nori_protocol::acp::v1::SessionId::new("0"),
                cwd: std::path::PathBuf::from("/workspace"),
                transcript_path: None,
                history_log_id: 0,
                history_entry_count: 0,
                config_options: Vec::new(),
            },
        )),
    );
}

/// The context requests the widget sent, with their replies. Nothing may
/// read context in process.
fn context_requests(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> Vec<(HostCall, HostReply)> {
    let mut requests = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::HostRequest { call, reply }
                if matches!(call, HostCall::SessionContext { .. }) =>
            {
                requests.push((call, reply));
            }
            AppEvent::ContextTimelineReady { .. } => panic!("context was read in process"),
            _ => {}
        }
    }
    requests
}

fn report(used: i64, size: i64, instructions: serde_json::Value) -> serde_json::Value {
    json!({
        "timeline": {"turns": [], "current": null, "segments": [],
            "incompleteHistory": false, "epoch": 0},
        "instructions": instructions,
        "usage": {"used": used, "size": size},
    })
}

/// Answer `reply` and hand the widget what the App routes back to the
/// widget on screen.
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
                Some(AppEvent::HostContextLoaded {
                    session_id,
                    result,
                    asked,
                }) => return chat.on_host_context_loaded(&session_id, result, asked),
                Some(_) => {}
                None => panic!("the widget stopped sending"),
            }
        }
    })
    .await
    .unwrap();
}

fn printed(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    drain_insert_history(rx)
        .into_iter()
        .map(|lines| lines_to_single_string(&lines))
        .collect()
}

#[tokio::test]
async fn context_command_shows_the_hosts_report_for_the_session_on_screen() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let session = host_session(&mut chat);
    while rx.try_recv().is_ok() {}

    chat.dispatch_command(SlashCommand::Context);
    let mut requests = context_requests(&mut rx);
    assert_eq!(requests.len(), 1);
    let (call, reply) = requests.pop().unwrap();
    assert!(
        matches!(
            &call,
            HostCall::SessionContext { session_id, skip_instructions: false }
                if *session_id == session
        ),
        "{call:?}"
    );
    answer(
        &mut chat,
        &mut rx,
        reply,
        report(
            61_000,
            200_000,
            json!([{"path": "/srv/orchard/AGENTS.md", "tokens": 412}]),
        ),
    )
    .await;

    let shown = printed(&mut rx);
    assert!(shown.contains("61,000 / 200,000 tokens"), "{shown}");
    assert!(shown.contains("/srv/orchard/AGENTS.md"), "{shown}");
}

#[tokio::test]
async fn a_session_coming_on_screen_takes_its_usage_from_the_host() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let session = host_session(&mut chat);

    session_started(&mut chat);
    let mut requests = context_requests(&mut rx);
    assert_eq!(requests.len(), 1);
    let (call, reply) = requests.pop().unwrap();
    assert!(
        matches!(
            &call,
            HostCall::SessionContext { session_id, skip_instructions: true }
                if *session_id == session
        ),
        "{call:?}"
    );
    answer(
        &mut chat,
        &mut rx,
        reply,
        report(42_000, 200_000, json!([])),
    )
    .await;

    let (usage, _) = chat.bottom_pane.context_usage_snapshot();
    assert_eq!(
        (usage.tokens, usage.window_tokens),
        (Some(42_000), Some(200_000))
    );
    assert_eq!(printed(&mut rx), "", "seeding the footer prints nothing");
}

#[tokio::test]
async fn a_live_reading_is_not_replaced_by_an_older_host_reply() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    session_started(&mut chat);
    let (_, reply) = context_requests(&mut rx).pop().unwrap();

    chat.bottom_pane.set_session_usage(Some(SessionUsageState {
        used_tokens: 90_000,
        total_tokens: 200_000,
        cost_display: None,
    }));
    answer(
        &mut chat,
        &mut rx,
        reply,
        report(42_000, 200_000, json!([])),
    )
    .await;

    assert_eq!(
        chat.bottom_pane.context_usage_snapshot().0.tokens,
        Some(90_000)
    );
}

#[tokio::test]
async fn a_report_for_another_session_is_dropped() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    host_session(&mut chat);
    let other = nori_harness::ConversationId::new().to_string();
    let report: nori_protocol::ContextReport =
        serde_json::from_value(report(7_000, 100_000, json!([]))).unwrap();

    chat.on_host_context_loaded(&other, Ok(report), true);

    assert_eq!(printed(&mut rx), "");
    assert_eq!(chat.bottom_pane.context_usage_snapshot().0.tokens, None);
}
