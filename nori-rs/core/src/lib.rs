//! Root of the `codex-core` library.

// Prevent accidental direct writes to stdout/stderr in library code. All
// user-visible output must go through the appropriate abstraction (e.g.,
// the TUI or the tracing stack).
#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod auth;
pub mod bash;
pub mod config;
pub mod config_loader;
pub mod custom_prompts;
pub mod error;
pub mod exec_env;
pub mod features;
pub mod git_info;
pub mod landlock;
pub mod mcp;
mod model_provider_info;
pub use model_provider_info::LMSTUDIO_OSS_PROVIDER_ID;
pub use model_provider_info::ModelProviderInfo;
pub use model_provider_info::OLLAMA_OSS_PROVIDER_ID;
pub use model_provider_info::built_in_model_providers;
pub mod exec;
mod openai_model_info;
pub mod parse_command;
pub mod powershell;
pub mod sandboxing;
pub mod shell;
pub mod terminal;
mod text_encoding;
pub mod token_data;
pub(crate) mod tool_types;
mod truncate;
// Re-export common auth types for workspace consumers
pub use auth::AuthManager;
pub use auth::CodexAuth;
pub mod default_client;
pub mod model_family;
pub mod project_doc;
pub(crate) mod safety;
pub mod seatbelt;
pub mod spawn;
mod user_notification;
pub use user_notification::UserNotification;
pub use user_notification::UserNotifier;
pub mod util;

pub use safety::get_platform_sandbox;
pub use safety::set_windows_sandbox_enabled;
pub use tool_types::CODEX_APPLY_PATCH_ARG1;

pub mod compact;
pub mod otel_init;
