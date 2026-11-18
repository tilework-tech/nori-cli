//! Optional remote ACP transport: the WebSocket profile of the upstream
//! [Streamable HTTP & WebSocket Transport RFD].
//!
//! Legacy remote control serves a harness session as an ACP Agent on `/acp`.
//! Managed hosts expose authenticated `/sessions` connections with one
//! controller and any number of watchers. See `docs/remote-control.md`.
//!
//! [Streamable HTTP & WebSocket Transport RFD]:
//!     https://agentclientprotocol.com/rfds/streamable-http-websocket-transport

mod client;
mod connection;
mod host_control;
mod host_wire;
mod hosted_agent;
mod managed;
mod managed_wire;
mod server;
mod standard_wire;
mod wire;

pub use client::ManagedClient;
pub use hosted_agent::HostedAgent;
pub use hosted_agent::HostedEventReceiver;
pub use hosted_agent::HostedSubscription;
pub use hosted_agent::LoadedSession;
pub use server::RemoteAcpServer;
pub use server::parse_bind_addr;

pub use managed::ManagedAgent;
pub use managed::ManagedReply;
pub use managed::ManagedSubscription;
pub use managed::StandardAcpConnection;
pub use managed::managed_event;
