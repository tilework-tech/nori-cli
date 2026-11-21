//! Agents that title a session from its first prompt must not surface the
//! context envelope Nori prepends to that prompt.
use super::*;
use pretty_assertions::assert_eq;

fn title_update(title: &str) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
        nori_protocol::acp::v1::AgentNotification::SessionNotification(
            nori_protocol::acp::v1::SessionNotification::new(
                "session",
                nori_protocol::acp::v1::SessionUpdate::SessionInfoUpdate(
                    nori_protocol::acp::v1::SessionInfoUpdate::new().title(title.to_string()),
                ),
            ),
        ),
    ))
}

fn history(rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>) -> String {
    drain_insert_history(rx)
        .iter()
        .map(|cell| lines_to_single_string(cell))
        .collect()
}

#[test]
fn a_title_built_from_the_first_prompt_shows_only_what_the_user_typed() {
    for envelope in [
        include_str!("../../../session_context.md"),
        include_str!("../../../session_context_http_mcp.md"),
    ] {
        let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
        let generation = chat.session_generation;
        chat.handle_session_event(
            generation,
            title_update(&format!("{envelope}\nSYNTH-MARKER-91 read notes")),
        );

        assert_eq!(
            chat.bottom_pane.status_footer_values().session_title,
            Some("SYNTH-MARKER-91 read notes".to_string())
        );
        let shown = history(&mut rx);
        assert!(!shown.contains("Source:"), "{shown}");
        assert!(!shown.contains("<context>"), "{shown}");
    }
}

#[test]
fn a_title_cut_off_inside_the_envelope_is_not_shown() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    chat.handle_session_event(
        generation,
        title_update("<context>\nSource: this message is from Nori CLI.\n\nYou are operat"),
    );

    assert_eq!(chat.bottom_pane.status_footer_values().session_title, None);
    let shown = history(&mut rx);
    assert!(!shown.contains("Source:"), "{shown}");
}

#[test]
fn a_title_the_user_wrote_about_context_tags_is_kept() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;
    chat.handle_session_event(generation, title_update("<context> tags in the parser"));

    assert_eq!(
        chat.bottom_pane.status_footer_values().session_title,
        Some("<context> tags in the parser".to_string())
    );
}
