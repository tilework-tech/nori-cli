//! Agent Context Protocol (ACP) implementation for Codex
//!
//! This crate provides JSON-RPC 2.0-based communication with ACP-compliant
//! agent subprocesses over stdin/stdout.

pub mod acp_client;
pub mod agent;
pub mod client;
pub mod protocol;
pub mod registry;
pub mod session;
pub mod transport;

pub use acp_client::{AcpEvent, AcpModelClient, AcpStream};
pub use agent::AgentProcess;
pub use protocol::{JsonRpcError, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};
pub use registry::{AcpAgentConfig, get_agent_config};
pub use session::{AcpSession, SessionState};
pub use transport::StdioTransport;

// Re-export commonly used types from tokio
pub use tokio::process::{ChildStdin, ChildStdout};
