//! An initialized downstream ACP server belongs to the picker connection until
//! session/new consumes it. Its catalog contains the adapter's opaque IDs.

use std::path::Path;
use std::sync::Arc;

use nori_config::NoriConfig;
use nori_harness::backend::SessionContext;
use nori_harness::runtime::AgentPrepareSpec;
use nori_harness::runtime::PreparedAgent;
use nori_harness::runtime::SessionCatalog;
use nori_harness::runtime::SessionResume;
use nori_harness::runtime::SessionStart;
use nori_harness::session_manager::SessionAgent;
use nori_protocol::PrepareAgentParams;
use nori_protocol::PrepareAgentResult;
use nori_protocol::PreparedAgentActivation;
use nori_protocol::PreparedAgentRef;
use nori_protocol::acp::v1 as acp;
use serde_json::Value;
use serde_json::json;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::AcpError;
use super::Connection;
use crate::host::service::HostService;

pub(in crate::host) const KEY: &str = "nori/preparedAgent";
pub(in crate::host) const PREPARE: &str = "_nori/host/prepare_agent";
pub(in crate::host) const DISCARD: &str = "_nori/host/discard_agent";

struct Catalog {
    sessions: Vec<acp::SessionInfo>,
    capabilities: acp::AgentCapabilities,
}

type CatalogReceiver = watch::Receiver<Option<Result<Arc<Catalog>, AcpError>>>;

pub(super) struct Selection {
    id: String,
    config: Arc<NoriConfig>,
    context: Option<SessionContext>,
    catalog: CatalogReceiver,
    /// Dropping the unused sender retires the worker, even during initialize/list.
    activate: oneshot::Sender<oneshot::Sender<PreparedAgent>>,
    worker: JoinHandle<()>,
}

impl Selection {
    fn open(config: NoriConfig, context: Option<SessionContext>) -> Self {
        let config = Arc::new(config);
        let spec = AgentPrepareSpec {
            config: config.clone(),
            session_context: context.clone(),
            initial_context: None,
            cli_version: env!("CARGO_PKG_VERSION").into(),
        };
        let (catalog_tx, catalog) = watch::channel(None);
        let (activate, mut request) = oneshot::channel::<oneshot::Sender<PreparedAgent>>();
        let worker = tokio::spawn(async move {
            let prepared = tokio::select! {
                biased;
                _ = &mut request => return,
                result = tokio::time::timeout(std::time::Duration::from_secs(20), nori_harness::runtime::prepare_agent(spec)) => {
                    result.unwrap_or_else(|_| Err(anyhow::anyhow!("timed out preparing agent after 20s")))
                },
            };
            let prepared = match prepared {
                Ok(agent) => agent,
                Err(error) => {
                    catalog_tx.send_replace(Some(Err(error.into())));
                    return;
                }
            };
            let catalog = Catalog {
                sessions: match prepared.catalog() {
                    SessionCatalog::Listed(rows) => rows.clone(),
                    SessionCatalog::Unsupported => Vec::new(),
                },
                capabilities: prepared.capabilities().clone(),
            };
            catalog_tx.send_replace(Some(Ok(Arc::new(catalog))));
            match request.await {
                Ok(target) => {
                    if let Err(unused) = target.send(prepared) {
                        unused.shutdown().await;
                    }
                }
                Err(_) => prepared.shutdown().await,
            }
        });
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            config,
            context,
            catalog,
            activate,
            worker,
        }
    }

    async fn retire(self) {
        drop(self.activate);
        let _ = self.worker.await;
    }
}

fn expired() -> AcpError {
    AcpError::not_found("the prepared agent is no longer available on this connection")
}

impl Connection {
    fn selection(&self, id: &str) -> Result<&Selection, AcpError> {
        self.selection
            .as_ref()
            .filter(|selection| selection.id == id)
            .ok_or_else(expired)
    }
}

async fn catalog(mut receiver: CatalogReceiver) -> Result<Arc<Catalog>, AcpError> {
    receiver
        .wait_for(Option::is_some)
        .await
        .map_err(|_| expired())?
        .clone()
        .ok_or_else(expired)?
}

impl HostService {
    pub(super) async fn prepare_selection(
        &self,
        client: i64,
        params: Value,
    ) -> Result<Value, AcpError> {
        let PrepareAgentParams { agent, cwd } =
            serde_json::from_value(params).map_err(AcpError::invalid_params)?;
        let base = self
            .acp
            .connection(client)
            .ok()
            .filter(|connection| {
                matches!(
                    connection.origin,
                    nori_harness::remote_agent::HostClientOrigin::Foreground
                )
            })
            .and_then(|_| {
                self.foreground_config
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
            })
            .unwrap_or_else(|| self.config.clone());
        let folder =
            nori_harness::managed_launch::session_folder(cwd.as_deref().unwrap_or(&base.cwd))
                .map_err(AcpError::invalid_params)?;
        let mut config = if folder == base.cwd {
            (*base).clone()
        } else {
            self.config_in(folder.clone()).await?
        };
        let agent =
            nori_harness::resolve_agent_slug(agent.as_deref().unwrap_or(&base.active_agent));
        if !nori_harness::registry::list_available_agents()
            .iter()
            .any(|a| a.agent_name == agent)
        {
            return Err(AcpError::invalid_params(format!("unknown agent {agent}")));
        }
        config.active_agent.clone_from(&agent);
        config.cwd.clone_from(&folder);
        self.apply_host_default_mode(&mut config);
        let previous;
        let result;
        {
            let mut state = self.acp.lock();
            let connection = state.connections.get_mut(&client).ok_or_else(expired)?;
            let selection = Selection::open(config, self.terminal_context.clone());
            result = json!(PrepareAgentResult {
                id: selection.id.clone(),
                cwd: folder,
                agent
            });
            previous = connection.selection.replace(selection);
        }
        if let Some(previous) = previous {
            previous.retire().await;
        }
        Ok(result)
    }

