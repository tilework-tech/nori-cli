//! The hint row under the composer names the keys that work right now.

use super::*;
use crate::host_client::HostConnection;
use crate::host_client::HostFeatures;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use nori_protocol::ControlRole;
use nori_protocol::QueuePause;

const DISCOVERY: &str = "/ commands · ! shell · @ files · ? help";

fn screen(chat: &ChatWidget) -> String {
    use crate::render::renderable::Renderable;
    let width = 100;
    let area = ratatui::layout::Rect::new(0, 0, width, chat.desired_height(width));
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    chat.render(area, &mut buffer);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn prompting(id: i64) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
        nori_protocol::SessionPhase::Prompting {
            request_id: nori_protocol::acp::v1::RequestId::Number(id),
        },
    ))
}

fn initialize(steering: bool) -> nori_protocol::SessionEvent {
    let meta = serde_json::json!({ "steering": { "supported": steering } })
        .as_object()
        .cloned()
        .expect("metadata is an object");
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
        request_id: nori_protocol::acp::v1::RequestId::Str("initialize".to_string()),
        response: Ok(nori_protocol::acp::v1::AgentResponse::InitializeResponse(
            nori_protocol::acp::v1::InitializeResponse::new(
                nori_protocol::acp::ProtocolVersion::V1,
            )
            .agent_info(nori_protocol::acp::v1::Implementation::new(
                "@agentclientprotocol/claude-agent-acp",
                "0.109.0",
            ))
            .meta(meta),
        )),
    })
}

fn paused_queue() -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::QueueChanged(
        nori_protocol::QueueSnapshot {
            prompts: vec!["synthetic queued prompt".to_string()],
            entries: vec![nori_protocol::QueueEntry {
                id: "q1".to_string(),
                text: "synthetic queued prompt".to_string(),
                editable: true,
            }],
            paused: Some(QueuePause::Interrupted),
        },
    ))
}

fn send(chat: &mut ChatWidget, event: nori_protocol::SessionEvent) {
    let generation = chat.session_generation;
    chat.handle_session_event(generation, event);
}

#[test]
fn hints_switch_from_discovery_to_editing_as_soon_as_text_is_typed() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    assert!(screen(&chat).contains(DISCOVERY), "{}", screen(&chat));

    type_text(&mut chat, "h");
    let typed = screen(&chat);
    assert!(typed.contains("Enter send · Ctrl+J newline"), "{typed}");
    assert!(!typed.contains(DISCOVERY), "{typed}");

    chat.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert!(screen(&chat).contains(DISCOVERY));
}

fn prompt_line(chat: &ChatWidget) -> String {
    screen(chat)
        .lines()
        .find(|line| line.trim_start().starts_with('›'))
        .expect("the prompt is drawn")
        .trim()
        .to_string()
}

#[test]
fn the_placeholder_steps_aside_while_hints_are_shown() {
    let (chat, _rx, _op_rx) = make_chatwidget_manual();
    assert_eq!(prompt_line(&chat), "›");

    let mut segments = nori_config::FooterSegmentConfig::default();
    segments.set_enabled(nori_config::FooterSegment::ComposerHints, false);
    let (chat, _rx, _op_rx) = make_chatwidget_manual_with_footer_config(
        segments,
        nori_config::FooterLayoutConfig::default(),
    );
    let prompt = prompt_line(&chat);
    assert!(
        ["shortcuts", "slash command", "skill", "shell", "file"]
            .iter()
            .any(|tip| prompt.contains(tip)),
        "{prompt}"
    );
}

#[test]
fn a_shell_draft_is_run_with_enter() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    type_text(&mut chat, "!");
    let shell_mode = screen(&chat);
    assert!(!shell_mode.contains("/ commands"), "{shell_mode}");
    assert!(!shell_mode.contains("Enter run"), "{shell_mode}");
    type_text(&mut chat, "ls");
    let rendered = screen(&chat);
    assert!(rendered.contains("Enter run"), "{rendered}");
}

#[test]
fn a_reconnecting_controller_is_not_offered_send_keys() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(HostConnection {
        role: ControlRole::Controller,
        connected: false,
        stopped: None,
        features: HostFeatures::default(),
    });
    type_text(&mut chat, "synthetic draft");
    let rendered = screen(&chat);
    assert!(!rendered.contains("Enter send"), "{rendered}");
    assert!(rendered.contains("Ctrl+J newline"), "{rendered}");
}

#[test]
fn during_its_own_turn_a_draft_can_steer_or_be_queued() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    send(&mut chat, initialize(true));
    send(&mut chat, prompting(7));
    type_text(&mut chat, "also check the tests");
    let rendered = screen(&chat);
    assert!(rendered.contains("Enter steer · Tab queue"), "{rendered}");

    chat.set_composer_text("/model".to_string());
    chat.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    let rendered = screen(&chat);
    assert!(rendered.contains("Enter run"), "{rendered}");
    assert!(!rendered.contains("Tab queue"), "{rendered}");
}

#[test]
fn without_steering_enter_is_advertised_as_queueing() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    send(&mut chat, initialize(false));
    send(&mut chat, prompting(7));
    type_text(&mut chat, "also check the tests");
    let rendered = screen(&chat);
    assert!(rendered.contains("Enter queue"), "{rendered}");
    assert!(!rendered.contains("Enter steer"), "{rendered}");
}

#[test]
fn a_paused_queue_offers_resume_first() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    send(&mut chat, paused_queue());
    let rendered = screen(&chat);
    assert!(
        rendered.contains(&format!("Enter resume queue · {DISCOVERY}")),
        "{rendered}"
    );
}

#[test]
fn a_watcher_is_never_offered_send_keys() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(HostConnection {
        role: ControlRole::Watcher,
        connected: true,
        stopped: None,
        features: HostFeatures::default(),
    });
    send(&mut chat, paused_queue());
    let empty = screen(&chat);
    assert!(!empty.contains("Enter"), "{empty}");
    assert!(!empty.contains("! shell"), "{empty}");
    assert!(empty.contains("/ commands · @ files"), "{empty}");

    type_text(&mut chat, "synthetic watcher draft");
    let typed = screen(&chat);
    assert!(!typed.contains("Enter send"), "{typed}");
    assert!(typed.contains("Ctrl+J newline"), "{typed}");
}

#[test]
fn a_remote_host_without_shell_or_path_search_offers_neither() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(HostConnection {
        role: ControlRole::Controller,
        connected: true,
        stopped: None,
        features: HostFeatures {
            remote: true,
            ..HostFeatures::default()
        },
    });
    let rendered = screen(&chat);
    assert!(rendered.contains("/ commands · ? help"), "{rendered}");
    assert!(!rendered.contains("! shell"), "{rendered}");
    assert!(!rendered.contains("@ files"), "{rendered}");
}

#[test]
fn turning_hints_off_restores_the_plain_footer() {
    let mut segments = nori_config::FooterSegmentConfig::default();
    segments.set_enabled(nori_config::FooterSegment::ComposerHints, false);
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual_with_footer_config(
        segments,
        nori_config::FooterLayoutConfig::default(),
    );
    assert!(!screen(&chat).contains("/ commands"), "{}", screen(&chat));
    type_text(&mut chat, "h");
    assert!(!screen(&chat).contains("Enter send"), "{}", screen(&chat));
}
