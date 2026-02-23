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
/// - Release builds: `warn,nori_tui=info,acp=info` (warn default, info for TUI/ACP)
///
/// Note: This can be overridden by setting the RUST_LOG environment variable.
fn default_log_level() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "warn,nori_tui=info,acp=info"
    }
}

/// Initialize rolling daily file-based tracing subscriber.
///
/// Sets up a tracing subscriber that writes logs to rolling daily files in the
/// specified directory. Log files are named with the pattern `{prefix}.YYYY-MM-DD.log`.
///
/// Log level is determined by the RUST_LOG environment variable if set,
/// otherwise falls back to build configuration defaults:
/// - Debug builds: DEBUG and above
/// - Release builds: WARN by default, INFO for nori_tui and acp crates
///
/// # Arguments
///
/// * `log_dir` - Directory where log files will be stored (e.g., `~/.nori/cli/log/`)
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
/// let log_dir = Path::new("/home/user/.nori/cli/log");
/// init_rolling_file_tracing(log_dir, "nori-acp").expect("Failed to initialize tracing");
/// // Creates files like: /home/user/.nori/cli/log/nori-acp.2024-01-15.log
/// ```
///
/// # Note
///
/// This function should be called once at program startup. Subsequent calls
/// will return an error since the global subscriber can only be set once.
pub fn init_rolling_file_tracing(log_dir: &Path, file_prefix: &str) -> Result<()> {
    // Create the log directory if it doesn't exist
    std::fs::create_dir_all(log_dir).context("Failed to create log directory")?;

    // Create rolling daily file appender using builder for fallible initialization
    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(file_prefix)
        .build(log_dir)
        .context("Failed to initialize rolling file appender")?;

    // Create non-blocking writer
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Build the subscriber with RUST_LOG override support.
    // If RUST_LOG is set, use it; otherwise fall back to build-dependent defaults.
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_log_level()));

    let subscriber = tracing_subscriber::registry().with(env_filter).with(
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
///
/// Log level is determined by the RUST_LOG environment variable if set,
/// otherwise falls back to build configuration defaults:
/// - Debug builds: DEBUG and above
/// - Release builds: WARN by default, INFO for nori_tui and acp crates
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

    // Build the subscriber with RUST_LOG override support.
    // If RUST_LOG is set, use it; otherwise fall back to build-dependent defaults.
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_log_level()));

    let subscriber = tracing_subscriber::registry().with(env_filter).with(
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that default_log_level returns the correct value based on build type.
    ///
    /// - Debug builds: "debug" (all debug and above)
    /// - Release builds: "warn,nori_tui=info,acp=info" (warn default, but info for TUI/ACP)
    #[test]
    fn test_default_log_level_returns_expected_value() {
        let level = default_log_level();

        #[cfg(debug_assertions)]
        {
            assert_eq!(
                level, "debug",
                "Debug builds should default to 'debug' level"
            );
        }

        #[cfg(not(debug_assertions))]
        {
            assert_eq!(
                level, "warn,nori_tui=info,acp=info",
                "Release builds should default to 'warn,nori_tui=info,acp=info'"
            );
        }
    }

    /// Test that the default log level string can be parsed by EnvFilter.
    /// This ensures the filter string is syntactically valid.
    #[test]
    fn test_default_log_level_is_valid_env_filter() {
        let level = default_log_level();
        let result = EnvFilter::try_new(level);
        assert!(
            result.is_ok(),
            "default_log_level() should return a valid EnvFilter string, got error: {:?}",
            result.err()
        );
    }

    /// Test that RUST_LOG environment variable values can be parsed.
    /// This validates the EnvFilter parsing that we rely on for RUST_LOG overrides.
    #[test]
    fn test_rust_log_env_filter_parsing() {
        // Test that various RUST_LOG values can be parsed
        let test_cases = [
            "debug",
            "info",
            "warn",
            "error",
            "trace",
            "my_crate=debug",
            "warn,my_crate=info,other=debug",
        ];

        for test_value in test_cases {
            let result = EnvFilter::try_new(test_value);
            assert!(
                result.is_ok(),
                "EnvFilter should parse '{}', got error: {:?}",
                test_value,
                result.err()
            );
        }
    }
}
