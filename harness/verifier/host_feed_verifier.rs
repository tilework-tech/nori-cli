//! Verifier-only: the exact envelopes a real `nori host` replays for one
//! finished turn, fed to a joined widget. Injected as
//! `chatwidget::tests::host_feed_verifier`; never shipped.

use super::*;
use nori_harness::ConversationId;

const REPLAY: &str = include_str!("host-replay-first-light.jsonl");

fn widget() -> (ChatWidget, tokio::sync::mpsc::UnboundedReceiver<AppEvent>) {
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
        ConversationId::new(),
        None,
    );
    widget.app_event_tx = app_event_tx;
    (widget, event_rx)
}

fn printed(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    let mut text = String::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            text.push_str(&lines_to_single_string(&cell.display_lines(80)));
        }
    }
    text
}

fn events(newline: bool) -> Vec<nori_protocol::SessionEvent> {
    REPLAY
        .lines()
        .map(|line| {
            let mut line = line.to_string();
            if newline {
                line = line.replace("heard: first light\"", "heard: first light\\n\"");
            }
            let envelope: nori_protocol::SessionEnvelope = serde_json::from_str(&line).unwrap();
            envelope.event
        })
        .collect()
}

fn run(newline: bool) -> String {
    let (mut chat, mut rx) = widget();
    let generation = chat.session_generation();
    for event in events(newline) {
        chat.handle_session_event(generation, event);
    }
    for _ in 0..50 {
        chat.on_commit_tick();
    }
    let text = printed(&mut rx);
    text
}

#[test]
fn a_replayed_finished_turn_shows_the_agent_reply() {
    let text = run(false);
    assert!(text.contains("first light"), "user prompt missing: {text}");
    assert!(text.contains("claude-code heard: first light"), "agent reply missing: {text}");
}

#[test]
fn control_the_same_turn_with_a_newline_terminated_reply() {
    let text = run(true);
    assert!(text.contains("claude-code heard: first light"), "agent reply missing: {text}");
}

#[test]
fn the_replayed_prompt_response_deserializes_as_a_prompt_response() {
    let line = REPLAY.lines().find(|l| l.contains("\"stopReason\"")).expect("prompt response in replay");
    let envelope: nori_protocol::SessionEnvelope = serde_json::from_str(line).unwrap();
    let shown = format!("{:?}", envelope.event);
    assert!(shown.contains("PromptResponse"), "deserialized as: {}", &shown[..shown.len().min(400)]);
}
