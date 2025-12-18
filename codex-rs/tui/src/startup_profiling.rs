//! Startup profiling instrumentation for codex-tui.
//!
//! This module provides detailed tracing spans and milestone tracking for profiling
//! the TUI startup performance. Enable with the `startup-profiling` feature.
//!
//! ## Usage
//!
//! Run with profiling enabled:
//! ```bash
//! CODEX_PROFILE_STARTUP=1 cargo run --features startup-profiling -- [args]
//! ```
//!
//! This will generate trace files in the current directory:
//! - `codex-startup-trace.json` - Chrome tracing format (open in chrome://tracing or https://ui.perfetto.dev)
//! - `codex-startup.folded` - Flame graph format (use with inferno or flamegraph.pl)
//!
//! ## Key Milestones
//!
//! The following milestones are tracked and reported:
//!
//! 1. **startup_begin** - Process starts initialization
//! 2. **config_loaded** - Configuration loading complete
//! 3. **logging_ready** - Tracing/logging infrastructure ready
//! 4. **terminal_init** - Terminal initialized (raw mode, crossterm)
//! 5. **tui_created** - Tui struct created with event streams
//! 6. **chat_widget_created** - ChatWidget created, input is interactive
//! 7. **agent_spawned** - Agent subprocess spawned
//! 8. **session_configured** - Session header shows, agent ready
//! 9. **first_frame** - First frame rendered to terminal
//!

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Global startup instant for calculating durations
static STARTUP_INSTANT: OnceLock<Instant> = OnceLock::new();

/// Flags to track which milestones have been recorded
static MILESTONES_RECORDED: AtomicU64 = AtomicU64::new(0);

/// Whether profiling is enabled (set once at startup)
static PROFILING_ENABLED: AtomicBool = AtomicBool::new(false);

/// Milestone bit flags
const MILESTONE_STARTUP_BEGIN: u64 = 1 << 0;
const MILESTONE_CONFIG_LOADED: u64 = 1 << 1;
const MILESTONE_LOGGING_READY: u64 = 1 << 2;
const MILESTONE_TERMINAL_INIT: u64 = 1 << 3;
const MILESTONE_TUI_CREATED: u64 = 1 << 4;
const MILESTONE_CHAT_WIDGET_CREATED: u64 = 1 << 5;
const MILESTONE_AGENT_SPAWNED: u64 = 1 << 6;
const MILESTONE_SESSION_CONFIGURED: u64 = 1 << 7;
const MILESTONE_FIRST_FRAME: u64 = 1 << 8;

/// Guards for tracing layers that must be kept alive
#[cfg(feature = "startup-profiling")]
pub struct ProfileGuards {
    _chrome_guard: tracing_chrome::FlushGuard,
    _flame_guard: tracing_flame::FlushGuard<std::io::BufWriter<std::fs::File>>,
}

/// Profile guards (no-op when profiling feature is disabled)
#[cfg(not(feature = "startup-profiling"))]
pub struct ProfileGuards;

/// Initialize startup profiling if enabled via environment variable.
///
/// Returns guards that must be kept alive for the duration of profiling,
/// or None if profiling is not enabled.
///
/// Set `CODEX_PROFILE_STARTUP=1` to enable profiling.
#[cfg(feature = "startup-profiling")]
pub fn init_profiling() -> Option<ProfileGuards> {
    use tracing_subscriber::prelude::*;

    // Check if profiling is requested
    let enabled = std::env::var("CODEX_PROFILE_STARTUP")
        .is_ok_and(|v| v == "1" || v.to_lowercase() == "true");

    if !enabled {
        return None;
    }

    PROFILING_ENABLED.store(true, Ordering::SeqCst);

    // Record the startup instant
    let _ = STARTUP_INSTANT.set(Instant::now());

    // Create chrome tracing layer
    let (chrome_layer, chrome_guard) = tracing_chrome::ChromeLayerBuilder::new()
        .file("codex-startup-trace.json")
        .include_args(true)
        .build();

    // Create flame graph layer - with_file takes a path, not a writer
    let Ok((flame_layer, flame_guard)) = tracing_flame::FlameLayer::with_file("codex-startup.folded") else {
        // Silently fail - profiling is optional
        return None;
    };

    // Install the subscriber with both layers
    let subscriber = tracing_subscriber::registry()
        .with(chrome_layer)
        .with(flame_layer);

    if tracing::subscriber::set_global_default(subscriber).is_err() {
        // Silently fail - profiling is optional
        return None;
    }

    tracing::info!("Startup profiling enabled - trace files will be written on exit");

    Some(ProfileGuards {
        _chrome_guard: chrome_guard,
        _flame_guard: flame_guard,
    })
}

