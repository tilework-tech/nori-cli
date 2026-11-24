//! The adapter's own mode behind each host permission mode.
//!
//! `ask` puts a built-in adapter in its strictest mode that still asks: no
//! edit and no command that can change anything runs without a permission
//! request. `agent` and `approve-all` put it in a mode where the adapter
//! decides what to ask about; under `approve-all` the host then answers
//! every request itself. The adapter can only be made to ask through its own
//! modes, because ACP lets a client stop a tool call only when the adapter
//! asks about it. An adapter with no strict mode is not offered `ask`, and
//! keeps whatever mode it chose under `agent` and `approve-all`.

use nori_harness::AgentKind;
use nori_harness::ConversationId;
use nori_harness::permission_mode::PermissionMode;
use nori_harness::session_manager::SessionManager;
use nori_protocol::acp::v1 as acp;
use serde_json::json;

use super::AcpError;
use crate::host::service::HostService;

/// A built-in adapter's mode ids for `ask` and for `agent`.
struct AdapterModes {
    ask: &'static str,
    agent: &'static str,
}

fn adapter_modes(agent: &str) -> Option<AdapterModes> {
    match AgentKind::from_slug(agent)? {
        // Claude Code's `default` asks before edits and before any command
        // outside its read-only set; `acceptEdits` allows edits.
        AgentKind::ClaudeCode => Some(AdapterModes {
            ask: "default",
            agent: "acceptEdits",
        }),
        // Codex's `read-only` runs commands in a read-only sandbox and asks
        // before every edit or escalation; `workspace-write` lets it work in
        // the workspace and ask the user, not Codex's auto-reviewer, for more.
        AgentKind::Codex => Some(AdapterModes {
            ask: "read-only",
            agent: "workspace-write",
        }),
        // None of these has a mode that asks before every edit and command:
        // their modes change behavior, or their prompts depend on settings
        // ACP cannot reach.
        AgentKind::Gemini
        | AgentKind::GrokBuild
        | AgentKind::OpenCode
        | AgentKind::Cursor
        | AgentKind::Hermes
        | AgentKind::Pi => None,
    }
}

/// The adapter's mode option: its ID, current value and offered values.
fn adapter_mode_select(
    options: &[acp::SessionConfigOption],
) -> Option<(String, String, Vec<String>)> {
    options.iter().find_map(|option| {
        let is_mode = option.id.0.as_ref() == "mode"
            || option.category == Some(acp::SessionConfigOptionCategory::Mode);
        let acp::SessionConfigKind::Select(select) = &option.kind else {
            return None;
        };
        if !is_mode {
            return None;
        }
        let values = match &select.options {
            acp::SessionConfigSelectOptions::Ungrouped(values) => values
                .iter()
                .map(|choice| choice.value.0.to_string())
                .collect(),
            acp::SessionConfigSelectOptions::Grouped(groups) => groups
                .iter()
                .flat_map(|group| &group.options)
                .map(|choice| choice.value.0.to_string())
                .collect(),
            _ => Vec::new(),
        };
        Some((
            option.id.0.to_string(),
            select.current_value.0.to_string(),
            values,
        ))
    })
}

/// Whether `agent`'s adapter, with these options, has a mode that asks
/// before every edit and command.
pub(super) fn offers_ask(agent: &str, options: &[acp::SessionConfigOption]) -> bool {
    match (adapter_modes(agent), adapter_mode_select(options)) {
        (Some(modes), Some((_, _, values))) => values.iter().any(|value| value == modes.ask),
        (None, _) | (_, None) => false,
    }
}

/// The adapter option and value that put the adapter in the mode behind
/// `mode`, when it is not in that mode already.
pub(super) fn change(
    agent: &str,
    mode: PermissionMode,
    options: &[acp::SessionConfigOption],
) -> Option<(String, String)> {
    let modes = adapter_modes(agent)?;
    let (option, current, values) = adapter_mode_select(options)?;
    let wanted = match mode {
        PermissionMode::Ask => modes.ask,
        PermissionMode::Agent | PermissionMode::ApproveAll => modes.agent,
    };
    (current != wanted && values.iter().any(|value| value == wanted))
        .then(|| (option, wanted.to_owned()))
}

