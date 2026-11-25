//! A session shown from a joined host's replay and live events, with no
//! harness in this process: its history prints from the feed and its
//! prompts go to the host.

use super::*;
use crate::host_client::HostCall;
use crate::host_client::HostConnection;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use nori_harness::ConversationId;
use nori_protocol::ControlRole;
use pretty_assertions::assert_eq;

const SESSION: &str = "00000000-0000-4000-8000-00000000feed";

fn session_id() -> ConversationId {
    ConversationId::from_string(SESSION).expect("synthetic session id")
}

fn joined(role: ControlRole, connected: bool) -> HostConnection {
    HostConnection {
        role,
        connected,
        stopped: None,
    }
}

fn host_feed_widget() -> (ChatWidget, tokio::sync::mpsc::UnboundedReceiver<AppEvent>) {
    let (event_tx, event_rx) = unbounded_channel();
    let app_event_tx = AppEventSender::new(event_tx);
    let mut widget = ChatWidget::from_host_feed(
        ChatWidgetInit {
            config: test_config(),
            frame_requester: FrameRequester::test_dummy(),
            app_event_tx: app_event_tx.clone(),
            initial_prompt: None,
            initial_images: Vec::new(),
            enhanced_keys_supported: false,
            auth_manager: AuthManager::from_auth_for_testing(CodexAuth::from_api_key("test")),
            vertical_footer: false,
            footer_segment_config: nori_config::FooterSegmentConfig::default(),
            footer_layout_config: nori_config::FooterLayoutConfig::default(),
            deferred_spawn: false,
            fork_context: None,
            prepared_agent: None,
            cloud_mode: false,
            analytics: None,
            session_manager: None,
            clock: crate::answer_guard::Clock::system(),
        },
        session_id(),
        Some("Synthetic joined session".to_string()),
    );
    widget.app_event_tx = app_event_tx;
    (widget, event_rx)
}

fn notice(message: &str) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::Notice(nori_protocol::Notice {
        message: message.to_string(),
    }))
}

fn answer(text: &str) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
        nori_protocol::acp::v1::AgentNotification::SessionNotification(
            nori_protocol::acp::v1::SessionNotification::new(
                SESSION,
                nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(
                    nori_protocol::acp::v1::ContentChunk::new(
                        nori_protocol::acp::v1::ContentBlock::Text(
                            nori_protocol::acp::v1::TextContent::new(text),
                        ),
                    ),
                ),
            ),
        ),
    ))
}

fn prompting(request_id: &str) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
        nori_protocol::SessionPhase::Prompting {
            request_id: nori_protocol::acp::v1::RequestId::Str(request_id.to_string()),
        },
    ))
}

fn feed(chat: &mut ChatWidget, events: impl IntoIterator<Item = nori_protocol::SessionEvent>) {
    let generation = chat.session_generation();
    for event in events {
        chat.handle_session_event(generation, event);
    }
}

fn printed(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> (String, Vec<AppEvent>) {
    let mut text = String::new();
    let mut other = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::InsertHistoryCell(cell) => {
                text.push_str(&lines_to_single_string(&cell.display_lines(80)));
            }
            event => other.push(event),
        }
    }
    (text, other)
}

#[test]
fn replayed_history_prints_in_order() {
    let (mut chat, mut rx) = host_feed_widget();

    feed(
        &mut chat,
        [
            notice("synthetic first notice"),
            answer("synthetic agent reply\n"),
            prompting("synthetic-request"),
            notice("synthetic second notice"),
        ],
    );

    let (text, _) = printed(&mut rx);
    let first = text.find("synthetic first notice");
    let reply = text.find("synthetic agent reply");
    let second = text.find("synthetic second notice");
    assert!(
        first.is_some() && reply.is_some() && second.is_some(),
        "{text}"
    );
    assert!(first < reply && reply < second, "{text}");
}

#[tokio::test]
async fn a_joined_controller_sends_its_prompt_to_the_host_session() {
    let (mut chat, mut rx) = host_feed_widget();
    chat.set_host_connection(joined(ControlRole::Controller, true));

    type_text(&mut chat, "synthetic joined prompt");
    chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(chat.composer_text(), "");
    let (text, events) = printed(&mut rx);
    let prompt = events.iter().find_map(|event| match event {
        AppEvent::HostRequest {
            call: HostCall::Prompt(params),
            ..
        } => Some(params.clone()),
        _ => None,
    });
    let Some(prompt) = prompt else {
        panic!("no prompt reached the host; printed {text:?}, sent {events:?}");
    };
    assert_eq!(prompt["session_id"], SESSION);
    assert!(
        !events.iter().any(|event| matches!(
            event,
            AppEvent::NewSession | AppEvent::HarnessActionFailed(_)
        )),
        "{events:?}"
    );
    assert!(!text.contains("No active"), "{text}");
}

#[test]
fn a_draft_survives_losing_and_regaining_the_host() {
    let (mut chat, _rx) = host_feed_widget();
    chat.set_host_connection(joined(ControlRole::Controller, true));
    type_text(&mut chat, "synthetic draft across an outage");

    chat.set_host_connection(joined(ControlRole::Controller, false));
    chat.set_host_connection(joined(ControlRole::Controller, true));

    assert_eq!(chat.composer_text(), "synthetic draft across an outage");
}

#[test]
fn a_long_replay_applies_quickly() {
    let (mut chat, mut rx) = host_feed_widget();
    let events: Vec<_> = (0..4_000)
        .map(|index| {
            if index % 2 == 0 {
                notice(&format!("synthetic notice {index}"))
            } else {
                answer(&format!("synthetic reply {index}\n"))
            }
        })
        .collect();

    let started = std::time::Instant::now();
    feed(&mut chat, events);
    let (text, _) = printed(&mut rx);
    let elapsed = started.elapsed();

    assert!(
        text.contains("synthetic notice 3998"),
        "the replay must finish"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "replaying 4,000 events took {elapsed:?}"
    );
}

fn prompt_done(request_id: &str) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
        request_id: nori_protocol::acp::v1::RequestId::Str(request_id.to_string()),
        response: Ok(nori_protocol::acp::v1::AgentResponse::PromptResponse(
            nori_protocol::acp::v1::PromptResponse::new(
                nori_protocol::acp::v1::StopReason::EndTurn,
            ),
        )),
    })
}

fn idle() -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
        nori_protocol::SessionPhase::Idle,
    ))
}

/// Events as a joined terminal receives them: decoded from the host's JSON.
fn feed_wire(chat: &mut ChatWidget, events: impl IntoIterator<Item = nori_protocol::SessionEvent>) {
    feed(
        chat,
        events
            .into_iter()
            .map(|event| serde_json::from_value(serde_json::to_value(&event).unwrap()).unwrap()),
    );
}

#[test]
fn an_answer_without_a_final_newline_prints_when_its_turn_ends() {
    let (mut chat, mut rx) = host_feed_widget();

    feed_wire(
        &mut chat,
        [
            prompting("synthetic-request"),
            answer("synthetic host answer"),
            prompt_done("synthetic-request"),
            idle(),
        ],
    );

    let (text, _) = printed(&mut rx);
    assert!(text.contains("synthetic host answer"), "{text}");
}
