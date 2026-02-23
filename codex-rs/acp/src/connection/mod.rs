//! ACP Connection management
//!
//! Handles spawning and communicating with ACP agent subprocesses.
//!
//! The ACP protocol library uses `LocalBoxFuture` which requires `!Send` futures.
//! To integrate with codex-core's multi-threaded tokio runtime, we spawn a dedicated
//! single-threaded runtime for each ACP connection and communicate via channels.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::thread;
use std::time::Duration;

use agent_client_protocol as acp;
use anyhow::Context;
use anyhow::Result;
use codex_protocol::approvals::ApplyPatchApprovalRequestEvent;
use codex_protocol::approvals::ExecApprovalRequestEvent;
use codex_protocol::protocol::ReviewDecision;
use futures::AsyncBufReadExt;
use futures::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;
use tracing::debug;
use tracing::warn;

use crate::registry::AcpAgentConfig;
use crate::translator;

mod client_delegate;
mod public_api;
mod worker;

#[cfg(test)]
mod tests;

/// The type of approval event to send to the UI.
///
/// This enum allows us to use the more appropriate approval UI for different
/// operation types - exec approval for shell commands, patch approval for
/// file edits/writes/deletes.
#[derive(Debug)]
pub enum ApprovalEventType {
    /// Exec approval for shell commands and other operations
    Exec(ExecApprovalRequestEvent),
    /// Patch approval for file edit/write/delete operations
    Patch(ApplyPatchApprovalRequestEvent),
}

impl ApprovalEventType {
    /// Get the call_id from the event
    pub fn call_id(&self) -> &str {
        match self {
            ApprovalEventType::Exec(e) => &e.call_id,
            ApprovalEventType::Patch(e) => &e.call_id,
        }
    }
}

/// An approval request sent from the ACP layer to the UI layer.
///
/// When an ACP agent requests permission to perform an operation,
/// this struct is sent to the UI layer which should display the request
/// to the user and return their decision via the response channel.
#[derive(Debug)]
pub struct ApprovalRequest {
    /// The translated Codex approval event (either exec or patch)
    pub event: ApprovalEventType,
    /// The original ACP permission options for translating the response
    pub options: Vec<acp::PermissionOption>,
    /// Channel to send the user's decision back
    pub response_tx: oneshot::Sender<ReviewDecision>,
}

/// Minimum supported ACP protocol version
const MINIMUM_SUPPORTED_VERSION: acp::ProtocolVersion = acp::ProtocolVersion::V1;

/// Model state captured from the ACP session.
///
/// This is populated when a session is created (from `NewSessionResponse`)
/// and can be updated when the model is changed.
#[derive(Debug, Clone, Default)]
pub struct AcpModelState {
    /// The ID of the currently active model
    pub current_model_id: Option<acp::ModelId>,
    /// List of available models from the agent
    pub available_models: Vec<acp::ModelInfo>,
}

impl AcpModelState {
    /// Create a new empty model state
    pub fn new() -> Self {
        Self::default()
    }

    /// Update from an ACP SessionModelState
    #[cfg(feature = "unstable")]
    pub fn from_session_model_state(state: &acp::SessionModelState) -> Self {
        Self {
            current_model_id: Some(state.current_model_id.clone()),
            available_models: state.available_models.clone(),
        }
    }
}

/// Commands sent from the main thread to the ACP worker thread.
enum AcpCommand {
    CreateSession {
        cwd: PathBuf,
        response_tx: oneshot::Sender<Result<acp::SessionId>>,
    },
    Prompt {
        session_id: acp::SessionId,
        prompt: Vec<acp::ContentBlock>,
        update_tx: mpsc::Sender<acp::SessionUpdate>,
        response_tx: oneshot::Sender<Result<acp::StopReason>>,
    },
    LoadSession {
        session_id: String,
        cwd: PathBuf,
        update_tx: mpsc::Sender<acp::SessionUpdate>,
        response_tx: oneshot::Sender<Result<acp::SessionId>>,
    },
    Cancel {
        session_id: acp::SessionId,
        response_tx: oneshot::Sender<Result<()>>,
    },
    #[cfg(feature = "unstable")]
    SetModel {
        session_id: acp::SessionId,
        model_id: acp::ModelId,
        response_tx: oneshot::Sender<Result<()>>,
    },
}

/// Timeout for waiting for worker thread cleanup during Drop.
/// This should be long enough for the child process kill to complete,
/// but not so long that it blocks shutdown indefinitely.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// A thread-safe wrapper around an ACP agent subprocess.
///
/// This spawns a dedicated single-threaded tokio runtime on a background thread
/// to handle the ACP protocol (which requires `!Send` futures), and communicates
/// with the main runtime via channels.
///
/// When dropped, this struct ensures the worker thread completes its cleanup
/// (including killing the child process) before returning. This prevents
/// orphaned agent subprocesses when the TUI exits.
pub struct AcpConnection {
    command_tx: mpsc::Sender<AcpCommand>,
    agent_capabilities: acp::AgentCapabilities,
    /// Channel to receive approval requests from the agent.
    /// The UI layer should listen on this channel and respond via the oneshot sender.
    approval_rx: mpsc::Receiver<ApprovalRequest>,
    /// Channel to receive inter-turn notifications that arrive after
    /// `unregister_session` but before the next `register_session`.
    persistent_rx: mpsc::Receiver<acp::SessionUpdate>,
    /// Thread-safe model state shared between the main thread and worker thread.
    /// Updated when sessions are created or models are switched.
    model_state: Arc<RwLock<AcpModelState>>,
    /// Worker thread handle. Stored as Option inside Mutex to allow taking in Drop.
    worker_thread: Mutex<Option<thread::JoinHandle<()>>>,
    /// Synchronous channel to receive notification when worker thread cleanup is complete.
    /// This allows Drop to wait for the child process kill to finish.
    /// Wrapped in Mutex<Option<>> for Sync (required by Arc<AcpConnection>).
    shutdown_complete_rx: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

/// Internal connection state that lives on the worker thread.
struct AcpConnectionInner {
    connection: acp::ClientSideConnection,
    #[allow(dead_code)]
    client_delegate: Rc<ClientDelegate>,
    child: Child,
    /// IO task that handles reading from the agent's stdout.
    /// Aborted during cleanup to prevent hanging on orphaned pipes.
    io_task: tokio::task::JoinHandle<acp::Result<()>>,
    /// Stderr task that logs the agent's stderr output.
    /// Aborted during cleanup to prevent hanging on orphaned pipes.
    stderr_task: tokio::task::JoinHandle<()>,
}

/// Client delegate that handles requests from the ACP agent.
///
/// This implements the `acp::Client` trait to handle:
/// - Session update notifications
/// - Permission requests
/// - File system operations
/// - Terminal operations (stubbed)
pub struct ClientDelegate {
    sessions: RefCell<HashMap<acp::SessionId, mpsc::Sender<acp::SessionUpdate>>>,
    /// Persistent fallback listener for inter-turn notifications.
    /// When a session notification arrives for an unregistered session (e.g.,
    /// between turns after `unregister_session` is called), the notification
    /// is forwarded here instead of being silently dropped.
    persistent_tx: RefCell<Option<mpsc::Sender<acp::SessionUpdate>>>,
    /// Working directory for approval events
    cwd: PathBuf,
    /// Channel to send approval requests to the UI layer
    approval_tx: mpsc::Sender<ApprovalRequest>,
}
