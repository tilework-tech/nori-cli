//! Initialized ACP connection ownership before a session directive is chosen.

use std::fmt;

use anyhow::Result;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use tokio::sync::mpsc;

use super::AcpBackendConfig;
use super::enhance_agent_error;
use super::get_agent_config;
use crate::connection::ConnectionEvent;
use crate::connection::acp_connection::AcpConnection;

/// A prepared connection owns no remote session that needs the active-session
/// detach grace. Allow a brief EOF cleanup, then reap it promptly.
const PRE_SESSION_SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_millis(250);

/// Result of inspecting an initialized agent's session-list capability.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionCatalog {
    /// The agent does not advertise ACP `session/list`.
    Unsupported,
    /// The agent advertised `session/list` and returned these rows.
    Listed(Vec<acp::SessionInfo>),
}

/// Unique ownership of an initialized ACP connection that has no active session.
///
/// This value is deliberately not cloneable. Activating a session consumes it;
/// dropping it kills the unused subprocess through [`AcpConnection`]'s drop
/// backstop.
pub struct PreparedAgent {
    connection: Option<AcpConnection>,
    event_rx: Option<mpsc::Receiver<ConnectionEvent>>,
    config: Option<AcpBackendConfig>,
    catalog: SessionCatalog,
    setup_events: Option<Vec<SessionEvent>>,
}

impl fmt::Debug for PreparedAgent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedAgent")
            .field("agent", &self.config.as_ref().map(|config| &config.agent))
            .field("catalog", &self.catalog)
            .finish_non_exhaustive()
    }
}

impl PreparedAgent {
    pub(crate) async fn prepare(
        config: AcpBackendConfig,
    ) -> std::result::Result<Self, AgentPrepareFailure> {
        let mut agent_config = get_agent_config(&config.agent)
            .map_err(|error| AgentPrepareFailure::new(error, Vec::new()))?;
        super::spawn_and_relay::inject_default_model(
            &mut agent_config,
            config.default_model.as_deref(),
        );
        let (initialize_event_tx, mut initialize_event_rx) = mpsc::channel(1);
        let connection_result = AcpConnection::spawn_with_initialize_event_sender(
            &agent_config,
            &config.cwd,
            config.acp_proxy.clone(),
            initialize_event_tx,
        )
        .await;
        let initialize_event = initialize_event_rx.recv().await;
        let initialize_observed = initialize_event.is_some();
        let setup_events = initialize_event
            .into_iter()
            .map(SessionEvent::Acp)
            .collect::<Vec<_>>();
        let mut connection = match connection_result {
            Ok(connection) if initialize_observed => connection,
            Ok(_) => {
                return Err(AgentPrepareFailure::new(
                    anyhow::anyhow!("ACP agent produced no initialize response"),
                    setup_events,
                ));
            }
            Err(error) => {
                return Err(AgentPrepareFailure::new(
                    enhance_agent_error(error, &agent_config),
                    setup_events,
                ));
            }
        };
        let mut event_rx = connection.take_event_receiver();
        let mut inspection_events = Vec::new();

        let catalog_result: Result<SessionCatalog> = async {
            if connection
                .capabilities()
                .session_capabilities
                .list
                .is_some()
            {
                let list_result = {
                    let list = connection.list_sessions(&config.cwd);
                    tokio::pin!(list);
                    loop {
                        tokio::select! {
                            biased;
                            event = event_rx.recv() => {
                                handle_preparation_event(event, &mut inspection_events).await?;
                            }
                            result = &mut list => break result,
                        }
                    }
                };
                while let Ok(event) = event_rx.try_recv() {
                    handle_preparation_event(Some(event), &mut inspection_events).await?;
                }
                Ok(SessionCatalog::Listed(list_result.map_err(|error| {
                    enhance_agent_error(error, &agent_config)
                })?))
            } else {
                Ok(SessionCatalog::Unsupported)
            }
        }
        .await;
        let catalog = match catalog_result {
            Ok(catalog) => catalog,
            Err(error) => {
                let mut events = setup_events;
                events.extend(inspection_events);
                return Err(AgentPrepareFailure::new(error, events));
            }
        };

        Ok(Self {
            connection: Some(connection),
            event_rx: Some(event_rx),
            config: Some(config),
            catalog,
            setup_events: Some(setup_events),
        })
    }

