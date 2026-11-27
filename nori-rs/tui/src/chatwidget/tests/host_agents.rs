//! `/agent` in a terminal joined to a host lists the agents that host
//! reported, and choosing one asks that host to start it.

use super::*;
use crate::host_client::HostCall;
use crate::host_client::HostConnection;
use crate::host_client::HostFeatures;
use crate::host_client::agents::HostAgent;
use nori_protocol::ControlRole;
use pretty_assertions::assert_eq;

fn host(role: ControlRole) -> HostConnection {
    HostConnection {
        role,
        connected: true,
        stopped: None,
        features: HostFeatures {
            agents: true,
            ..HostFeatures::default()
        },
        agents: vec![
            HostAgent {
                slug: "amber".into(),
                name: "Amber".into(),
                note: "Installed".into(),
            },
            HostAgent {
                slug: "cedar".into(),
                name: "Cedar".into(),
                note: "Adapter not installed. Install the synthetic cedar adapter.".into(),
            },
        ],
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

fn press(chat: &mut ChatWidget, code: KeyCode) {
    chat.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
}

/// Deliver each `/agent` choice back to the widget, as the App does, and
/// return the host calls and printed text that followed.
fn settle(
    chat: &mut ChatWidget,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> (Vec<HostCall>, String) {
    let mut calls = Vec::new();
    let mut text = String::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            AppEvent::ChooseHostAgent { slug, name } => chat.choose_host_agent(slug, name),
            AppEvent::HostRequest { call, .. } => calls.push(call),
            AppEvent::InsertHistoryCell(cell) => {
                text.push_str(&lines_to_single_string(&cell.display_lines(200)));
            }
            AppEvent::SessionHistory { event, .. } => {
                if let AppEvent::InsertHistoryCell(cell) = *event {
                    text.push_str(&lines_to_single_string(&cell.display_lines(200)));
                }
            }
            _ => {}
        }
    }
    (calls, text)
}

#[test]
fn the_picker_lists_the_hosts_agents_and_why_one_cannot_start() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(host(ControlRole::Controller));
    chat.dispatch_command(SlashCommand::Agent);
    insta::assert_snapshot!("joined_agent_picker", render_bottom_pane(&chat));
}

#[tokio::test]
async fn choosing_an_agent_with_no_session_asks_the_host_to_start_it() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(host(ControlRole::Controller));
    chat.dispatch_command(SlashCommand::Agent);
    press(&mut chat, KeyCode::Enter);
    let (calls, _) = settle(&mut chat, &mut rx);
    let started: Vec<&serde_json::Value> = calls
        .iter()
        .filter_map(|call| match call {
            HostCall::NewSession(params) => Some(params),
            _ => None,
        })
        .collect();
    assert_eq!(started.len(), 1, "{calls:?}");
    assert_eq!(started[0]["agent"], "amber");
}

#[tokio::test]
async fn an_agent_the_host_cannot_start_says_why_and_starts_nothing() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(host(ControlRole::Controller));
    chat.dispatch_command(SlashCommand::Agent);
    press(&mut chat, KeyCode::Down);
    press(&mut chat, KeyCode::Enter);
    let (calls, text) = settle(&mut chat, &mut rx);
    assert_eq!(calls.len(), 0, "{calls:?}");
    assert!(
        text.contains(
            "Cedar cannot start on this host: Adapter not installed. Install the synthetic cedar adapter."
        ),
        "{text}"
    );
}

#[tokio::test]
async fn a_choice_that_lands_after_control_is_lost_starts_nothing() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(host(ControlRole::Controller));
    chat.dispatch_command(SlashCommand::Agent);
    press(&mut chat, KeyCode::Enter);
    chat.set_host_connection(host(ControlRole::Watcher));
    let (calls, _) = settle(&mut chat, &mut rx);
    assert_eq!(calls.len(), 0, "{calls:?}");
}
