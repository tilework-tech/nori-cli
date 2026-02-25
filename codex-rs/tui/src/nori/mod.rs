//! Nori-specific customizations for the TUI.
//!
//! This module contains Nori-branded components that replace or extend
//! the default Codex TUI behavior.

pub(crate) mod agent_picker;
pub(crate) mod exit_message;
pub(crate) mod onboarding;
pub(crate) mod resume_session_picker;
pub(crate) mod session_header;
pub(crate) mod skillset_picker;
pub(crate) mod token_count;
pub(crate) mod viewonly_session_picker;

#[cfg(feature = "nori-config")]
pub(crate) mod config_adapter;

#[cfg(feature = "nori-config")]
pub(crate) mod config_picker;

pub(crate) mod hotkey_match;
pub(crate) mod hotkey_picker;

#[cfg(feature = "nori-config")]
pub(crate) mod loop_count_picker;

#[cfg(feature = "nori-config")]
pub(crate) mod worktree_ask;

// update_action is available in all builds for the UpdateAction type
// update_prompt and updates are only for release builds
pub(crate) mod update_action;
#[cfg(not(debug_assertions))]
pub(crate) mod update_prompt;
#[cfg(not(debug_assertions))]
pub(crate) mod updates;
