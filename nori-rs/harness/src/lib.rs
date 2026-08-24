//! Agent Context Protocol (ACP) implementation for Nori CLI
//!
//! This crate provides JSON-RPC 2.0-based communication with ACP-compliant
//! agent subprocesses over stdin/stdout (capturing stderr logs).
//!
//! Configuration lives in the `nori-config` crate; the low-level connection,
//! registry, and translator machinery lives in `nori-acp-host`. This crate
//! re-exports what the frontends still consume while the harness layer forms.

pub mod auto_worktree;
pub mod backend;
pub mod bash;
pub mod compact;
pub(crate) use nori_config as config;
pub mod custom_prompts;
pub use custom_prompts::CustomPrompt;
pub use custom_prompts::CustomPromptKind;
pub use custom_prompts::PROMPTS_CMD_PREFIX;
pub use nori_acp_host::patch;
pub mod powershell;
pub mod shell;
mod user_notification;
pub use nori_acp_host::connection;
pub use user_notification::UserNotification;
pub use user_notification::UserNotifier;
pub mod hooks;
pub mod message_history;
pub(crate) mod normalized;
pub use nori_acp_host::registry;
pub mod runtime;
pub mod tracing_setup;
pub mod transcript;
pub mod transcript_discovery;
pub mod undo;
pub use undo::UndoSnapshot;

// Re-export message history types
pub use message_history::ConversationId;
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
pub use backend::SessionContext;
pub use backend::probe::AgentSessionsProbe;
pub use backend::probe::ProbeError;
pub use backend::probe::probe_agent_sessions;
pub use backend::probe::probe_agent_sessions_for;
pub use connection::acp_connection::AcpConnection;
pub use registry::AcpAgentConfig;
pub use registry::AcpAgentInfo;
pub use registry::AcpProviderInfo;
pub use registry::AgentKind;
pub use registry::OtherModel;
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
pub use transcript_discovery::discover_transcript_for_agent_with_message;
pub use transcript_discovery::parse_transcript_tokens;
pub use transcript_discovery::parse_transcript_total_tokens;

// Re-export transcript types
pub use transcript::ProjectId;
pub use transcript::ProjectInfo;
pub use transcript::SessionInfo;
pub use transcript::SessionMetadata;
pub use transcript::Transcript;
pub use transcript::TranscriptLoader;
pub use transcript::TranscriptRecord;
pub use transcript::TranscriptRecorder;
