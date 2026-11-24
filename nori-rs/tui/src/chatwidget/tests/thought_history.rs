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

fn replay_started() -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayStarted(
        nori_protocol::ReplayStarted {
            source: nori_protocol::ReplaySource::Agent,
        },
    ))
}

fn replay_finished() -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ReplayFinished)
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

fn complete_prompt(chat: &mut ChatWidget) {
    chat.handle_client_event(crate::presentation::ClientEvent::PromptCompleted(
        crate::presentation::PromptCompleted {
            stop_reason: nori_protocol::acp::v1::StopReason::EndTurn,
            last_agent_message: None,
            failure: None,
        },
    ));
}

fn send(chat: &mut ChatWidget, events: Vec<nori_protocol::SessionEvent>) {
    let generation = chat.session_generation;
    for event in events {
        chat.handle_session_event(generation, event);
    }
}

/// History cells as plain text, one entry per cell, using the chat view.
fn history_cells(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> Vec<String> {
    let mut cells = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            let text = lines_to_single_string(&cell.display_lines(80));
            if !text.trim().is_empty() {
                cells.push(text);
            }
        }
    }
    cells
}

/// History cells as plain text, one entry per cell, using the Ctrl+T transcript.
fn transcript_cells(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> Vec<String> {
    let mut cells = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            let text = lines_to_single_string(&cell.transcript_lines(80));
            if !text.trim().is_empty() {
                cells.push(text);
            }
        }
    }
    cells
}

fn thought_cells(cells: &[String]) -> Vec<String> {
    cells
        .iter()
        .filter(|cell| cell.starts_with("│ "))
        .cloned()
        .collect()
}

/// A turn that thinks in split chunks, reads a file, thinks again with two
/// headings streamed token by token, then answers.
fn mixed_turn() -> Vec<nori_protocol::SessionEvent> {
    vec![
        thought("**Reading the config"),
        thought(" loader**\n\nThe loader merges "),
        thought("three sources."),
        tool_call("call-1", "Read src/config.rs"),
        thought("**"),
        thought("Weighing"),
        thought(" precedence"),
        thought("**"),
        thought("\n\n"),
        thought("The file wins over the environment."),
        thought("\n\n"),
        thought("**"),
        thought("Choosing the edit"),
        thought("**"),
        thought("\n\n"),
        thought("Flip the default in one place."),
        answer("The default now lives in one place."),
    ]
}

const MIXED_TURN_THOUGHTS: [&str; 2] = [
    "│ Reading the config loader\n│\n│ The loader merges three sources.\n",
    "│ Weighing precedence\n│\n│ The file wins over the environment.\n│\n│ Choosing the edit\n│\n│ Flip the default in one place.\n",
];

#[test]
fn live_thought_text_appears_once_as_its_own_block_before_the_answer() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);

    send(
        &mut chat,
        vec![
            thought("I should read the config loader"),
            thought(" before changing the default."),
            answer("Done."),
        ],
    );
    complete_prompt(&mut chat);

    let cells = history_cells(&mut rx);
    let thoughts = thought_cells(&cells);
    assert_eq!(
        thoughts,
        vec!["│ I should read the config loader before changing the default.\n".to_string()]
    );
    let thought_index = cells.iter().position(|cell| cell == &thoughts[0]);
    let answer_index = cells.iter().position(|cell| cell.contains("Done."));
    assert!(
        thought_index < answer_index,
        "thought before answer: {cells:?}"
    );
    assert!(
        cells
            .iter()
            .filter(|cell| !cell.starts_with("│ "))
            .all(|cell| !cell.contains("read the config loader")),
        "thought text only in its own block: {cells:?}"
    );
}

#[test]
fn headings_streamed_token_by_token_keep_their_own_lines() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);

    send(&mut chat, mixed_turn());
    complete_prompt(&mut chat);

    assert_eq!(
        thought_cells(&history_cells(&mut rx)),
        MIXED_TURN_THOUGHTS.map(str::to_string).to_vec()
    );
}

#[test]
fn bold_text_split_inside_a_sentence_stays_in_the_sentence() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);

    send(
        &mut chat,
        vec![
            thought("I should check the synthetic "),
            thought("**"),
            thought("loader"),
            thought(".toml"),
            thought("**"),
            thought(" first."),
            answer("Waiting."),
        ],
    );

    assert_eq!(
        thought_cells(&history_cells(&mut rx)),
        vec!["│ I should check the synthetic loader.toml first.\n".to_string()]
    );
}

