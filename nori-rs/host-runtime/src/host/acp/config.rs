//! Session configuration on the ACP face: the host-owned `agent` and `mode`
//! options around the adapter's own options, permission modes as ACP session
//! modes, and the settings a session keeps across resume.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::PoisonError;

use nori_harness::ConversationId;
use nori_harness::permission_mode::PermissionMode;
use nori_protocol::AcpEvent;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

use super::AcpError;
use super::adapter_modes;
use crate::host::service::HostService;

pub(super) const AGENT_OPTION: &str = "agent";
pub(super) const MODE_OPTION: &str = "mode";
const AGENT_CATEGORY: &str = "_nori_agent";

/// What a client chose for a session, reapplied when it resumes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub(super) struct SessionSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Adapter option values by option ID, including the model.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub options: BTreeMap<String, String>,
    /// The ACP `mcpServers` the client started the session with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Value>,
}

/// Session settings kept in `$NORI_HOME/host/session-settings.json`, so they
/// survive a host restart.
pub(crate) struct SettingsStore {
    path: PathBuf,
    sessions: Mutex<HashMap<String, SessionSettings>>,
}

impl SettingsStore {
    pub(crate) fn open(nori_home: &Path) -> Self {
        let path = nori_home.join("host/session-settings.json");
        let sessions = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            sessions: Mutex::new(sessions),
        }
    }

    pub(super) fn get(&self, id: &ConversationId) -> SessionSettings {
        self.lock()
            .get(&id.to_string())
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn update(&self, id: &ConversationId, change: impl FnOnce(&mut SessionSettings)) {
        let mut sessions = self.lock();
        change(sessions.entry(id.to_string()).or_default());
        self.save(&sessions);
    }

    pub(super) fn remove(&self, id: &ConversationId) {
        let mut sessions = self.lock();
        if sessions.remove(&id.to_string()).is_some() {
            self.save(&sessions);
        }
    }

    /// Write the file while the caller holds the lock, so concurrent
    /// changes are saved in order. The file is private to the user.
    fn save(&self, sessions: &HashMap<String, SessionSettings>) {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let written = serde_json::to_vec_pretty(sessions)
            .map_err(std::io::Error::other)
            .and_then(|bytes| {
                let temporary = self.path.with_extension("json.tmp");
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&temporary)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                std::fs::rename(&temporary, &self.path)
            });
        if let Err(error) = written {
            tracing::warn!(%error, "could not save session settings");
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SessionSettings>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn select(id: &str, name: &str, category: &str, current: &str, choices: Vec<Value>) -> Value {
    json!({
        "id": id,
        "name": name,
        "category": category,
        "type": "select",
        "currentValue": current,
        "options": choices,
    })
}

/// The agents this host can start, each saying whether it can start now.
pub(super) fn agent_option(current: &str) -> Value {
    let choices = nori_harness::registry::list_available_agents()
        .into_iter()
        .map(|agent| {
            // The adapter can start when its launch command resolves here.
            let description = match nori_harness::registry::get_agent_config(&agent.agent_name) {
                Ok(config) if which::which(&config.command).is_ok() => "Installed".to_owned(),
                Ok(config) if !config.install_hint.is_empty() => {
                    format!("Adapter not installed. {}", config.install_hint)
                }
                Ok(_) => "Adapter not installed.".to_owned(),
                Err(error) => format!("Adapter not installed. {error:#}"),
            };
            json!({"value": agent.agent_name, "name": agent.display_name, "description": description})
        })
        .collect();
    select(AGENT_OPTION, "Agent", AGENT_CATEGORY, current, choices)
}

/// The permission modes a session can choose: `ask` only when its adapter
/// can ask before every edit and command, or the session is already in it.
fn offered(mode: PermissionMode, can_ask: bool) -> impl Iterator<Item = PermissionMode> {
    PermissionMode::ALL
        .into_iter()
        .filter(move |offered| can_ask || *offered != PermissionMode::Ask || mode == *offered)
}

fn mode_option(mode: PermissionMode, can_ask: bool) -> Value {
    let choices = offered(mode, can_ask)
        .map(|mode| json!({"value": mode.id(), "name": mode.name(), "description": mode.description()}))
        .collect();
    select(MODE_OPTION, "Mode", MODE_OPTION, mode.id(), choices)
}

pub(super) fn modes(mode: PermissionMode, can_ask: bool) -> Value {
    json!({
        "currentModeId": mode.id(),
        "availableModes": offered(mode, can_ask)
            .map(|mode| json!({"id": mode.id(), "name": mode.name(), "description": mode.description()}))
            .collect::<Vec<_>>(),
    })
}

/// The host's options around the adapter's. Adapter options that would
/// shadow the host-owned agent or permission mode are left out.
pub(super) fn compose(
    agent: &str,
    mode: PermissionMode,
    adapter: &[acp::SessionConfigOption],
) -> Vec<Value> {
    let can_ask = adapter_modes::offers_ask(agent, adapter);
    std::iter::once(agent_option(agent))
        .chain(
            adapter
                .iter()
                .filter_map(|option| serde_json::to_value(option).ok())
                .filter(|option| {
                    option["id"] != AGENT_OPTION
                        && option["id"] != MODE_OPTION
                        && option["category"] != MODE_OPTION
                }),
        )
        .chain(std::iter::once(mode_option(mode, can_ask)))
        .collect()
}

/// A host-originated `session/update`.
pub(super) fn update(session: &str, update: Value) -> Value {
    json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,"update":update}})
}

