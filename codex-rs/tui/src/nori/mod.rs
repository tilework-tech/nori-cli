//! Nori-specific customizations for the TUI.
//!
//! This module contains Nori-branded components that replace or extend
//! the default Codex TUI behavior.

pub(crate) mod agent_picker;
pub(crate) mod onboarding;
pub(crate) mod session_header;

#[cfg(feature = "nori-config")]
pub(crate) mod config_adapter;

#[cfg(not(feature = "feedback"))]
pub(crate) mod feedback;

// update_action is available in all builds for the UpdateAction type
// update_prompt and updates are only for release builds
#[cfg(not(feature = "upstream-updates"))]
pub(crate) mod update_action;
#[cfg(all(not(feature = "upstream-updates"), not(debug_assertions)))]
pub(crate) mod update_prompt;
#[cfg(all(not(feature = "upstream-updates"), not(debug_assertions)))]
pub(crate) mod updates;
