// These modules contain status display functionality used by tests.
// The production code path for /status uses different rendering.
#[cfg(feature = "codex-features")]
#[allow(dead_code)]
mod account;
#[cfg(feature = "codex-features")]
#[allow(dead_code)]
mod card;
#[cfg(feature = "codex-features")]
#[allow(dead_code)]
mod format;
#[cfg(feature = "codex-features")]
#[allow(dead_code)]
mod helpers;

// rate_limits exports types used by chatwidget unconditionally, but many
// internal functions are only used by card.rs (gated behind codex-features).
#[allow(dead_code)]
mod rate_limits;

pub(crate) use rate_limits::RateLimitSnapshotDisplay;
pub(crate) use rate_limits::rate_limit_snapshot_display;

#[cfg(feature = "codex-features")]
#[allow(unused_imports)]
pub(crate) use card::new_status_output;

#[cfg(all(test, feature = "codex-features"))]
mod tests;
