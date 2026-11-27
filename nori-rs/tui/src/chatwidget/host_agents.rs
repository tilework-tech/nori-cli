//! `/agent` (or `/agents`) on a joined host: choose one of the ACP agent
//! servers the host listed when this terminal connected, and start it there.
//!
//! - With no session on screen, the host starts a session with it.
//! - A session that has not been prompted changes agent in place through
//!   the host's `agent` configuration option, keeping its ID.
//! - A prompted session keeps its conversation; the chosen agent starts in
//!   a new session beside it, in the same folder.

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::history_cell;
use crate::host_client::HostCall;
use crate::host_client::agents::HostAgent;

use super::host_config::AGENT_OPTION;

/// An in-place agent change this terminal asked for.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentChange {
    #[default]
    None,
    /// Asked; the old agent's shutdown has not arrived.
    Asked,
    /// The old agent shut down; the host has not answered yet.
    ShutDown,
}

impl AgentChange {
    /// Whether a shutdown is the old agent stopping for the change.
    pub(super) fn absorb_shutdown(&mut self) -> bool {
        match self {
            Self::Asked => {
                *self = Self::ShutDown;
                true
            }
            Self::None | Self::ShutDown => false,
        }
    }
}

/// What choosing an agent does to the session on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Start,
    Replace,
    StartBeside,
}

impl ChatWidget {
    fn agent_choice(&self) -> Choice {
        if self.managed_conversation_id.is_none() {
            Choice::Start
        } else if self.session_stats.user_messages > 0 || self.first_prompt_text.is_some() {
            Choice::StartBeside
        } else {
            Choice::Replace
        }
    }

    /// The host's agents, the session's own marked current.
    pub(super) fn open_host_agent_popup(&mut self) {
        let agents = self
            .host_connection
            .as_ref()
            .map(|connection| connection.agents.clone())
            .unwrap_or_default();
        let current = self
            .managed_conversation_id
            .map(|_| nori_harness::resolve_agent_slug(&self.config.active_agent));
        let subtitle = match self.agent_choice() {
            Choice::Start => "Starts a session with it on this host",
            Choice::Replace => "Changes this session's agent; it has no prompts yet",
            Choice::StartBeside => "Starts a new session with it; this conversation keeps running",
        };
        let items = agents
            .into_iter()
            .map(|agent| {
                let is_current = current.as_deref() == Some(agent.slug.as_str());
                agent_item(agent, is_current)
            })
            .collect();
        self.bottom_pane.show_selection_view(
            SelectionViewParams {
                title: Some("Choose an agent".to_string()),
                subtitle: Some(subtitle.to_string()),
                footer_hint: Some(standard_popup_hint_line()),
                items,
                ..Default::default()
            }
            .picker(),
        );
    }

    /// Start or switch to `slug` on the host, as the picker promised.
    pub(crate) fn choose_host_agent(&mut self, slug: String, name: String) {
        if crate::command_availability::unavailable_reason(
            crate::slash_command::SlashCommand::Agent,
            self.command_surface(),
        )
        .is_some()
        {
            self.ensure_builtin_command_enabled(crate::slash_command::SlashCommand::Agent);
            return;
        }
        let choice = self.agent_choice();
        match (choice, self.managed_conversation_id) {
            (Choice::Replace, Some(id)) => {
                self.agent_change = AgentChange::Asked;
                let tx = self.app_event_tx.clone();
                let reply = tx.host_receipt(HostCall::SetConfigOption {
                    session_id: id.to_string(),
                    config_id: AGENT_OPTION.to_string(),
                    value: slug.clone(),
                });
                tokio::spawn(async move {
                    match reply.await {
                        Ok(result) => tx.send(AppEvent::HostAgentReplaced {
                            session_id: id,
                            slug,
                            name,
                            options: super::host_config::reported_options(&result),
                        }),
                        Err(error) => tx.send(AppEvent::HostAgentNotReplaced {
                            session_id: id,
                            message: format!(
                                "The host could not change the agent to {name}: {error}"
                            ),
                        }),
                    }
                });
            }
            (Choice::StartBeside, _) => {
                let note = format!(
                    "Started a new session with {name}. The previous conversation keeps running; Alt+S switches to it."
                );
                self.start_host_session_with("", Some(&slug), Some(note));
            }
            (Choice::Start | Choice::Replace, _) => {
                self.start_host_session_with("", Some(&slug), None);
            }
        }
    }

    /// The host refused to change this session's agent. If the old agent
    /// already stopped, the session has ended after all.
    pub(crate) fn on_host_agent_not_replaced(&mut self, message: String) {
        let change = std::mem::take(&mut self.agent_change);
        self.add_error_message(message);
        if change == AgentChange::ShutDown {
            self.app_event_tx.send(AppEvent::SessionClosed {
                generation: self.session_generation,
            });
        }
    }

    /// The host changed this session's agent in place.
    pub(crate) fn on_host_agent_replaced(
        &mut self,
        slug: String,
        name: String,
        options: Vec<nori_protocol::acp::v1::SessionConfigOption>,
    ) {
        self.agent_change = AgentChange::None;
        self.adopt_host_config_answer(options);
        self.config.active_agent.clone_from(&slug);
        self.config.agent = slug;
        self.bottom_pane.set_agent_display_name(name.clone());
        self.add_info_message(format!("This session now runs {name}."), None);
    }
}

fn agent_item(agent: HostAgent, is_current: bool) -> SelectionItem {
    let can_start = agent.can_start();
    let HostAgent { slug, name, note } = agent;
    let search_value = Some(format!("{name} {slug}"));
    let actions: Vec<SelectionAction> = if can_start {
        let name = name.clone();
        vec![Box::new(move |tx| {
            tx.send(AppEvent::ChooseHostAgent {
                slug: slug.clone(),
                name: name.clone(),
            });
        })]
    } else {
        let message = format!("{name} cannot start on this host: {note}");
        vec![Box::new(move |tx| {
            tx.send(AppEvent::InsertHistoryCell(Box::new(
                history_cell::new_error_event(message.clone()),
            )));
        })]
    };
    SelectionItem {
        name,
        description: Some(note),
        is_current,
        actions,
        dismiss_on_select: can_start,
        search_value,
        ..Default::default()
    }
}
