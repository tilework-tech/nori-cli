//! Optional remote ACP transport: the WebSocket profile of the upstream
//! [Streamable HTTP & WebSocket Transport RFD].
//!
//! Serves the hosted harness session as an ACP Agent on a single `/acp`
//! endpoint. Disabled unless remote mode is explicitly enabled; loopback by
//! default. See `docs/specs/remote-acp-transport.md`.
//!
//! [Streamable HTTP & WebSocket Transport RFD]:
//!     https://agentclientprotocol.com/rfds/streamable-http-websocket-transport

mod connection;
mod hosted_agent;
mod server;
mod wire;

pub use hosted_agent::HostedAgent;
pub use hosted_agent::HostedEventReceiver;
pub use hosted_agent::HostedSubscription;
pub use hosted_agent::LoadedSession;
pub use server::RemoteAcpServer;
pub use server::parse_bind_addr;
