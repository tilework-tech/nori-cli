//! Agent Context Protocol (ACP) implementation for Codex
//!
//! This crate provides JSON-RPC 2.0-based communication with ACP-compliant
//! agent subprocesses over stdin/stdout.

pub mod acp_client;
pub mod agent;
pub mod client;
pub mod client_handler;
pub mod registry;
pub mod session;
pub mod tracing_setup;

pub use acp_client::{AcpEvent, AcpModelClient, AcpStream};
pub use agent::AgentProcess;
pub use client_handler::{AcpClientHandler, ClientEvent};
pub use registry::{AcpAgentConfig, get_agent_config};
pub use session::{AcpSession, SessionState};
pub use tracing_setup::init_file_tracing;

// Re-export commonly used types from agent-client-protocol
pub use agent_client_protocol::{
    Agent, Client, ClientSideConnection, InitializeRequest, InitializeResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, SessionNotification, SessionUpdate,
};