/// Put the adapter of a session that no ACP client started or restarted
/// in the mode behind its permission mode, once its process is up.
pub(crate) async fn apply_when_started(manager: SessionManager, id: ConversationId) {
    let (Some(record), Some(handle)) = (manager.get(&id), manager.handle(&id)) else {
        return;
    };
    let Some(adapter) = handle.get_session_config().await else {
        return;
    };
    let mode = PermissionMode::from_approval_policy(record.approval_policy);
    if let Some((option, value)) = change(&record.agent, mode, &adapter)
        && let Err(error) = handle.set_session_config_option(option, value).await
    {
        tracing::warn!(%error, agent = record.agent, "could not set the adapter's mode");
    }
}

impl HostService {
    pub(super) async fn adapter_options(
        &self,
        id: &ConversationId,
    ) -> Vec<acp::SessionConfigOption> {
        match self.manager.handle(id) {
            Some(handle) => handle.get_session_config().await.unwrap_or_default(),
            None => Vec::new(),
        }
    }

    /// Put a session's adapter in the mode behind its permission mode, once
    /// it has started or restarted. A session in `ask` whose adapter cannot
    /// ask before every edit and command continues in `agent`, and attached
    /// clients are told.
    pub(super) async fn settle_adapter_mode(&self, id: &ConversationId) -> Result<(), AcpError> {
        let _changing = self.mode_changes.lock().await;
        let record = self
            .manager
            .get(id)
            .ok_or_else(|| AcpError::not_found("unknown session"))?;
        let Some(handle) = self.manager.handle(id) else {
            return Ok(());
        };
        // A session whose process is gone is settled when it restarts.
        let Some(adapter) = handle.get_session_config().await else {
            return Ok(());
        };
        let mode = PermissionMode::from_approval_policy(record.approval_policy);
        let entered = if mode == PermissionMode::Ask && !offers_ask(&record.agent, &adapter) {
            Err(anyhow::anyhow!(
                "the adapter has no mode that asks every time"
            ))
        } else if let Some((option, value)) = change(&record.agent, mode, &adapter) {
            handle
                .set_session_config_option(option, value)
                .await
                .map(drop)
        } else {
            Ok(())
        };
        match entered {
            Ok(()) => Ok(()),
            Err(error) if mode == PermissionMode::Ask => {
                tracing::warn!(%error, agent = record.agent, "the session continues in agent mode");
                let agent = PermissionMode::Agent;
                if let Some((option, value)) = change(&record.agent, agent, &adapter)
                    && let Err(error) = handle.set_session_config_option(option, value).await
                {
                    tracing::warn!(%error, "could not set the adapter's mode");
                }
                self.record_mode(id, agent).await
            }
            Err(error) => {
                tracing::warn!(%error, agent = record.agent, "could not set the adapter's mode");
                Ok(())
            }
        }
    }

    /// Switch the session to `mode` as a client asked: the adapter's mode
    /// first, then the host's. `ask` fails when the adapter cannot ask
    /// every time, and a refused adapter mode changes nothing.
    pub(super) async fn change_mode(
        &self,
        id: &ConversationId,
        mode: PermissionMode,
    ) -> Result<(), AcpError> {
        let _changing = self.mode_changes.lock().await;
        let record = self
            .manager
            .get(id)
            .ok_or_else(|| AcpError::not_found("unknown session"))?;
        if let Some(handle) = self.manager.handle(id) {
            let adapter = handle
                .get_session_config()
                .await
                .ok_or_else(|| AcpError::invalid_params("the session is not running"))?;
            if mode == PermissionMode::Ask && !offers_ask(&record.agent, &adapter) {
                return Err(AcpError::invalid_params(format!(
                    "{} has no mode that asks before every edit and command",
                    record.agent
                )));
            }
            if let Some((option, value)) = change(&record.agent, mode, &adapter) {
                handle
                    .set_session_config_option(option, value)
                    .await
                    .map_err(|error| AcpError::invalid_params(format!("{error:#}")))?;
            }
        }
        self.record_mode(id, mode).await
    }

    /// Make `mode` the session's permission mode, remember it, and send
    /// attached clients the new mode and options.
    async fn record_mode(&self, id: &ConversationId, mode: PermissionMode) -> Result<(), AcpError> {
        self.manager
            .set_approval_policy(id, mode.approval_policy())
            .await?;
        self.settings
            .update(id, |settings| settings.mode = Some(mode.id().to_owned()));
        let session = id.to_string();
        self.acp.broadcast(
            id,
            super::config::update(
                &session,
                json!({"sessionUpdate":"current_mode_update","currentModeId":mode.id()}),
            ),
        );
        let (options, _) = self.session_configuration(id).await?;
        self.acp.broadcast(
            id,
            super::config::update(
                &session,
                json!({"sessionUpdate":"config_option_update","configOptions":options}),
            ),
        );
        Ok(())
    }
}
