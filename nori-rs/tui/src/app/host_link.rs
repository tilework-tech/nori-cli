//! The App's connection to its host: either the host this terminal owns,
//! over the in-process pipe, or a running host it joined over the network.
//!
//! An owned host always makes this terminal its controller and cannot
//! reconnect, so losing it ends the run. A joined host assigns a role,
//! may drop and restore the connection, and may stop for good; none of
//! that ends the run. The App learns about it through [`HostUpdate`]s and
//! reads the current standing from [`HostLink::connection`].

use std::time::Duration;

use nori_harness::remote_agent::ManagedClient;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::OwnedHost;
use crate::app_event_sender::HostReply;
use crate::host_client::ConnectionEvent;
use crate::host_client::HostCall;
use crate::host_client::HostConnection;
use crate::host_client::HostEndpoint;
use crate::host_client::HostEvent;
use crate::host_client::HostFeatures;
use crate::host_client::HostSessionClient;
use nori_host_runtime::StopHost;
use nori_protocol::ControlRole;

/// A joined host drops a connection that is silent for ten seconds; the
/// session list doubles as the heartbeat.
const LIST_EVERY: Duration = Duration::from_secs(1);

const OWNED_STOPPED: &str = "The host this terminal owns stopped.";
const NOT_CONNECTED: &str = "Reconnecting to the host. Try again once it is back.";
/// The transport cannot tell whether the host applied a call it never answered.
const UNANSWERED: &str =
    "The connection to the host dropped before it answered. The request may have reached the host.";

/// What a call to the host was for.
pub(crate) enum HostTag {
    Initialize,
    TakeControl,
    List,
    Operation(HostReply),
}

/// Something about the host the App shows or acts on.
#[derive(Debug)]
pub(crate) enum HostUpdate {
    /// The host accepted this terminal. `reconnected` is false the first
    /// time and true after every restored connection.
    Ready {
        reconnected: bool,
    },
    RoleChanged(ControlRole),
    /// The connection dropped; it is being re-established.
    Disconnected(String),
    /// The host can no longer serve this terminal.
    Stopped(String),
    /// The host's `_nori/session/list` result.
    Sessions(Value),
    /// One `_nori/session/event` notification's parameters.
    Session(Value),
}

enum Ownership {
    Owned(StopHost),
    Joined { list_timer: tokio::time::Interval },
}

pub(crate) struct HostLink {
    client: HostSessionClient<HostTag>,
    events: mpsc::UnboundedReceiver<ConnectionEvent>,
    driver: JoinHandle<()>,
    ownership: Ownership,
    connection: HostConnection,
    initialized: bool,
}

impl HostLink {
    /// Connects to `host`; the returned manager is the host's own.
    pub fn start(
        host: OwnedHost,
    ) -> (
        nori_harness::session_manager::SessionManager,
        nori_host_runtime::ForegroundConfig,
        Self,
    ) {
        let OwnedHost {
            manager,
            to_host,
            from_host,
            stop,
            foreground_config,
        } = host;
        let (requests, request_rx) = mpsc::unbounded_channel();
        let (event_tx, events) = mpsc::unbounded_channel();
        let driver = tokio::spawn(crate::host_client::drive_pipe(
            to_host, from_host, request_rx, event_tx,
        ));
        (
            manager,
            foreground_config,
            Self {
                client: HostSessionClient::new(requests),
                events,
                driver,
                ownership: Ownership::Owned(stop),
                connection: HostConnection::owned(),
                initialized: false,
            },
        )
    }

    /// Joins the running host `initial` is connected to. The link initializes
    /// itself on every connection and reports [`HostUpdate::Ready`].
    pub fn join(endpoint: HostEndpoint, initial: ManagedClient) -> Self {
        // Only a host discovered on this machine refreshes its token.
        let remote = endpoint.refresh_local_token.is_none();
        let (requests, request_rx) = mpsc::unbounded_channel();
        let (event_tx, events) = mpsc::unbounded_channel();
        let driver = tokio::spawn(crate::host_client::drive(
            endpoint, initial, request_rx, event_tx,
        ));
        let mut list_timer = tokio::time::interval(LIST_EVERY);
        list_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Self {
            client: HostSessionClient::new(requests),
            events,
            driver,
            ownership: Ownership::Joined { list_timer },
            connection: HostConnection {
                role: ControlRole::Watcher,
                connected: false,
                stopped: None,
                features: HostFeatures {
                    remote,
                    ..HostFeatures::default()
                },
            },
            initialized: false,
        }
    }

    pub fn connection(&self) -> HostConnection {
        self.connection.clone()
    }

    /// Initializes an owned host before anything else is sent to it.
    pub fn initialize(&mut self) -> anyhow::Result<()> {
        self.client
            .request(HostCall::Initialize, HostTag::Initialize)
            .map_err(|_| anyhow::anyhow!(OWNED_STOPPED))
    }

    pub fn request(&mut self, call: HostCall, reply: HostReply) {
        if let Some(reason) = self.refusal() {
            reply.deliver(Err(anyhow::anyhow!(reason)));
            return;
        }
        if let Err(HostTag::Operation(reply)) = self.client.request(call, HostTag::Operation(reply))
        {
            reply.deliver(Err(anyhow::anyhow!(self.stopped_reason())));
        }
    }

    /// Asks the host to make this terminal its controller. The answer
    /// arrives as [`HostUpdate::RoleChanged`].
    pub fn take_control(&mut self) {
        if self.refusal().is_none() {
            let _ = self
                .client
                .request(HostCall::TakeControl, HostTag::TakeControl);
        }
    }

    /// Why a call cannot be sent now. The connection discards calls queued
    /// while it is down, so they are refused here instead.
    fn refusal(&self) -> Option<String> {
        if let Some(reason) = &self.connection.stopped {
            return Some(reason.clone());
        }
        (!self.connection.connected).then(|| NOT_CONNECTED.to_string())
    }

