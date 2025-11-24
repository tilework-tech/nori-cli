//! Client trait implementation for handling agent callbacks

use agent_client_protocol::{
    Client, CreateTerminalRequest, CreateTerminalResponse, Error, KillTerminalCommandRequest,
    KillTerminalCommandResponse, ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest,
    ReleaseTerminalResponse, RequestPermissionRequest, RequestPermissionResponse,
    SessionNotification, TerminalOutputRequest, TerminalOutputResponse, WaitForTerminalExitRequest,
    WaitForTerminalExitResponse, WriteTextFileRequest, WriteTextFileResponse,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc;

/// Event type for communication between Client handler and consumers
#[derive(Debug, Clone)]
pub enum ClientEvent {
    /// Session update notification received
    SessionUpdate(SessionNotification),
    /// Permission request needs user interaction
    PermissionRequest(RequestPermissionRequest),
}

/// Client implementation that handles callbacks from the agent
pub struct AcpClientHandler {
    /// Channel for sending events to consumers
    event_tx: Arc<Mutex<mpsc::Sender<ClientEvent>>>,
}

impl AcpClientHandler {
    /// Create a new client handler with an event channel
    pub fn new(event_tx: mpsc::Sender<ClientEvent>) -> Self {
        Self {
            event_tx: Arc::new(Mutex::new(event_tx)),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Client for AcpClientHandler {
    async fn request_permission(
        &self,
        args: RequestPermissionRequest,
    ) -> agent_client_protocol::Result<RequestPermissionResponse> {
        // Forward permission request to event channel
        self.event_tx
            .lock()
            .await
            .send(ClientEvent::PermissionRequest(args.clone()))
            .await
            .map_err(|_| Error::internal_error())?;

        // For now, auto-cancel all permission requests
        // TODO: Implement proper permission handling
        Ok(RequestPermissionResponse {
            outcome: agent_client_protocol::RequestPermissionOutcome::Cancelled,
            meta: None,
        })
    }

    async fn session_notification(
        &self,
        args: SessionNotification,
    ) -> agent_client_protocol::Result<()> {
        // Forward session update to event channel
        self.event_tx
            .lock()
            .await
            .send(ClientEvent::SessionUpdate(args))
            .await
            .map_err(|_| Error::internal_error())?;

        Ok(())
    }

    async fn write_text_file(
        &self,
        _args: WriteTextFileRequest,
    ) -> agent_client_protocol::Result<WriteTextFileResponse> {
        Err(Error::method_not_found())
    }

    async fn read_text_file(
        &self,
        _args: ReadTextFileRequest,
    ) -> agent_client_protocol::Result<ReadTextFileResponse> {
        Err(Error::method_not_found())
    }

    async fn create_terminal(
        &self,
        _args: CreateTerminalRequest,
    ) -> agent_client_protocol::Result<CreateTerminalResponse> {
        Err(Error::method_not_found())
    }

    async fn terminal_output(
        &self,
        _args: TerminalOutputRequest,
    ) -> agent_client_protocol::Result<TerminalOutputResponse> {
        Err(Error::method_not_found())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: WaitForTerminalExitRequest,
    ) -> agent_client_protocol::Result<WaitForTerminalExitResponse> {
        Err(Error::method_not_found())
    }

    async fn kill_terminal_command(
        &self,
        _args: KillTerminalCommandRequest,
    ) -> agent_client_protocol::Result<KillTerminalCommandResponse> {
        Err(Error::method_not_found())
    }

    async fn release_terminal(
        &self,
        _args: ReleaseTerminalRequest,
    ) -> agent_client_protocol::Result<ReleaseTerminalResponse> {
        Err(Error::method_not_found())
    }
}