    /// Sessions discovered on this exact initialized connection.
    pub fn catalog(&self) -> &SessionCatalog {
        &self.catalog
    }

    /// Capabilities advertised by this exact initialized connection.
    #[expect(
        clippy::expect_used,
        reason = "PreparedAgent typestate guarantees ownership until consuming activation"
    )]
    pub fn capabilities(&self) -> &acp::AgentCapabilities {
        self.connection
            .as_ref()
            .expect("prepared connection is present until activation")
            .capabilities()
    }

    pub(crate) fn replace_activation_config(
        &mut self,
        config: AcpBackendConfig,
    ) -> anyhow::Result<()> {
        let prepared_config = self
            .config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("prepared agent was already activated"))?;
        anyhow::ensure!(
            prepared_config.agent == config.agent,
            "prepared agent identity changed from {} to {}",
            prepared_config.agent,
            config.agent,
        );
        anyhow::ensure!(
            prepared_config.cwd == config.cwd,
            "prepared agent working directory changed from {} to {}",
            prepared_config.cwd.display(),
            config.cwd.display(),
        );
        anyhow::ensure!(
            prepared_config.acp_proxy == config.acp_proxy,
            "ACP wire-recording configuration changed while the agent was preparing; retry the switch"
        );
        anyhow::ensure!(
            prepared_config.default_model == config.default_model,
            "prepared agent default model changed while the agent was preparing; retry the switch"
        );
        self.config = Some(config);
        Ok(())
    }

    /// Explicitly close an abandoned prepared connection and await process reaping.
    pub async fn shutdown(mut self) {
        if let Some(connection) = self.connection.take() {
            connection
                .shutdown_with_grace(PRE_SESSION_SHUTDOWN_GRACE)
                .await;
        }
    }

    #[expect(
        clippy::expect_used,
        reason = "PreparedAgent is uniquely consumed exactly once at the activation boundary"
    )]
    pub(super) fn into_parts(mut self) -> PreparedAgentParts {
        PreparedAgentParts {
            connection: self
                .connection
                .take()
                .expect("prepared connection can only be activated once"),
            event_rx: self
                .event_rx
                .take()
                .expect("prepared event receiver can only be activated once"),
            config: self
                .config
                .take()
                .expect("prepared config can only be activated once"),
            setup_events: self
                .setup_events
                .take()
                .expect("prepared setup events can only be activated once"),
        }
    }
}

pub(crate) struct AgentPrepareFailure {
    pub(crate) error: anyhow::Error,
    pub(crate) setup_events: Vec<SessionEvent>,
}

impl AgentPrepareFailure {
    fn new(error: anyhow::Error, setup_events: Vec<SessionEvent>) -> Self {
        Self {
            error,
            setup_events,
        }
    }
}

pub(super) struct PreparedAgentParts {
    pub(super) connection: AcpConnection,
    pub(super) event_rx: mpsc::Receiver<ConnectionEvent>,
    pub(super) config: AcpBackendConfig,
    pub(super) setup_events: Vec<SessionEvent>,
}

async fn handle_preparation_event(
    event: Option<ConnectionEvent>,
    inspection_events: &mut Vec<SessionEvent>,
) -> Result<()> {
    match event {
        Some(ConnectionEvent::Acp(event)) => {
            inspection_events.push(SessionEvent::Acp(*event));
            Ok(())
        }
        Some(ConnectionEvent::SessionUpdate(_)) => Ok(()),
        Some(ConnectionEvent::DelegatedRequest(request)) => {
            let _ = request
                .response_tx
                .send(Ok(acp::ClientResponse::RequestPermissionResponse(
                    acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled),
                )));
            Ok(())
        }
        Some(ConnectionEvent::SessionClosed) => {
            anyhow::bail!("ACP session closed while preparing agent")
        }
        Some(ConnectionEvent::ChildExited {
            status,
            stderr_tail,
        }) => anyhow::bail!(
            "ACP agent exited while preparing connection (status: {status:?}): {stderr_tail}"
        ),
        None => anyhow::bail!("ACP connection event stream closed while preparing agent"),
    }
}
