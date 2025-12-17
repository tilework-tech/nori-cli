//! Nori-specific onboarding flow.
//!
//! This module provides Nori-branded onboarding widgets that replace the
//! default Codex onboarding experience. It includes:
//!
//! - First-launch detection (`~/.nori/cli/config.toml`)
//! - Nori-branded welcome screen with ASCII banner
//! - Nori-branded directory trust prompts
//!
//! The onboarding flow is:
//! 1. First-launch welcome (if `~/.nori/cli/config.toml` doesn't exist)
//! 2. Directory trust prompt (if directory not yet trusted)

mod first_launch;
mod trust_directory;
mod welcome;

pub(crate) use first_launch::find_nori_home;
pub(crate) use first_launch::is_first_launch;
pub(crate) use first_launch::mark_first_launch_complete;
pub(crate) use trust_directory::NoriTrustDirectoryWidget;
pub(crate) use welcome::NoriWelcomeWidget;

// Re-export the selection enum for compatibility
pub(crate) use crate::onboarding::TrustDirectorySelection;
