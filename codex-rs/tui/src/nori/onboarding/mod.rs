//! Nori-specific onboarding flow.
//!
//! This module provides Nori-branded onboarding widgets that replace the
//! default Codex onboarding experience. It includes:
//!
//! - First-launch detection (`~/.nori/cli/config.toml`)
//! - Nori-branded welcome screen with ASCII banner
//! - Nori-branded directory trust prompts
//! - Nori-branded onboarding screen orchestration
//!
//! The onboarding flow is:
//! 1. First-launch welcome (if `~/.nori/cli/config.toml` doesn't exist)
//! 2. Directory trust prompt (if directory not yet trusted)

mod first_launch;
mod onboarding_screen;
mod trust_directory;
mod welcome;

// Re-exports for the Nori onboarding module
pub(crate) use first_launch::is_first_launch;
pub(crate) use first_launch::mark_first_launch_complete;
#[allow(unused_imports)]
pub(crate) use onboarding_screen::NoriOnboardingResult;
#[allow(unused_imports)]
pub(crate) use onboarding_screen::NoriOnboardingScreen;
pub(crate) use onboarding_screen::NoriOnboardingScreenArgs;
pub(crate) use onboarding_screen::run_nori_onboarding_app;
pub(crate) use trust_directory::NoriTrustDirectoryWidget;
pub(crate) use welcome::NoriWelcomeWidget;

// Re-export the selection enum for compatibility
#[allow(unused_imports)]
pub(crate) use crate::onboarding::TrustDirectorySelection;
