//! The harness side of the remote ACP transport: the managed host and the
//! transport types it is served with.

pub use crate::managed_remote::ManagedRemoteHost;
pub use nori_acp_host::recovery;
pub use nori_acp_host::remote::HostConnection;
pub use nori_acp_host::remote::ManagedAgent;
pub use nori_acp_host::remote::ManagedClient;
pub use nori_acp_host::remote::ManagedReply;
pub use nori_acp_host::remote::ManagedSubscription;
pub use nori_acp_host::remote::RemoteAcpServer;

/// Responder label on answers that a remote host client sends.
pub const REMOTE_CONTROL_RESPONDER: &str = "remote_control";
