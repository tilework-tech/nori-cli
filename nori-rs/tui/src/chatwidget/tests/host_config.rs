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
