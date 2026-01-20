//! Nori-specific customizations for the TUI.
//!
//! This module contains Nori-branded components that replace or extend
//! the default Codex TUI behavior.

pub(crate) mod agent_picker;
pub(crate) mod exit_message;
pub(crate) mod onboarding;
pub(crate) mod session_header;

#[cfg(feature = "nori-config")]
pub(crate) mod config_adapter;

// update_action is available in all builds for the UpdateAction type
// update_prompt and updates are only for release builds
pub(crate) mod update_action;
#[cfg(not(debug_assertions))]
pub(crate) mod update_prompt;
#[cfg(not(debug_assertions))]
pub(crate) mod updates;
