//! Agent Context Protocol (ACP) implementation for Codex
//!
//! This crate provides JSON-RPC 2.0-based communication with ACP-compliant
//! agent subprocesses over stdin/stdout (capturing stderr logs).

pub mod registry;
pub mod tracing_setup;

pub use registry::get_agent_config;
pub use tracing_setup::init_file_tracing;

// Re-export commonly used types from agent-client-protocol
pub use agent_client_protocol::{
    Agent, Client, ClientSideConnection, InitializeRequest, InitializeResponse, NewSessionRequest,
    NewSessionResponse, PromptRequest, PromptResponse, SessionNotification, SessionUpdate,
};
