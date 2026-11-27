//! `/model` and `/config` on a joined host: the pickers open from the
//! configuration the session on screen advertised, and a choice is sent to
//! the host as `session/set_config_option`. Only the host's answer changes
//! what this terminal shows, and it goes to the session the choice was made
//! in, even after a switch.
//!
//! The host owns two options of every session: its agent (`/agent`) and its
//! permission mode, behind which it moves the adapter's own mode
//! (`/approvals`). Neither is offered here. Models that need the agent
//! restarted on this machine are not offered either.
//!
//! The host journals the agent's answer to every change, so a terminal that
//! watches the session or joins it later adopts the same configuration.

use nori_protocol::acp::v1 as acp;

use super::ChatWidget;
use crate::app_event::AppEvent;
use crate::host_client::HostCall;

/// The host's option for a session's agent, and its category.
pub(super) const AGENT_OPTION: &str = "agent";
const AGENT_CATEGORY: &str = "_nori_agent";
/// The host's option for a session's permission mode.
const MODE_OPTION: &str = "mode";

/// Whether the host, not the adapter, owns `option` in a joined session.
fn host_owned(option: &acp::SessionConfigOption) -> bool {
    let id = option.id.to_string();
    id == AGENT_OPTION
        || id == MODE_OPTION
        || matches!(
            &option.category,
            Some(acp::SessionConfigOptionCategory::Mode)
        )
        || matches!(
            &option.category,
            Some(acp::SessionConfigOptionCategory::Other(category)) if category == AGENT_CATEGORY
        )
}

/// The session's configuration after the host answered with `reported`:
/// every adapter option the host reported replaces the last one, and an
/// adapter option the host leaves out because it owns it keeps its value.
fn merge(
    last: Vec<acp::SessionConfigOption>,
    reported: Vec<acp::SessionConfigOption>,
) -> Vec<acp::SessionConfigOption> {
    let reported: Vec<acp::SessionConfigOption> = reported
        .into_iter()
        .filter(|option| !host_owned(option))
        .collect();
    let kept = last
        .into_iter()
        .filter(|option| host_owned(option) && !reported.iter().any(|new| new.id == option.id));
    reported.iter().cloned().chain(kept).collect()
}

/// The options in a `session/set_config_option` result; an entry this
/// terminal cannot read is skipped.
pub(crate) fn reported_options(result: &serde_json::Value) -> Vec<acp::SessionConfigOption> {
    result["configOptions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|option| serde_json::from_value(option.clone()).ok())
        .collect()
}

impl ChatWidget {
    /// The options `/config` offers on a joined host.
    fn joined_options(&self) -> Vec<acp::SessionConfigOption> {
        self.agent_config()
            .raw_options()
            .into_iter()
            .filter(|option| !host_owned(option))
            .collect()
    }

    pub(super) fn open_joined_model_popup(&mut self) {
        let model = self
            .joined_options()
            .into_iter()
            .find(|option| option.category == Some(acp::SessionConfigOptionCategory::Model));
        match model {
            Some(option) => self.open_joined_value_picker(option),
            None => self.open_model_unsupported_popup(),
        }
    }

    pub(super) fn open_joined_config_popup(&mut self, focus_config_id: Option<String>) {
        let options = self.joined_options();
        self.open_acp_session_config_picker(options, focus_config_id);
    }

    pub(super) fn open_joined_value_picker(&mut self, option: acp::SessionConfigOption) {
        let params = crate::nori::session_config_picker::acp_session_config_value_picker_params(
            &option, None,
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Ask the host to set one option of the session on screen.
    pub(super) fn set_joined_config_option(
        &mut self,
        config_id: String,
        value: String,
        option_name: String,
        value_name: String,
    ) {
        let Some(session_id) = self.managed_conversation_id else {
            self.add_error_message("No session is on screen to configure.".to_string());
            return;
        };
        if !self.ensure_builtin_command_enabled(crate::slash_command::SlashCommand::Config) {
            return;
        }
        let tx = self.app_event_tx.clone();
        let reply = tx.host_receipt(HostCall::SetConfigOption {
            session_id: session_id.to_string(),
            config_id: config_id.clone(),
            value,
        });
        tokio::spawn(async move {
            let result = reply
                .await
                .map(|result| reported_options(&result))
                .map_err(|error| error.to_string());
            tx.send(AppEvent::HostSessionConfigSet {
                session_id,
                config_id,
                option_name,
                value_name,
                result,
            });
        });
    }

    /// Some client changed one of this session's options on the host,
    /// which journaled the agent's answer. Joining later or watching, this
    /// is how the session's current configuration arrives.
    pub(super) fn adopt_host_config_answer(&mut self, reported: Vec<acp::SessionConfigOption>) {
        let options = merge(self.agent_config().raw_options(), reported);
        self.sync_acp_session_config_snapshot(&options);
    }

    /// The host answered a choice made in this session.
    pub(crate) fn on_host_session_config_set(
        &mut self,
        config_id: String,
        option_name: String,
        value_name: String,
        result: Result<Vec<acp::SessionConfigOption>, String>,
    ) {
        match result {
            Ok(reported) => {
                self.adopt_host_config_answer(reported);
                self.add_acp_session_config_set_message(&option_name, &value_name, false);
                if self.foreground {
                    self.open_joined_config_popup(Some(config_id));
                }
            }
            Err(error) => self.add_error_message(format!("Failed to set {option_name}: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn select(
        id: &str,
        category: Option<acp::SessionConfigOptionCategory>,
        current: &str,
    ) -> acp::SessionConfigOption {
        let option = acp::SessionConfigOption::select(
            id.to_string(),
            id.to_string(),
            current.to_string(),
            vec![acp::SessionConfigSelectOption::new(
                current.to_string(),
                current.to_string(),
            )],
        );
        match category {
            Some(category) => option.category(category),
            None => option,
        }
    }

    #[test]
    fn the_hosts_answer_replaces_what_it_reports_and_keeps_the_adapter_mode() {
        let last = vec![
            select(
                "model",
                Some(acp::SessionConfigOptionCategory::Model),
                "initial",
            ),
            select(
                "permission",
                Some(acp::SessionConfigOptionCategory::Mode),
                "explore",
            ),
        ];
        let reported = vec![
            select(
                AGENT_OPTION,
                Some(acp::SessionConfigOptionCategory::Other(
                    AGENT_CATEGORY.into(),
                )),
                "amber",
            ),
            select(
                "model",
                Some(acp::SessionConfigOptionCategory::Model),
                "amber",
            ),
            select(
                MODE_OPTION,
                Some(acp::SessionConfigOptionCategory::Other(MODE_OPTION.into())),
                "agent",
            ),
        ];
        assert_eq!(
            merge(last, reported),
            vec![
                select(
                    "model",
                    Some(acp::SessionConfigOptionCategory::Model),
                    "amber"
                ),
                select(
                    "permission",
                    Some(acp::SessionConfigOptionCategory::Mode),
                    "explore"
                ),
            ]
        );
    }
}
