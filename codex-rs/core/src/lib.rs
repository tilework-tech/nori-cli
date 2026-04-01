//! Root of the `codex-core` library.

// Prevent accidental direct writes to stdout/stderr in library code. All
// user-visible output must go through the appropriate abstraction (e.g.,
// the TUI or the tracing stack).
#![deny(clippy::print_stdout, clippy::print_stderr)]

#[cfg(feature = "legacy-http-backend")]
pub(crate) mod api_bridge;
#[cfg(feature = "legacy-http-backend")]
mod apply_patch;
pub mod auth;
pub mod bash;
#[cfg(feature = "legacy-http-backend")]
mod client;
mod client_common;
#[cfg(feature = "legacy-http-backend")]
pub(crate) mod codex;
#[cfg(feature = "legacy-http-backend")]
mod codex_conversation;
#[cfg(feature = "legacy-http-backend")]
pub use codex_conversation::CodexConversation;
mod command_safety;
pub mod config;
pub mod config_loader;
#[cfg(feature = "legacy-http-backend")]
mod context_manager;
pub mod custom_prompts;
#[cfg(feature = "legacy-http-backend")]
mod environment_context;
pub mod error;
pub mod exec;
pub mod exec_env;
mod exec_policy;
pub mod features;
mod flags;
pub mod git_info;
pub mod landlock;
pub mod mcp;
#[cfg(feature = "legacy-http-backend")]
mod mcp_connection_manager;
#[cfg(feature = "legacy-http-backend")]
pub use mcp_connection_manager::MCP_SANDBOX_STATE_CAPABILITY;
#[cfg(feature = "legacy-http-backend")]
pub use mcp_connection_manager::MCP_SANDBOX_STATE_NOTIFICATION;
#[cfg(feature = "legacy-http-backend")]
pub use mcp_connection_manager::SandboxState;
#[cfg(feature = "legacy-http-backend")]
mod mcp_tool_call;
#[cfg(feature = "legacy-http-backend")]
mod message_history;
mod model_provider_info;
pub mod parse_command;
pub mod powershell;
#[cfg(feature = "legacy-http-backend")]
mod response_processing;
pub mod sandboxing;
mod text_encoding;
pub mod token_data;
pub(crate) mod tool_types;
mod truncate;
#[cfg(feature = "legacy-http-backend")]
mod unified_exec;
mod user_instructions;
pub use model_provider_info::DEFAULT_LMSTUDIO_PORT;
pub use model_provider_info::DEFAULT_OLLAMA_PORT;
pub use model_provider_info::LMSTUDIO_OSS_PROVIDER_ID;
pub use model_provider_info::ModelProviderInfo;
pub use model_provider_info::OLLAMA_OSS_PROVIDER_ID;
pub use model_provider_info::built_in_model_providers;
pub use model_provider_info::create_oss_provider_with_base_url;
#[cfg(feature = "legacy-http-backend")]
mod conversation_manager;
mod event_mapping;
pub use codex_protocol::protocol::InitialHistory;
#[cfg(feature = "legacy-http-backend")]
pub use conversation_manager::ConversationManager;
#[cfg(feature = "legacy-http-backend")]
pub use conversation_manager::NewConversation;
// Re-export common auth types for workspace consumers
pub use auth::AuthManager;
pub use auth::CodexAuth;
pub mod default_client;
pub mod model_family;
mod openai_model_info;
pub mod project_doc;
mod rollout;
pub(crate) mod safety;
pub mod seatbelt;
pub mod shell;
pub mod spawn;
pub mod terminal;
mod tools;
pub mod turn_diff_tracker;
pub use rollout::ARCHIVED_SESSIONS_SUBDIR;
pub use rollout::INTERACTIVE_SESSION_SOURCES;
pub use rollout::RolloutRecorder;
pub use rollout::SESSIONS_SUBDIR;
pub use rollout::SessionMeta;
pub use rollout::find_conversation_path_by_id_str;
pub use rollout::list::ConversationItem;
pub use rollout::list::ConversationsPage;
pub use rollout::list::Cursor;
pub use rollout::list::parse_cursor;
pub use rollout::list::read_head_for_summary;
#[cfg(feature = "legacy-http-backend")]
mod function_tool;
#[cfg(feature = "legacy-http-backend")]
mod state;
#[cfg(feature = "legacy-http-backend")]
mod tasks;
mod user_notification;
pub use user_notification::UserNotification;
pub use user_notification::UserNotifier;
#[cfg(feature = "legacy-http-backend")]
mod user_shell_command;
pub mod util;

pub use command_safety::is_safe_command;
pub use safety::get_platform_sandbox;
pub use safety::set_windows_sandbox_enabled;
pub use tool_types::CODEX_APPLY_PATCH_ARG1;
// Re-export the protocol types from the standalone `codex-protocol` crate so existing
// `codex_core::protocol::...` references continue to work across the workspace.
pub use codex_protocol::protocol;
// Re-export protocol config enums to ensure call sites can use the same types
// as those in the protocol crate when constructing protocol messages.
pub use codex_protocol::config_types as protocol_config_types;

#[cfg(feature = "legacy-http-backend")]
pub use client::ModelClient;
#[cfg(feature = "legacy-http-backend")]
pub use client::ResponseEvent;
#[cfg(feature = "legacy-http-backend")]
pub use client::ResponseStream;
#[cfg(feature = "legacy-http-backend")]
pub use client_common::Prompt;
pub use codex_protocol::models::ContentItem;
pub use codex_protocol::models::LocalShellAction;
pub use codex_protocol::models::LocalShellExecAction;
pub use codex_protocol::models::LocalShellStatus;
pub use codex_protocol::models::ResponseItem;
pub use compact::content_items_to_text;
pub use event_mapping::parse_turn_item;
pub mod compact;
pub mod otel_init;
