//! A terminal joined to a host it does not own follows the host's role for
//! it: a watcher keeps what it typed and is told how to take control.

use super::*;
use crate::host_client::HostCall;
use crate::host_client::HostConnection;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use nori_protocol::ControlRole;
use pretty_assertions::assert_eq;

fn joined(role: ControlRole, connected: bool) -> HostConnection {
    HostConnection {
        role,
        connected,
        stopped: None,
    }
}

fn render_bottom_pane(chat: &ChatWidget) -> String {
    use crate::render::renderable::Renderable;
    let area = ratatui::layout::Rect::new(0, 0, 80, chat.bottom_pane.desired_height(80));
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    chat.bottom_pane.render(area, &mut buffer);
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

fn asks_host(events: &[AppEvent], wanted: impl Fn(&HostCall) -> bool) -> bool {
    events
        .iter()
        .any(|event| matches!(event, AppEvent::HostRequest { call, .. } if wanted(call)))
}

#[test]
fn a_watcher_keeps_its_draft_and_is_told_to_take_control() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(joined(ControlRole::Watcher, true));

    type_text(&mut chat, "synthetic watcher draft");
    chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(chat.composer_text(), "synthetic watcher draft");
    let (text, events) = printed(&mut rx);
    assert!(text.contains("Alt+T"), "{text}");
    assert!(!asks_host(&events, |call| matches!(
        call,
        HostCall::Prompt(_)
    )));
}

#[test]
fn input_while_reconnecting_keeps_its_draft() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(joined(ControlRole::Controller, false));

    type_text(&mut chat, "synthetic draft during an outage");
    chat.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(chat.composer_text(), "synthetic draft during an outage");
    let (text, events) = printed(&mut rx);
    assert!(text.contains("Reconnecting to the host"), "{text}");
    assert!(!asks_host(&events, |call| matches!(call, HostCall::Prompt(_))));
}

#[test]
fn alt_t_asks_the_host_for_control_only_when_joined() {
    let alt_t = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT);

    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(joined(ControlRole::Watcher, true));
    chat.handle_key_event(alt_t);
    let (_, events) = printed(&mut rx);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AppEvent::TakeHostControl))
    );

    let (mut owned, mut owned_rx, _op_rx) = make_chatwidget_manual();
    owned.handle_key_event(alt_t);
    let (_, events) = printed(&mut owned_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AppEvent::TakeHostControl))
    );
}

#[test]
fn alt_t_while_reconnecting_waits_for_the_connection() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(joined(ControlRole::Watcher, false));
    chat.handle_key_event(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT));
    let (text, events) = printed(&mut rx);
    assert!(text.contains("Reconnecting"), "{text}");
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AppEvent::TakeHostControl))
    );
}

#[test]
fn a_watchers_command_menu_greys_what_needs_control() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(joined(ControlRole::Watcher, true));
    type_text(&mut chat, "/");
    let menu = render_bottom_pane(&chat);
    assert!(
        menu.contains(crate::command_availability::WATCHING),
        "{menu}"
    );
    insta::assert_snapshot!("joined_watcher_command_menu", menu);
}