/// The adapter's own options carried by a session event, when it changes
/// them; `Some` for the adapter's `config_option_update`.
pub(super) fn adapter_options_update(
    event: &SessionEvent,
) -> Option<Vec<acp::SessionConfigOption>> {
    let SessionEvent::Acp(AcpEvent::Notification(acp::AgentNotification::SessionNotification(
        notification,
    ))) = event
    else {
        return None;
    };
    match &notification.update {
        acp::SessionUpdate::ConfigOptionUpdate(update) => Some(update.config_options.clone()),
        _ => None,
    }
}

/// Whether a session event is the adapter's own mode change, which the host's
/// permission modes replace.
pub(super) fn is_adapter_mode_update(event: &SessionEvent) -> bool {
    matches!(
        event,
        SessionEvent::Acp(AcpEvent::Notification(acp::AgentNotification::SessionNotification(
            notification
        ))) if matches!(notification.update, acp::SessionUpdate::CurrentModeUpdate(_))
    )
}

impl HostService {
    fn mode_of(&self, id: &ConversationId) -> Result<PermissionMode, AcpError> {
        let record = self
            .manager
            .get(id)
            .ok_or_else(|| AcpError::not_found("unknown session"))?;
        Ok(PermissionMode::from_approval_policy(record.approval_policy))
    }

    /// The session's complete options and modes, as a load, resume or set
    /// result.
    pub(super) async fn session_configuration(
        &self,
        id: &ConversationId,
    ) -> Result<(Vec<Value>, Value), AcpError> {
        let record = self
            .manager
            .get(id)
            .ok_or_else(|| AcpError::not_found("unknown session"))?;
        let mode = PermissionMode::from_approval_policy(record.approval_policy);
        let adapter = self.adapter_options(id).await;
        let can_ask = adapter_modes::offers_ask(&record.agent, &adapter);
        Ok((compose(&record.agent, mode, &adapter), modes(mode, can_ask)))
    }

    pub(super) async fn set_mode(
        &self,
        id: &ConversationId,
        mode_id: &str,
    ) -> Result<(), AcpError> {
        let mode = PermissionMode::from_id(mode_id).ok_or_else(|| {
            AcpError::invalid_params(format!(
                "unknown mode {mode_id}; use ask, agent or approve-all"
            ))
        })?;
        self.change_mode(id, mode).await
    }

    pub(super) async fn set_config_option(
        &self,
        id: &ConversationId,
        config_id: &str,
        value: &str,
    ) -> Result<Vec<Value>, AcpError> {
        match config_id {
            MODE_OPTION => self.set_mode(id, value).await?,
            AGENT_OPTION => self.replace_agent(id, value).await?,
            _ => {
                let handle = self
                    .manager
                    .handle(id)
                    .ok_or_else(|| AcpError::invalid_params("the session is not running"))?;
                handle
                    .set_session_config_option(config_id.to_owned(), value.to_owned())
                    .await
                    .map_err(|error| AcpError::invalid_params(format!("{error:#}")))?;
                self.settings.update(id, |settings| {
                    settings
                        .options
                        .insert(config_id.to_owned(), value.to_owned());
                });
                let (options, _) = self.session_configuration(id).await?;
                self.acp.broadcast(
                    id,
                    update(
                        &id.to_string(),
                        json!({"sessionUpdate":"config_option_update","configOptions":options}),
                    ),
                );
            }
        }
        let (options, _) = self.session_configuration(id).await?;
        Ok(options)
    }

