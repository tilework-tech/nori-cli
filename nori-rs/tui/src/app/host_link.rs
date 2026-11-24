//! The App's connection to the host it owns: a controller client over the
//! in-process pipe, and the host's stop at the end of the run.

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::OwnedHost;
use crate::host_client::ConnectionEvent;
use crate::host_client::HostCall;
use crate::host_client::HostEvent;
use crate::host_client::HostSessionClient;
use crate::owned_host::StopHost;

/// What a call to the owned host was for.
pub(crate) enum HostTag {
    Initialize,
}

pub(crate) struct HostLink {
    client: HostSessionClient<HostTag>,
    events: mpsc::UnboundedReceiver<ConnectionEvent>,
    driver: JoinHandle<()>,
    stop: StopHost,
}

impl HostLink {
    /// Connects to `host`; the returned manager is the host's own.
    pub fn start(host: OwnedHost) -> (nori_harness::session_manager::SessionManager, Self) {
        let OwnedHost {
            manager,
            to_host,
            from_host,
            stop,
        } = host;
        let (requests, request_rx) = mpsc::unbounded_channel();
        let (event_tx, events) = mpsc::unbounded_channel();
        let driver = tokio::spawn(crate::host_client::drive_pipe(
            to_host, from_host, request_rx, event_tx,
        ));
        (
            manager,
            Self {
                client: HostSessionClient::new(requests),
                events,
                driver,
                stop,
            },
        )
    }

    /// The host only sends what this pipe attached; sessions the App runs
    /// through the manager are not attached here, so the pipe stays quiet
    /// apart from replies.
    pub async fn next(&mut self) -> Option<ConnectionEvent> {
        self.events.recv().await
    }

    /// An error means the host can no longer serve this terminal.
    pub fn handle(&mut self, event: ConnectionEvent) -> anyhow::Result<()> {
        match self.client.incoming(event) {
            HostEvent::Connected => self
                .client
                .request(HostCall::Initialize, HostTag::Initialize)
                .map_err(|_| anyhow::anyhow!("The host this terminal owns stopped.")),
            HostEvent::Reply {
                tag: HostTag::Initialize,
                result: Err(error),
            } => anyhow::bail!(
                "The host this terminal owns refused to start: {}",
                error.message.unwrap_or_default()
            ),
            HostEvent::Stopped(reason) => anyhow::bail!(reason),
            HostEvent::Reply {
                tag: HostTag::Initialize,
                result: Ok(_),
            }
            | HostEvent::Disconnected { .. }
            | HostEvent::Session(_)
            | HostEvent::RoleChanged(_)
            | HostEvent::Ignored => Ok(()),
        }
    }

    /// Ends every session the host owns, then the pipe.
    pub async fn stop(self) -> anyhow::Result<()> {
        let Self { driver, stop, .. } = self;
        let stopped = stop().await;
        driver.abort();
        stopped
    }
}
