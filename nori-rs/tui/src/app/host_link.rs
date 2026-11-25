//! The App's connection to the host it owns: a controller client over the
//! in-process pipe, and the host's stop at the end of the run.

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::OwnedHost;
use crate::host_client::ConnectionEvent;
use crate::host_client::HostCall;
use crate::host_client::HostEvent;
use crate::host_client::HostSessionClient;
use nori_host_runtime::StopHost;

/// What a call to the owned host was for.
pub(crate) enum HostTag {
    Initialize,
    Operation(crate::app_event_sender::HostReply),
}

pub(crate) struct HostLink {
    client: HostSessionClient<HostTag>,
    events: mpsc::UnboundedReceiver<ConnectionEvent>,
    driver: JoinHandle<()>,
    stop: StopHost,
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
                stop,
            },
        )
    }

    pub fn initialize(&mut self) -> anyhow::Result<()> {
        self.client
            .request(HostCall::Initialize, HostTag::Initialize)
            .map_err(|_| anyhow::anyhow!("The host this terminal owns stopped."))
    }

    pub fn request(&mut self, call: HostCall, reply: crate::app_event_sender::HostReply) {
        if let Err(HostTag::Operation(reply)) = self.client.request(call, HostTag::Operation(reply))
        {
            reply.deliver(Err(anyhow::anyhow!("The host this terminal owns stopped.")));
        }
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
            HostEvent::Connected => Ok(()),
            HostEvent::Reply {
                tag: HostTag::Operation(feedback),
                result,
            } => {
                feedback.deliver(result.map_err(anyhow::Error::new));
                Ok(())
            }
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
