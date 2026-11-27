//! `/config` in a terminal joined to a host offers the agent's own options
//! from the session's advertised configuration, and leaves the agent and
//! permission mode to `/agent` and `/approvals`.

use super::*;
use crate::host_client::HostConnection;
use crate::host_client::HostFeatures;
use nori_protocol::ControlRole;
use nori_protocol::acp::v1 as acp;

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

fn select(
    id: &str,
    name: &str,
    category: acp::SessionConfigOptionCategory,
    values: &[&str],
) -> acp::SessionConfigOption {
    acp::SessionConfigOption::select(
        id.to_string(),
        name.to_string(),
        values[0].to_string(),
        values
            .iter()
            .map(|value| acp::SessionConfigSelectOption::new(value.to_string(), value.to_string()))
            .collect::<Vec<_>>(),
    )
    .category(category)
}

#[test]
fn the_joined_config_panel_offers_the_agents_own_options() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(HostConnection {
        role: ControlRole::Controller,
        connected: true,
        stopped: None,
        features: HostFeatures {
            session_config: true,
            ..HostFeatures::default()
        },
        agents: Vec::new(),
    });
    chat.handle_acp_session_config_update(&[
        select(
            "permission",
            "Permission",
            acp::SessionConfigOptionCategory::Mode,
            &["explore", "build"],
        ),
        select(
            "model",
            "Model",
            acp::SessionConfigOptionCategory::Model,
            &["synthetic-steady", "synthetic-swift"],
        ),
        select(
            "effort",
            "Effort",
            acp::SessionConfigOptionCategory::ThoughtLevel,
            &["low", "high"],
        ),
    ]);
    chat.dispatch_command(SlashCommand::Config);
    insta::assert_snapshot!("joined_config_panel", render_bottom_pane(&chat));
}

/// A joined controller's widget that has seen its session start with
/// `options`.
fn started_with(options: Vec<acp::SessionConfigOption>) -> ChatWidget {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.set_host_connection(HostConnection {
        role: ControlRole::Controller,
        connected: true,
        stopped: None,
        features: HostFeatures {
            session_config: true,
            ..HostFeatures::default()
        },
        agents: Vec::new(),
    });
    feed(&mut chat, started(options));
    chat
}

fn started(options: Vec<acp::SessionConfigOption>) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionStarted(
        nori_protocol::SessionStarted {
            transcript_id: None,
            acp_session_id: acp::SessionId::new("synthetic-session"),
            cwd: std::path::PathBuf::from("/synthetic/project"),
            transcript_path: None,
            history_log_id: 0,
            history_entry_count: 0,
            config_options: options,
        },
    ))
}

fn answer(response: acp::AgentResponse) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
        request_id: acp::RequestId::Number(11),
        response: Ok(response),
    })
}

/// Deliver `event` as a joined terminal reads it: through the wire.
fn feed(chat: &mut ChatWidget, event: nori_protocol::SessionEvent) {
    let wire = serde_json::to_value(&event).expect("event serializes");
    let event = serde_json::from_value(wire).expect("event decodes");
    let generation = chat.session_generation();
    chat.handle_session_event(generation, event);
}

/// Each option the widget shows, with its current value.
fn shown(chat: &ChatWidget) -> Vec<(String, String)> {
    chat.agent_config()
        .raw_options()
        .into_iter()
        .filter_map(|option| match option.kind {
            acp::SessionConfigKind::Select(select) => {
                Some((option.id.to_string(), select.current_value.to_string()))
            }
            _ => None,
        })
        .collect()
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(id, value)| (id.to_string(), value.to_string()))
        .collect()
}

fn model(current: &str) -> acp::SessionConfigOption {
    select(
        "model",
        "Model",
        acp::SessionConfigOptionCategory::Model,
        &[current, "synthetic-other"],
    )
}

fn permission(current: &str) -> acp::SessionConfigOption {
    select(
        "permission",
        "Permission",
        acp::SessionConfigOptionCategory::Mode,
        &[current, "synthetic-other"],
    )
}

#[test]
fn a_journaled_answer_changes_and_removes_the_agents_own_mode() {
    let mut chat = started_with(vec![model("initial"), permission("explore")]);

    feed(
        &mut chat,
        answer(acp::AgentResponse::SetSessionConfigOptionResponse(
            acp::SetSessionConfigOptionResponse::new(vec![model("birch"), permission("edit")]),
        )),
    );
    assert_eq!(
        shown(&chat),
        pairs(&[("model", "birch"), ("permission", "edit")])
    );

    feed(
        &mut chat,
        answer(acp::AgentResponse::SetSessionConfigOptionResponse(
            acp::SetSessionConfigOptionResponse::new(vec![model("birch")]),
        )),
    );
    assert_eq!(shown(&chat), pairs(&[("model", "birch")]));
}

#[test]
fn a_journaled_load_answer_replaces_options_only_when_it_reports_them() {
    let mut chat = started_with(vec![model("initial")]);

    feed(
        &mut chat,
        answer(acp::AgentResponse::LoadSessionResponse(
            acp::LoadSessionResponse::new(),
        )),
    );
    assert_eq!(shown(&chat), pairs(&[("model", "initial")]));

    feed(
        &mut chat,
        answer(acp::AgentResponse::ResumeSessionResponse(
            acp::ResumeSessionResponse::new().config_options(vec![]),
        )),
    );
    assert_eq!(shown(&chat), Vec::new());
}

#[test]
fn a_restart_under_the_same_id_with_no_options_drops_the_previous_agents() {
    let mut chat = started_with(vec![model("initial")]);
    feed(
        &mut chat,
        answer(acp::AgentResponse::SetSessionConfigOptionResponse(
            acp::SetSessionConfigOptionResponse::new(vec![model("synthetic-other")]),
        )),
    );
    feed(
        &mut chat,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionEnded(
            nori_protocol::SessionEnded {
                reason: nori_protocol::SessionEndReason::Shutdown,
                message: None,
            },
        )),
    );
    feed(&mut chat, started(Vec::new()));
    assert_eq!(shown(&chat), Vec::new());
}

#[test]
fn an_untagged_config_answer_from_an_older_journal_is_shown_as_a_snapshot() {
    let mut chat = started_with(vec![model("initial"), permission("explore")]);
    for (result, expected) in [
        (
            serde_json::json!({"configOptions": [model("birch")]}),
            pairs(&[("model", "birch")]),
        ),
        (
            serde_json::json!({"configOptions": null}),
            pairs(&[("model", "birch")]),
        ),
        (serde_json::json!({}), pairs(&[("model", "birch")])),
        (serde_json::json!({"configOptions": []}), Vec::new()),
    ] {
        let event = serde_json::from_value(serde_json::json!({
            "source": "acp",
            "event": {"message_type": "response", "request_id": 11, "response": {"Ok": result}},
        }))
        .expect("an older journal record decodes");
        let generation = chat.session_generation();
        chat.handle_session_event(generation, event);
        assert_eq!(shown(&chat), expected, "{result}");
    }
}
