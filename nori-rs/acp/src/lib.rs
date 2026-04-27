//! Agent Context Protocol (ACP) implementation for Nori CLI
//!
//! This crate provides JSON-RPC 2.0-based communication with ACP-compliant
//! agent subprocesses over stdin/stdout (capturing stderr logs).
//!
//! It also provides the Nori configuration system for ACP-only mode,
//! loading settings from `~/.nori/cli/config.toml`.

pub mod auto_worktree;
pub mod backend;
pub mod config;
pub mod connection;
pub mod hooks;
pub mod message_history;
pub mod registry;
pub mod session_parser;
pub mod tracing_setup;
pub mod transcript;
pub mod transcript_discovery;
pub mod translator;
pub mod undo;

// Re-export config types for convenience
pub use config::ApprovalPolicy;
pub use config::FileManager;
pub use config::HistoryPersistence;
pub use config::NoriConfig;
pub use config::NoriConfigOverrides;
pub use config::find_nori_home;

// Re-export message history types
pub use message_history::HistoryEntry;
pub use message_history::append_entry;
pub use message_history::history_filepath;
pub use message_history::history_metadata;
#[cfg(any(unix, windows))]
pub use message_history::lookup;
pub use message_history::search_entries;

pub use backend::AcpBackend;
pub use backend::AcpBackendConfig;
pub use backend::BackendEvent;
pub use connection::AcpModelState;
pub use connection::ApprovalRequest;
pub use connection::sacp_connection::SacpConnection;
pub use registry::AcpAgentConfig;
pub use registry::AcpAgentInfo;
pub use registry::AcpProviderInfo;
pub use registry::AgentKind;
pub use registry::PackageManager;
pub use registry::Provider;
pub use registry::RegisteredAgent;
pub use registry::build_default_agents;
pub use registry::build_registry;
pub use registry::detect_preferred_package_manager;
pub use registry::get_agent_config;
pub use registry::get_agent_display_name;
pub use registry::initialize_registry;
pub use registry::list_available_agents;
pub use registry::prewarm_installation_cache;
pub use tracing_setup::init_file_tracing;
pub use tracing_setup::init_rolling_file_tracing;
pub use transcript_discovery::DiscoveryError;
pub use transcript_discovery::TranscriptLocation;
pub use transcript_discovery::TranscriptTokenUsage;
pub use transcript_discovery::discover_transcript_for_agent;
pub use transcript_discovery::discover_transcript_for_agent_with_message;
pub use transcript_discovery::parse_transcript_tokens;
pub use transcript_discovery::parse_transcript_total_tokens;
pub use translator::TranslatedEvent;
pub use translator::translate_session_update;

// Re-export transcript types
pub use transcript::ProjectId;
pub use transcript::ProjectInfo;
pub use transcript::SessionInfo;
pub use transcript::SessionMetadata;
pub use transcript::Transcript;
pub use transcript::TranscriptLoader;
pub use transcript::TranscriptRecorder;

// Re-export commonly used types from the ACP schema
pub use agent_client_protocol_schema::InitializeRequest;
pub use agent_client_protocol_schema::InitializeResponse;
pub use agent_client_protocol_schema::NewSessionRequest;
pub use agent_client_protocol_schema::NewSessionResponse;
pub use agent_client_protocol_schema::PromptRequest;
pub use agent_client_protocol_schema::PromptResponse;
pub use agent_client_protocol_schema::SessionNotification;
pub use agent_client_protocol_schema::SessionUpdate;

// Re-export model-related types (unstable feature)
#[cfg(feature = "unstable")]
pub use agent_client_protocol_schema::ModelId;
#[cfg(feature = "unstable")]
pub use agent_client_protocol_schema::ModelInfo;
#[cfg(feature = "unstable")]
pub use agent_client_protocol_schema::SessionModelState;
#[cfg(feature = "unstable")]
pub use agent_client_protocol_schema::SetSessionModelRequest;
#[cfg(feature = "unstable")]
pub use agent_client_protocol_schema::SetSessionModelResponse;