#[test]
fn whole_thought_chunks_are_stored_as_supplied() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);

    send(
        &mut chat,
        vec![
            thought("**Weighing precedence**\nThe file wins."),
            thought("**Choosing the edit**\nOne place."),
            answer("Done."),
        ],
    );

    assert_eq!(
        thought_cells(&history_cells(&mut rx)),
        vec![
            "│ Weighing precedence\n│ The file wins.Choosing the edit\n│ One place.\n".to_string()
        ]
    );
}

#[test]
fn a_thought_that_ends_the_turn_is_kept_once() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);
    send(&mut chat, vec![thought("Nothing left to change.")]);
    complete_prompt(&mut chat);

    start_prompt(&mut chat);
    send(&mut chat, vec![answer("Next turn.")]);
    complete_prompt(&mut chat);

    assert_eq!(
        thought_cells(&history_cells(&mut rx)),
        vec!["│ Nothing left to change.\n".to_string()]
    );
}

#[test]
fn replayed_thoughts_render_the_same_blocks_as_live_thoughts() {
    let (mut live, mut live_rx, _live_op_rx) = make_chatwidget_manual();
    start_prompt(&mut live);
    send(&mut live, mixed_turn());
    complete_prompt(&mut live);
    let live_thoughts = thought_cells(&history_cells(&mut live_rx));

    let (mut replayed, mut replay_rx, _replay_op_rx) = make_chatwidget_manual();
    let mut events = vec![replay_started()];
    events.extend(mixed_turn());
    events.push(replay_finished());
    send(&mut replayed, events);

    assert_eq!(thought_cells(&history_cells(&mut replay_rx)), live_thoughts);
}

#[test]
fn replay_entries_split_per_chunk_join_into_one_block() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    for text in ["**Reading", " the loader**\n\nIt merges ", "three sources."] {
        chat.handle_client_event(crate::presentation::ClientEvent::ReplayEntry(
            crate::presentation::ReplayEntry::ReasoningMessage {
                text: text.to_string(),
            },
        ));
    }
    chat.handle_client_event(crate::presentation::ClientEvent::ReplayEntry(
        crate::presentation::ReplayEntry::AssistantMessage {
            text: "Done.".to_string(),
        },
    ));

    assert_eq!(
        thought_cells(&history_cells(&mut rx)),
        vec!["│ Reading the loader\n│\n│ It merges three sources.\n".to_string()]
    );
}

#[test]
fn transcript_overlay_shows_the_same_thought_block() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    start_prompt(&mut chat);
    send(&mut chat, mixed_turn());
    complete_prompt(&mut chat);

    assert_eq!(
        thought_cells(&transcript_cells(&mut rx)),
        MIXED_TURN_THOUGHTS.map(str::to_string).to_vec()
    );
}

#[test]
fn opting_out_keeps_thoughts_in_the_working_indicator_only() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.config.thoughts_in_history = false;
    start_prompt(&mut chat);

    send(
        &mut chat,
        vec![thought(
            "**Reading the config loader**\n\nThe loader merges three sources.",
        )],
    );
    assert_eq!(
        chat.bottom_pane
            .status_widget()
            .map(|status| status.header().to_string()),
        Some("Reading the config loader".to_string())
    );

    send(&mut chat, vec![answer("Done.")]);
    complete_prompt(&mut chat);
    let mut events = vec![replay_started()];
    events.extend(mixed_turn());
    events.push(replay_finished());
    send(&mut chat, events);

    let cells = history_cells(&mut rx);
    assert_eq!(thought_cells(&cells), Vec::<String>::new());
    assert!(
        cells
            .iter()
            .all(|cell| !cell.contains("merges three sources")),
        "{cells:?}"
    );
}

#[test]
fn thought_block_snapshot() {
    let cell = crate::history_cell::ThoughtCell::new(
        "**Reading the config loader**\n\nThe loader merges three sources: the user file, the \
         project file, and command-line overrides. The `tui` table is read last."
            .to_string(),
    )
    .expect("a non-empty thought");
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(60, 6)).expect("test terminal");
    terminal
        .draw(|frame| {
            let area = frame.area();
            ratatui::widgets::Paragraph::new(ratatui::text::Text::from(
                cell.display_lines(area.width),
            ))
            .render(area, frame.buffer_mut());
        })
        .expect("draw");
    insta::assert_snapshot!(terminal.backend());
}