    fn stopped_reason(&self) -> String {
        match (&self.connection.stopped, &self.ownership) {
            (Some(reason), _) => reason.clone(),
            (None, Ownership::Owned(_)) => OWNED_STOPPED.to_string(),
            (None, Ownership::Joined { .. }) => NOT_CONNECTED.to_string(),
        }
    }

    /// The next thing the connection delivered. A joined link also lists the
    /// host's sessions every second while connected.
    pub async fn next(&mut self) -> Option<ConnectionEvent> {
        loop {
            match &mut self.ownership {
                Ownership::Owned(_) => return self.events.recv().await,
                Ownership::Joined { list_timer } => {
                    tokio::select! {
                        event = self.events.recv() => return event,
                        _ = list_timer.tick() => {
                            if self.connection.connected
                                && self.connection.stopped.is_none()
                                && !self.client.has_pending(|tag| matches!(tag, HostTag::List))
                            {
                                let _ = self.client.request(HostCall::List, HostTag::List);
                            }
                        }
                    }
                }
            }
        }
    }

    /// An error means the owned host can no longer serve this terminal. A
    /// joined host never returns one; its failures are updates.
    pub fn handle(&mut self, event: ConnectionEvent) -> anyhow::Result<Option<HostUpdate>> {
        let joined = matches!(self.ownership, Ownership::Joined { .. });
        match self.client.incoming(event) {
            HostEvent::Connected => {
                if joined {
                    let _ = self
                        .client
                        .request(HostCall::Initialize, HostTag::Initialize);
                }
                Ok(None)
            }
            HostEvent::Reply {
                tag: HostTag::Operation(feedback),
                result,
            } => {
                feedback.deliver(result.map_err(|error| {
                    anyhow::anyhow!(
                        error
                            .message
                            .unwrap_or_else(|| "Host request failed.".into())
                    )
                }));
                Ok(None)
            }
            HostEvent::Reply {
                tag: HostTag::Initialize,
                result: Err(error),
            } => {
                let message = error.message.unwrap_or_default();
                if !joined {
                    anyhow::bail!("The host this terminal owns refused to start: {message}");
                }
                Ok(Some(self.stop(format!(
                    "The host refused this terminal: {message}"
                ))))
            }
            HostEvent::Reply {
                tag: HostTag::Initialize,
                result: Ok(mut result),
            } => {
                if !joined {
                    return Ok(None);
                }
                let reconnected = std::mem::replace(&mut self.initialized, true);
                self.connection.connected = true;
                let sessions = &result["capabilities"]["nori_sessions"];
                self.connection.features.session_folders = sessions["session_cwd"] == true;
                self.connection.features.find_paths = sessions["find_paths"] == true;
                self.connection.features.goal = sessions["goal"] == true;
                self.connection.features.context = sessions["context"] == true;
                self.connection.features.compact = sessions["compact"] == true;
                self.connection.features.shell = sessions["shell"] == true;
                if let Ok(control) =
                    serde_json::from_value::<nori_protocol::ControlStatus>(result["control"].take())
                {
                    self.connection.role = control.role;
                }
                Ok(Some(HostUpdate::Ready { reconnected }))
            }
            HostEvent::Reply {
                tag: HostTag::TakeControl,
                result,
            } => Ok(result
                .ok()
                .and_then(|status| {
                    serde_json::from_value::<nori_protocol::ControlStatus>(status).ok()
                })
                .and_then(|status| self.change_role(status.role))),
            HostEvent::Reply {
                tag: HostTag::List,
                result,
            } => Ok(result.ok().map(HostUpdate::Sessions)),
            HostEvent::Session(params) if joined => Ok(Some(HostUpdate::Session(params))),
            HostEvent::Session(_) | HostEvent::Ignored => Ok(None),
            HostEvent::RoleChanged(role) if joined => {
                // The client names a role it cannot read a watcher.
                let role = serde_json::from_value(serde_json::Value::String(role))
                    .unwrap_or(ControlRole::Watcher);
                Ok(self.change_role(role))
            }
            HostEvent::RoleChanged(_) => Ok(None),
            HostEvent::Disconnected { .. } if self.connection.stopped.is_some() => Ok(None),
            HostEvent::Disconnected { reason, unanswered } => {
                self.connection.connected = false;
                for tag in unanswered {
                    if let HostTag::Operation(reply) = tag {
                        reply.deliver(Err(anyhow::anyhow!(UNANSWERED)));
                    }
                }
                Ok(Some(HostUpdate::Disconnected(reason)))
            }
            HostEvent::Stopped(reason) => {
                if !joined {
                    anyhow::bail!(reason);
                }
                Ok(Some(self.stop(reason)))
            }
        }
    }

    fn change_role(&mut self, role: ControlRole) -> Option<HostUpdate> {
        (self.connection.role != role).then(|| {
            self.connection.role = role;
            HostUpdate::RoleChanged(role)
        })
    }

    fn stop(&mut self, reason: String) -> HostUpdate {
        self.client.stop();
        self.driver.abort();
        self.connection.connected = false;
        self.connection.stopped = Some(reason.clone());
        HostUpdate::Stopped(reason)
    }

    /// Ends every session an owned host runs, then the connection. Leaving a
    /// joined host only closes this terminal's connection; its sessions keep
    /// running.
    pub async fn stop_or_leave(self) -> anyhow::Result<()> {
        let Self {
            driver, ownership, ..
        } = self;
        let stopped = match ownership {
            Ownership::Owned(stop) => stop().await,
            Ownership::Joined { .. } => Ok(()),
        };
        driver.abort();
        stopped
    }
}
