//! The host plain `nori` runs on, built and owned by the CLI.

use std::future::Future;
use std::pin::Pin;

use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use nori_harness::session_manager::SessionManager;
use tokio::sync::mpsc;

/// A host the TUI owns for its whole run: an ACP client pipe that is
/// always the host's controller, and the host's own session manager for
/// foreground features the wire has no method for yet. Both reach the same
/// sessions. Only the CLI's foreground launcher builds one, so an attached
/// terminal never holds the in-process half.
pub struct OwnedHost {
    /// The host's session manager. The TUI never builds one of its own.
    pub manager: SessionManager,
    /// JSON-RPC lines to the host.
    pub to_host: mpsc::Sender<String>,
    /// JSON-RPC lines from the host.
    pub from_host: mpsc::Receiver<String>,
    /// Stops serving the pipe and ends every session the host owns.
    pub stop: StopHost,
}

/// Stops an owned host; resolves once its sessions have ended.
pub type StopHost =
    Box<dyn FnOnce() -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send>;

/// Builds the owned host from the configuration the TUI resolved, after the
/// TUI has loaded the agent registry.
pub type HostLauncher =
    Box<dyn FnOnce(&NoriConfig, &NoriConfigOverrides) -> anyhow::Result<OwnedHost> + Send>;
