//! The exit card summarises what the session's own events reported.
use super::*;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;

fn acp_event(event: nori_protocol::AcpEvent) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(event)
}

fn session_update(update: acp::SessionUpdate) -> nori_protocol::SessionEvent {
    acp_event(nori_protocol::AcpEvent::Notification(
        acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
            "session", update,
        )),
    ))
}

fn prompt_response(request: &str, usage: acp::Usage) -> nori_protocol::SessionEvent {
    acp_event(nori_protocol::AcpEvent::Response {
        request_id: acp::RequestId::Str(request.to_string()),
        response: Ok(acp::AgentResponse::PromptResponse(
            acp::PromptResponse::new(acp::StopReason::EndTurn).usage(usage),
        )),
    })
}

fn exit_card_rows(chat: &ChatWidget, block: &str) -> Vec<String> {
    let text: Vec<String> = chat
        .create_exit_message_cell()
        .display_lines(80)
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    let start = text
        .iter()
        .position(|row| row.trim_start().starts_with(block))
        .unwrap_or_else(|| panic!("no {block} row in {text:#?}"));
    let mut rows = vec![text[start].clone()];
    rows.extend(
        text[start + 1..]
            .iter()
            .take_while(|row| row.starts_with("               "))
            .cloned(),
    );
    rows
}

#[test]
fn token_totals_sum_every_reported_turn_including_ones_this_widget_did_not_send() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        prompt_response(
            "typed-prompt",
            acp::Usage::new(1_500, 1_000, 500).cached_read_tokens(800),
        ),
    );
    chat.handle_session_event(
        generation,
        prompt_response(
            "queued-prompt",
            acp::Usage::new(2_700, 2_000, 700).thought_tokens(300),
        ),
    );

    assert_eq!(
        exit_card_rows(&chat, "Tokens"),
        vec![
            "  Tokens       4,200  total".to_string(),
            "               3,000  input".to_string(),
            "                 800  cached read".to_string(),
            "               1,200  output".to_string(),
            "                 300  reasoning".to_string(),
        ]
    );
}

#[test]
fn a_session_whose_agent_never_reports_usage_shows_no_usage_section() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        acp_event(nori_protocol::AcpEvent::Response {
            request_id: acp::RequestId::Str("prompt".to_string()),
            response: Ok(acp::AgentResponse::PromptResponse(
                acp::PromptResponse::new(acp::StopReason::EndTurn),
            )),
        }),
    );

    let card = lines_to_single_string(&chat.create_exit_message_cell().display_lines(80));
    assert!(!card.contains("Tokens"), "{card}");
    assert!(!card.contains("Cost"), "{card}");
}

#[test]
fn context_and_cost_come_from_the_last_usage_update() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    for (used, cost) in [(90_000, 1.1), (164_208, 3.42)] {
        chat.handle_session_event(
            generation,
            session_update(acp::SessionUpdate::UsageUpdate(
                acp::UsageUpdate::new(used, 1_000_000).cost(acp::Cost::new(cost, "USD")),
            )),
        );
    }

    assert_eq!(
        exit_card_rows(&chat, "Context"),
        vec!["  Context      164,208 of 1,000,000 · 16% full".to_string()]
    );
    assert_eq!(
        exit_card_rows(&chat, "Cost"),
        vec!["  Cost         $3.42 · reported by the agent".to_string()]
    );
}

#[test]
fn the_session_title_appears_under_the_session_id() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.conversation_id = Some(nori_harness::ConversationId::new());
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        session_update(acp::SessionUpdate::SessionInfoUpdate(
            acp::SessionInfoUpdate::new().title("Tidy the release notes".to_string()),
        )),
    );

    let card = lines_to_single_string(&chat.create_exit_message_cell().display_lines(80));
    assert!(card.contains("Title  Tidy the release notes"), "{card}");
}
