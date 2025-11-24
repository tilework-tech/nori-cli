//! File-based tracing subscriber setup for ACP
//!
//! Provides initialization for logging ACP activity to a file using the tracing framework.

use anyhow::{Context, Result};
use std::path::Path;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize file-based tracing subscriber
///
/// Sets up a tracing subscriber that writes logs to the specified file path.
/// Log level is set to DEBUG and above (TRACE is filtered out).
///
/// # Arguments
///
/// * `log_file_path` - Path to the log file to create/append to
///
/// # Returns
///
/// * `Ok(())` if initialization succeeds
/// * `Err` if the global subscriber is already set or file cannot be created
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use codex_acp::init_file_tracing;
///
/// let log_path = Path::new(".codex-acp.log");
/// init_file_tracing(log_path).expect("Failed to initialize tracing");
/// ```
///
/// # Note
///
/// This function should be called once at program startup. Subsequent calls
/// will return an error since the global subscriber can only be set once.
pub fn init_file_tracing(log_file_path: &Path) -> Result<()> {
    // Create the parent directory if it doesn't exist
    if let Some(parent) = log_file_path.parent() {
        std::fs::create_dir_all(parent).context("Failed to create log file parent directory")?;
    }

    // Create file appender
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_path)
        .context("Failed to open log file")?;

    // Create non-blocking writer
    let (non_blocking, _guard) = tracing_appender::non_blocking(file);

    // Build the subscriber with DEBUG level filter
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new("debug"))
        .with(
            fmt::layer().with_writer(non_blocking).with_ansi(false), // Disable ANSI colors for file output
        );

    // Set as global default - this will fail if already set
    subscriber
        .try_init()
        .map_err(|e| anyhow::anyhow!("Failed to set global subscriber: {e}"))?;

    // Leak the guard to prevent it from being dropped
    // This ensures the non-blocking writer continues to work
    std::mem::forget(_guard);

    Ok(())
}
