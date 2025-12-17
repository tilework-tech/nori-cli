//! Agent Context Protocol (ACP) implementation for Nori CLI
//!
//! This crate provides JSON-RPC 2.0-based communication with ACP-compliant
//! agent subprocesses over stdin/stdout (capturing stderr logs).
//!
//! It also provides the Nori configuration system for ACP-only mode,
//! loading settings from `~/.nori/cli/config.toml`.

pub mod backend;
pub mod config;
pub mod connection;
pub mod registry;
pub mod tracing_setup;
pub mod translator;

// Re-export config types for convenience
pub use config::ApprovalPolicy;
pub use config::NoriConfig;
pub use config::NoriConfigOverrides;
pub use config::find_nori_home;

pub use backend::AcpBackend;
pub use backend::AcpBackendConfig;
pub use connection::AcpConnection;
pub use connection::AcpModelState;
pub use connection::ApprovalRequest;
pub use registry::AcpAgentConfig;
pub use registry::AcpAgentInfo;
pub use registry::AcpProviderInfo;
pub use registry::get_agent_config;
pub use registry::list_available_agents;
pub use tracing_setup::init_file_tracing;
pub use translator::TranslatedEvent;
pub use translator::translate_session_update;

// Re-export commonly used types from agent-client-protocol
pub use agent_client_protocol::Agent;
pub use agent_client_protocol::Client;
pub use agent_client_protocol::ClientSideConnection;
pub use agent_client_protocol::InitializeRequest;
pub use agent_client_protocol::InitializeResponse;
pub use agent_client_protocol::NewSessionRequest;
pub use agent_client_protocol::NewSessionResponse;
pub use agent_client_protocol::PromptRequest;
pub use agent_client_protocol::PromptResponse;
pub use agent_client_protocol::SessionNotification;
pub use agent_client_protocol::SessionUpdate;

// Re-export model-related types (unstable feature)
#[cfg(feature = "unstable")]
pub use agent_client_protocol::ModelId;
#[cfg(feature = "unstable")]
pub use agent_client_protocol::ModelInfo;
#[cfg(feature = "unstable")]
pub use agent_client_protocol::SessionModelState;
#[cfg(feature = "unstable")]
pub use agent_client_protocol::SetSessionModelRequest;
#[cfg(feature = "unstable")]
pub use agent_client_protocol::SetSessionModelResponse;
