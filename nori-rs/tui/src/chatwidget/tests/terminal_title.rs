//! The terminal title names the agent, the project and the session, in that
//! order, and keeps the working spinner in front.
use super::*;
use pretty_assertions::assert_eq;

fn title_update(title: Option<&str>) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Notification(
        nori_protocol::acp::v1::AgentNotification::SessionNotification(
            nori_protocol::acp::v1::SessionNotification::new(
                "session",
                nori_protocol::acp::v1::SessionUpdate::SessionInfoUpdate(
                    nori_protocol::acp::v1::SessionInfoUpdate::new()
                        .title(title.map(str::to_string)),
                ),
            ),
        ),
    ))
}

fn chat_in(project: &str, agent: &str) -> ChatWidget {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.config.animations = false;
    chat.config.cwd = PathBuf::from("/synthetic").join(project);
    chat.bottom_pane.set_agent_display_name(agent.to_string());
    chat.set_foreground(true);
    chat
}

fn shown_title(chat: &ChatWidget) -> Option<&str> {
    chat.last_terminal_title.as_deref()
}

#[test]
fn a_fresh_session_shows_the_project_alone() {
    let chat = chat_in("harbor-ledger", "Claude Code");
    assert_eq!(shown_title(&chat), Some("harbor-ledger"));
}

#[test]
fn the_agent_title_names_the_session_and_follows_renames() {
    let mut chat = chat_in("harbor-ledger", "Claude Code");
    let generation = chat.session_generation;

    chat.handle_session_event(generation, title_update(Some("Fix login flakes")));
    assert_eq!(
        shown_title(&chat),
        Some("Claude Code · harbor-ledger · Fix login flakes")
    );

    chat.handle_session_event(generation, title_update(Some("Speed up the CI cache")));
    assert_eq!(
        shown_title(&chat),
        Some("Claude Code · harbor-ledger · Speed up the CI cache")
    );

    chat.handle_session_event(generation, title_update(None));
    assert_eq!(shown_title(&chat), Some("harbor-ledger"));
}

#[test]
fn a_custom_agent_is_named_by_its_own_label() {
    let mut chat = chat_in("harbor-ledger", "night-shift-reviewer");
    let generation = chat.session_generation;
    chat.handle_session_event(generation, title_update(Some("Review open diffs")));
    assert_eq!(
        shown_title(&chat),
        Some("night-shift… · harbor-ledger · Review open diffs")
    );
}

#[test]
fn without_an_agent_title_the_first_prompt_names_the_session() {
    let mut chat = chat_in("harbor-ledger", "Pi");
    let generation = chat.session_generation;
    type_text(&mut chat, "Tidy the README badges");
    chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    // Pi titled the session from the envelope and was cut off inside it.
    chat.handle_session_event(
        generation,
        title_update(Some(
            "<context>\nSource: this message is from Nori CLI.\n\nYou are operat",
        )),
    );
    assert_eq!(
        shown_title(&chat),
        Some("Pi · harbor-ledger · Tidy the README badges")
    );
}

#[test]
fn a_parked_session_leaves_the_title_until_it_returns() {
    let mut chat = chat_in("harbor-ledger", "Codex");
    let generation = chat.session_generation;
    chat.set_foreground(false);
    chat.handle_session_event(generation, title_update(Some("Background chore")));
    assert_eq!(shown_title(&chat), Some("harbor-ledger"));

    chat.set_foreground(true);
    assert_eq!(
        shown_title(&chat),
        Some("Codex · harbor-ledger · Background chore")
    );
}

#[test]
fn the_working_spinner_stays_in_front_of_the_full_title() {
    let mut chat = chat_in("harbor-ledger", "Codex");
    let generation = chat.session_generation;
    chat.handle_session_event(generation, title_update(Some("Fix login flakes")));
    chat.config.animations = true;
    chat.bottom_pane.set_task_running(true);
    chat.pre_draw_tick();

    let title = shown_title(&chat).expect("title written");
    let (frame, rest) = title.split_once(' ').expect("spinner prefix");
    assert!(
        crate::terminal_title::SPINNER_FRAMES.contains(&frame),
        "{title}"
    );
    assert_eq!(rest, "Codex · harbor-ledger · Fix login flakes");
}