#[cfg(not(feature = "startup-profiling"))]
pub fn init_profiling() -> Option<ProfileGuards> {
    None
}

/// Check if startup profiling is enabled
pub fn is_profiling_enabled() -> bool {
    PROFILING_ENABLED.load(Ordering::Relaxed)
}

/// Get the duration since startup in milliseconds
pub fn millis_since_startup() -> u64 {
    STARTUP_INSTANT
        .get()
        .map(|start| start.elapsed().as_millis() as u64)
        .unwrap_or(0)
}

/// Record a milestone and log the timing
fn record_milestone(flag: u64, name: &'static str) {
    let prev = MILESTONES_RECORDED.fetch_or(flag, Ordering::SeqCst);
    if prev & flag == 0 {
        // First time recording this milestone
        let ms = millis_since_startup();
        tracing::info!(
            target: "startup_profiling",
            milestone = name,
            elapsed_ms = ms,
            "Milestone reached: {name} at {ms}ms"
        );
    }
}

// Milestone recording functions - these are no-ops if profiling is disabled

/// Record that startup has begun
pub fn mark_startup_begin() {
    let _ = STARTUP_INSTANT.set(Instant::now());
    if cfg!(feature = "startup-profiling") {
        record_milestone(MILESTONE_STARTUP_BEGIN, "startup_begin");
    }
}

/// Record that config has been loaded
pub fn mark_config_loaded() {
    if cfg!(feature = "startup-profiling") {
        record_milestone(MILESTONE_CONFIG_LOADED, "config_loaded");
    }
}

/// Record that logging is ready
pub fn mark_logging_ready() {
    if cfg!(feature = "startup-profiling") {
        record_milestone(MILESTONE_LOGGING_READY, "logging_ready");
    }
}

/// Record that terminal is initialized
pub fn mark_terminal_init() {
    if cfg!(feature = "startup-profiling") {
        record_milestone(MILESTONE_TERMINAL_INIT, "terminal_init");
    }
}

/// Record that Tui struct is created
pub fn mark_tui_created() {
    if cfg!(feature = "startup-profiling") {
        record_milestone(MILESTONE_TUI_CREATED, "tui_created");
    }
}

/// Record that ChatWidget is created (chat is now interactive)
pub fn mark_chat_widget_created() {
    if cfg!(feature = "startup-profiling") {
        record_milestone(MILESTONE_CHAT_WIDGET_CREATED, "chat_widget_created");
    }
}

/// Record that agent has been spawned
pub fn mark_agent_spawned() {
    if cfg!(feature = "startup-profiling") {
        record_milestone(MILESTONE_AGENT_SPAWNED, "agent_spawned");
    }
}

/// Record that session is configured (session header shows)
pub fn mark_session_configured() {
    if cfg!(feature = "startup-profiling") {
        record_milestone(MILESTONE_SESSION_CONFIGURED, "session_configured");
    }
}

/// Record that first frame has been rendered
pub fn mark_first_frame() {
    if cfg!(feature = "startup-profiling") {
        record_milestone(MILESTONE_FIRST_FRAME, "first_frame");
    }
}

/// Log a startup profiling summary
pub fn log_startup_summary() {
    if !cfg!(feature = "startup-profiling") {
        return;
    }

    let total_ms = millis_since_startup();
    let milestones = MILESTONES_RECORDED.load(Ordering::SeqCst);

    tracing::info!(
        target: "startup_profiling",
        total_startup_ms = total_ms,
        milestones_reached = milestones,
        "Startup profiling complete - total time: {total_ms}ms"
    );
}
