//! File-based tracing subscriber setup for ACP
//!
//! Provides initialization for logging ACP activity to a file using the tracing framework.
//! Supports rolling daily logs stored in the configured log directory.

use anyhow::Context;
use anyhow::Result;
use std::path::Path;
use tracing_appender::rolling::RollingFileAppender;
use tracing_appender::rolling::Rotation;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// Returns the default log level based on build configuration.
///
/// - Debug builds: `debug` level (captures debug, info, warn, error)
/// - Release builds: `warn` level (captures warn, error only)
fn default_log_level() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "warn"
    }
}

/// Initialize rolling daily file-based tracing subscriber.
///
/// Sets up a tracing subscriber that writes logs to rolling daily files in the
/// specified directory. Log files are named with the pattern `{prefix}.YYYY-MM-DD.log`.
///
/// Log level is determined by build configuration:
/// - Debug builds: DEBUG and above
/// - Release builds: WARN and above
///
/// # Arguments
///
/// * `log_dir` - Directory where log files will be stored (e.g., `~/.nori/cli/logs/`)
/// * `file_prefix` - Prefix for log file names (e.g., "nori-acp" produces "nori-acp.2024-01-15.log")
///
/// # Returns
///
/// * `Ok(())` if initialization succeeds
/// * `Err` if the global subscriber is already set or directory cannot be created
///
/// # Example
///
/// ```no_run
/// use std::path::Path;
/// use codex_acp::init_rolling_file_tracing;
///
/// let log_dir = Path::new("/home/user/.nori/cli/logs");
/// init_rolling_file_tracing(log_dir, "nori-acp").expect("Failed to initialize tracing");
/// // Creates files like: /home/user/.nori/cli/logs/nori-acp.2024-01-15.log
/// ```
///
/// # Note
///
/// This function should be called once at program startup. Subsequent calls
/// will return an error since the global subscriber can only be set once.
pub fn init_rolling_file_tracing(log_dir: &Path, file_prefix: &str) -> Result<()> {
    // Create the log directory if it doesn't exist
    std::fs::create_dir_all(log_dir).context("Failed to create log directory")?;

    // Create rolling daily file appender
    let file_appender = RollingFileAppender::new(Rotation::DAILY, log_dir, file_prefix);

    // Create non-blocking writer
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Build the subscriber with build-dependent log level filter
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new(default_log_level()))
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

/// Initialize file-based tracing subscriber (legacy single-file mode).
///
/// Sets up a tracing subscriber that writes logs to the specified file path.
/// Log level is determined by build configuration:
/// - Debug builds: DEBUG and above
/// - Release builds: WARN and above
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
///
/// # Deprecated
///
/// Consider using [`init_rolling_file_tracing`] instead for rolling daily logs.
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

    // Build the subscriber with build-dependent log level filter
    let subscriber = tracing_subscriber::registry()
        .with(EnvFilter::new(default_log_level()))
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
