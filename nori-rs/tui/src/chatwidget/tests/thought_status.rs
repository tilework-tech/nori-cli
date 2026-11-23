use super::*;
use pretty_assertions::assert_eq;

fn notification(update: nori_protocol::acp::v1::SessionUpdate) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
        nori_protocol::acp::v1::AgentNotification::SessionNotification(
            nori_protocol::acp::v1::SessionNotification::new("session", update),
        ),
    ))
}

fn text_chunk(text: &str) -> nori_protocol::acp::v1::ContentChunk {
    nori_protocol::acp::v1::ContentChunk::new(nori_protocol::acp::v1::ContentBlock::Text(
        nori_protocol::acp::v1::TextContent::new(text),
    ))
}

fn thought(text: &str) -> nori_protocol::SessionEvent {
    notification(nori_protocol::acp::v1::SessionUpdate::AgentThoughtChunk(
        text_chunk(text),
    ))
}

fn answer(text: &str) -> nori_protocol::SessionEvent {
    notification(nori_protocol::acp::v1::SessionUpdate::AgentMessageChunk(
        text_chunk(text),
    ))
}

fn tool_call(id: &str, title: &str) -> nori_protocol::SessionEvent {
    notification(nori_protocol::acp::v1::SessionUpdate::ToolCall(
        nori_protocol::acp::v1::ToolCall::new(id.to_string(), title.to_string()),
    ))
}

fn start_prompt(chat: &mut ChatWidget) {
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
            nori_protocol::SessionPhase::Prompting {
                request_id: nori_protocol::acp::v1::RequestId::Str("local-prompt".to_string()),
            },
        )),
    );
}

fn send(chat: &mut ChatWidget, events: Vec<nori_protocol::SessionEvent>) {
    let generation = chat.session_generation;
    for event in events {
        chat.handle_session_event(generation, event);
    }
}

/// The working indicator's first row, which carries the status header.
fn working_indicator(chat: &mut ChatWidget) -> String {
    use crate::render::renderable::Renderable;

    let width = 100;
    let area = ratatui::layout::Rect::new(0, 0, width, chat.bottom_pane.desired_height(width));
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    chat.bottom_pane.render(area, &mut buffer);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buffer[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect::<String>()
                .trim()
                .to_string()
        })
        .find(|row| row.contains("esc to interrupt"))
        .expect("working indicator row")
}

fn header_of(row: &str) -> String {
    let after_spinner = row.trim_start_matches(|c: char| !c.is_alphanumeric());
    after_spinner
        .split(" (")
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[test]
fn thought_without_a_bold_header_shows_its_first_line_in_the_working_indicator() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);

    send(
        &mut chat,
        vec![
            thought("I should read the config loader first"),
            thought(" before changing the default.\nThen I'll update the tests."),
        ],
    );

    assert_eq!(
        header_of(&working_indicator(&mut chat)),
        "I should read the config loader first before changing the default."
    );
}

#[test]
fn working_indicator_follows_the_latest_bold_thought_header() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);

    send(
        &mut chat,
        vec![
            thought("**Reading the config loader**\n\nThe loader merges three sources."),
            thought("\n\n**Planning the default change**\n\nFlip the default in one place."),
        ],
    );

    assert_eq!(
        header_of(&working_indicator(&mut chat)),
        "Planning the default change"
    );
}

#[test]
fn a_new_thought_after_tool_work_replaces_the_previous_thought_in_the_working_indicator() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);

    send(
        &mut chat,
        vec![
            thought("**Reading the config loader**\n\nThe loader merges three sources."),
            tool_call("call-1", "Read src/config.rs"),
            thought("Now the precedence is clear; next I'll write the test."),
        ],
    );

    assert_eq!(
        header_of(&working_indicator(&mut chat)),
        "Now the precedence is clear; next I'll write the test."
    );
}

#[test]
fn a_new_thought_after_answer_text_replaces_the_previous_thought_in_the_working_indicator() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);

    send(
        &mut chat,
        vec![
            thought("Checking which files mention the setting."),
            answer("Found two files."),
            thought("Comparing both call sites."),
        ],
    );

    assert_eq!(
        header_of(&working_indicator(&mut chat)),
        "Comparing both call sites."
    );
}

#[test]
fn a_half_streamed_bold_header_waits_for_its_closing_marker() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);

    send(&mut chat, vec![thought("Checking the loader.")]);
    send(
        &mut chat,
        vec![answer("Done."), thought("**Planning the def")],
    );
    assert_eq!(
        header_of(&working_indicator(&mut chat)),
        "Checking the loader."
    );

    send(&mut chat, vec![thought("ault change**\n\nOne place.")]);
    assert_eq!(
        header_of(&working_indicator(&mut chat)),
        "Planning the default change"
    );
}

#[test]
fn a_long_thought_line_is_shortened_so_the_interrupt_hint_stays_visible() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);

    let long_line = "I am going to read every module that touches the configuration loader, \
                     compare how each one resolves precedence, and then decide where to flip it.";
    send(&mut chat, vec![thought(long_line)]);

    let row = working_indicator(&mut chat);
    let header = header_of(&row);
    assert!(header.ends_with("..."), "{row}");
    assert!(
        long_line.starts_with(header.trim_end_matches("...")),
        "{row}"
    );
}

#[test]
fn a_stray_double_asterisk_in_prose_does_not_hide_the_real_header() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);

    send(
        &mut chat,
        vec![thought(
            "Raising 2**10 gives 1024.\n\n**Planning the change**\n\nOne place.",
        )],
    );

    assert_eq!(
        header_of(&working_indicator(&mut chat)),
        "Planning the change"
    );
}
