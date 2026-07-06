//! Sandboxed command execution: platform sandbox selection (Seatbelt,
//! Landlock/seccomp, Windows restricted tokens), process spawning, and the
//! exec engine that runs commands under a sandbox policy.
//!
//! Split out of `codex-core` during the crate-layering cleanup
//! (`docs/specs/crate-layering.md`). This crate must not depend on config or
//! auth machinery; policy types it consumes live in
//! `codex_protocol::config_types`.

// Prevent accidental direct writes to stdout/stderr in library code.
#![deny(clippy::print_stdout, clippy::print_stderr)]

pub mod error;
pub mod exec;
pub mod exec_env;
pub mod landlock;
mod safety;
pub mod sandboxing;
pub mod seatbelt;
pub mod spawn;
mod text_encoding;
pub mod truncate;

pub use safety::get_platform_sandbox;
pub use safety::set_windows_sandbox_enabled;