    /// Start a different agent for a session that has not been prompted.
    async fn replace_agent(&self, id: &ConversationId, agent: &str) -> Result<(), AcpError> {
        let record = self
            .manager
            .get(id)
            .ok_or_else(|| AcpError::not_found("unknown session"))?;
        let known = nori_harness::registry::list_available_agents()
            .iter()
            .any(|available| available.agent_name == agent);
        if !known {
            return Err(AcpError::invalid_params(format!("unknown agent {agent}")));
        }
        if self.has_prompted(id) {
            return Err(AcpError::invalid_params(
                "the agent can only change before the first prompt",
            ));
        }
        let mode = self.mode_of(id)?;
        let mut config = self.config_in(record.cwd).await?;
        config.active_agent = agent.to_owned();
        config.approval_policy = mode.approval_policy();
        self.restore_mcp_servers(id, &mut config);
        let _restart = self.restarts.lock().await;
        // Attached connections keep following the conversation ID, so they
        // see the new agent's updates without attaching again.
        self.manager
            .replace_agent(id, Self::prepare(config))
            .await?;
        self.settings
            .update(id, |settings| settings.options.clear());
        self.settle_adapter_mode(id).await
    }

    /// Whether any prompt reached the session, judged from its journal.
    fn has_prompted(&self, id: &ConversationId) -> bool {
        let cursors = HashMap::from([(id.to_string(), 0)]);
        match self.manager.subscribe_from(Some(*id), &cursors, 1) {
            Ok(subscription) => subscription.replay.iter().any(|envelope| {
                matches!(
                    &envelope.event,
                    SessionEvent::Acp(AcpEvent::Notification(
                        acp::AgentNotification::SessionNotification(notification)
                    )) if matches!(notification.update, acp::SessionUpdate::UserMessageChunk(_))
                )
            }),
            // An evicted start means the session has been busy for a while.
            Err(_) => true,
        }
    }

    /// Settings to restore when `id` restarts: the mode as its approval
    /// policy and the model as the adapter's starting model.
    pub(crate) fn restore_settings(
        &self,
        id: &ConversationId,
        config: &mut nori_config::NoriConfig,
    ) {
        self.restore_mcp_servers(id, config);
        let settings = self.settings.get(id);
        if let Some(mode) = settings.mode.as_deref().and_then(PermissionMode::from_id) {
            config.approval_policy = mode.approval_policy();
        }
        if let Some(model) = settings.options.get("model") {
            let slug = nori_harness::resolve_agent_slug(&config.active_agent);
            config.default_models.insert(slug.clone(), model.clone());
            config
                .default_model_override_keys
                .retain(|key| key != &slug);
            config.default_model_override_keys.insert(0, slug);
        }
    }

    /// Give a restarted session the MCP servers its client started it with.
    fn restore_mcp_servers(&self, id: &ConversationId, config: &mut nori_config::NoriConfig) {
        let Some(servers) = self.settings.get(id).mcp_servers else {
            return;
        };
        let servers: Vec<acp::McpServer> = serde_json::from_value(servers).unwrap_or_default();
        for server in &servers {
            match super::session::mcp_server(server) {
                Ok((name, server)) => {
                    config.mcp_servers.insert(name, server);
                }
                Err(error) => {
                    tracing::warn!(message = error.message, "could not restore an MCP server");
                }
            }
        }
    }

    /// Apply a restarted session's other remembered adapter options.
    pub(super) async fn reapply_options(&self, id: &ConversationId) {
        let settings = self.settings.get(id);
        let Some(handle) = self.manager.handle(id) else {
            return;
        };
        for (option, value) in settings
            .options
            .iter()
            .filter(|(option, _)| *option != "model")
        {
            if let Err(error) = handle
                .set_session_config_option(option.clone(), value.clone())
                .await
            {
                tracing::warn!(%error, option, "could not restore a session option");
            }
        }
    }
}