    fn take_selection(&self, client: i64, id: &str) -> Result<Selection, AcpError> {
        let mut state = self.acp.lock();
        let conn = state.connections.get_mut(&client).ok_or_else(expired)?;
        conn.selection(id)?;
        conn.selection.take().ok_or_else(expired)
    }

    pub(super) async fn discard_selection(
        &self,
        client: i64,
        params: Value,
    ) -> Result<Value, AcpError> {
        let PreparedAgentRef { id } =
            serde_json::from_value(params).map_err(AcpError::invalid_params)?;
        self.take_selection(client, &id)?.retire().await;
        Ok(json!({}))
    }

    fn selection_catalog(
        &self,
        client: i64,
        id: &str,
        cwd: Option<&Value>,
    ) -> Result<CatalogReceiver, AcpError> {
        let state = self.acp.lock();
        let selection = state
            .connections
            .get(&client)
            .ok_or_else(expired)?
            .selection(id)?;
        if let Some(cwd) = cwd
            && cwd.as_str().map(Path::new) != Some(selection.config.cwd.as_path())
        {
            return Err(AcpError::invalid_params(
                "cwd must match the prepared folder",
            ));
        }
        Ok(selection.catalog.clone())
    }

    pub(super) async fn list_selection(
        &self,
        client: i64,
        params: &Value,
        selection: Value,
    ) -> Result<Value, AcpError> {
        let PreparedAgentRef { id } =
            serde_json::from_value(selection).map_err(AcpError::invalid_params)?;
        if params.get("cursor").is_some_and(|cursor| !cursor.is_null()) {
            return Err(AcpError::invalid_params(
                "the prepared catalog has no further pages",
            ));
        }
        let catalog = catalog(self.selection_catalog(client, &id, params.get("cwd"))?).await?;
        Ok(
            json!({"sessions":catalog.sessions, "_meta":{(KEY):{"agentCapabilities":catalog.capabilities}}}),
        )
    }

    pub(super) async fn activate_selection(
        &self,
        client: i64,
        params: &Value,
        selection: Value,
    ) -> Result<Value, AcpError> {
        let PreparedAgentActivation {
            id,
            saved_session_id,
        } = serde_json::from_value(selection).map_err(AcpError::invalid_params)?;
        let cwd = params
            .get("cwd")
            .ok_or_else(|| AcpError::invalid_params("cwd is required"))?;
        let catalog = catalog(self.selection_catalog(client, &id, Some(cwd))?).await?;
        let start = match saved_session_id {
            Some(saved) => {
                if !catalog.capabilities.load_session
                    && catalog.capabilities.session_capabilities.resume.is_none()
                {
                    return Err(AcpError::invalid_params(
                        "the selected agent cannot restore saved sessions",
                    ));
                }
                if !catalog
                    .sessions
                    .iter()
                    .any(|row| row.session_id.to_string() == saved)
                {
                    return Err(AcpError::invalid_params("unknown saved adapter session"));
                }
                SessionStart::Resume(SessionResume {
                    acp_session_id: Some(saved),
                    transcript: None,
                })
            }
            None => SessionStart::New,
        };
        if params["_meta"].get("nori/agent").is_some() {
            return Err(AcpError::invalid_params(
                "the preparation already selects an agent",
            ));
        }
        let servers = super::session::mcp_servers(params)?;
        let additions = servers
            .iter()
            .map(super::session::mcp_server)
            .collect::<Result<Vec<_>, _>>()?;
        let selection = self.take_selection(client, &id)?;
        let (target, ready) = oneshot::channel();
        selection.activate.send(target).map_err(|_| expired())?;
        let mut agent = ready.await.map_err(|_| expired())?;
        if !additions.is_empty() {
            let mut config = selection.config;
            Arc::make_mut(&mut config).mcp_servers.extend(additions);
            nori_harness::runtime::refresh_prepared_agent(
                &mut agent,
                AgentPrepareSpec {
                    config,
                    session_context: selection.context,
                    initial_context: None,
                    cli_version: env!("CARGO_PKG_VERSION").into(),
                },
            )?;
        }
        self.start_and_attach(client, SessionAgent::Prepared(agent), start, &servers)
            .await
    }
}
