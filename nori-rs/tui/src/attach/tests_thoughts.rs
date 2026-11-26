use nori_protocol::SessionEnvelope;
use pretty_assertions::assert_eq;

use super::state::AttachState;

fn update(seq: i64, update: nori_protocol::acp::v1::SessionUpdate) -> SessionEnvelope {
    use nori_protocol::acp::v1 as acp;
    SessionEnvelope {
        conversation_id: "one".into(),
        agent: "test-agent".into(),
        seq,
        event: nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
            acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
                "agent-session",
                update,
            )),
        )),
    }
}

fn chunk(text: &str, id: &str) -> nori_protocol::acp::v1::ContentChunk {
    use nori_protocol::acp::v1 as acp;
    acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text))).message_id(id)
}

/// Thoughts split across chunks and message ids, then the answer.
fn thinking_turn(state: &mut AttachState) {
    use nori_protocol::acp::v1::SessionUpdate;
    let events = [
        SessionUpdate::AgentThoughtChunk(chunk("**Reading the config", "t1")),
        SessionUpdate::AgentThoughtChunk(chunk(" loader**\n\nIt merges ", "t1")),
        SessionUpdate::AgentThoughtChunk(chunk("three sources.", "t2")),
        SessionUpdate::AgentThoughtChunk(chunk("\n\n**Choosing the edit**\n\nOne place.", "t3")),
        SessionUpdate::AgentMessageChunk(chunk("The default now lives in one place.", "a1")),
    ];
    for (seq, event) in (1..).zip(events) {
        state.apply(update(seq, event)).unwrap();
    }
    state.connected = true;
    state.selected = Some("one".into());
    state.list_open = false;
}

fn screen(state: &AttachState) -> String {
    let area = ratatui::layout::Rect::new(0, 0, 70, 16);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    super::render::render(
        state,
        &crate::ComposerInput::for_attachment(),
        area,
        &mut buffer,
    );
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn attached_thoughts_render_as_one_thought_block_before_the_answer() {
    let mut state = AttachState::default();
    thinking_turn(&mut state);

    let screen = screen(&state);
    let thought_lines = screen
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('│'))
        .collect::<Vec<_>>();
    assert_eq!(
        thought_lines,
        vec![
            "│ Reading the config loader",
            "│",
            "│ It merges three sources.",
            "│",
            "│ Choosing the edit",
            "│",
            "│ One place.",
        ],
        "{screen}"
    );
    assert!(
        screen.find("│ One place.") < screen.find("The default now lives in one place."),
        "{screen}"
    );
    insta::assert_snapshot!("attached_thought_block", screen);
}

#[test]
fn attach_opt_out_hides_thought_text() {
    let mut state = AttachState {
        thoughts_in_history: false,
        ..Default::default()
    };
    thinking_turn(&mut state);

    let screen = screen(&state);
    assert!(!screen.contains("merges three sources"), "{screen}");
    assert!(!screen.contains('│'), "{screen}");
    assert!(
        screen.contains("The default now lives in one place."),
        "{screen}"
    );
}
