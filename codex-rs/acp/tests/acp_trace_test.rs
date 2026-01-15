//! Tests for ACP trace configuration and command wrapping
//!
//! These tests verify the behavior of:
//! - `acp_trace_enabled` config field
//! - `acp_trace_log_path()` helper with session ID
//! - Command wrapping with sacp-tee proxy

use codex_acp::NoriConfig;
use std::path::PathBuf;
use tempfile::TempDir;

// =============================================================================
// Config Field Tests
// =============================================================================

#[test]
fn test_acp_trace_enabled_defaults_to_false() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Write an empty config file
    std::fs::write(&config_path, "").unwrap();

    let config = NoriConfig::load_from_path(&config_path).unwrap();

    assert!(
        !config.acp_trace_enabled,
        "acp_trace_enabled should default to false"
    );
}

#[test]
fn test_acp_trace_enabled_can_be_loaded_from_toml() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Write a config file with acp_trace_enabled = true
    std::fs::write(&config_path, "acp_trace_enabled = true").unwrap();

    let config = NoriConfig::load_from_path(&config_path).unwrap();

    assert!(
        config.acp_trace_enabled,
        "acp_trace_enabled should be loaded as true from config"
    );
}

#[test]
fn test_acp_trace_enabled_cli_override() {
    use codex_acp::NoriConfigOverrides;

    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    // Write a config file with acp_trace_enabled = false
    std::fs::write(&config_path, "acp_trace_enabled = false").unwrap();

    // Create overrides with acp_trace_enabled = true
    let overrides = NoriConfigOverrides {
        acp_trace_enabled: Some(true),
        ..Default::default()
    };

    let config = NoriConfig::load_from_path_with_overrides(&config_path, overrides).unwrap();

    assert!(
        config.acp_trace_enabled,
        "CLI override should take precedence over config file"
    );
}

// =============================================================================
// Trace Log Path Tests
// =============================================================================

#[test]
fn test_acp_trace_log_path_uses_nori_home() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.toml");

    std::fs::write(&config_path, "").unwrap();

    let config = NoriConfig::load_from_path(&config_path).unwrap();
    let session_id = "session-456";
    let log_path = config.acp_trace_log_path(session_id);

    // The log path should be under nori_home
    let nori_home_str = config.nori_home.to_string_lossy();
    let log_path_str = log_path.to_string_lossy();

    assert!(
        log_path_str.starts_with(nori_home_str.as_ref()),
        "Log path {:?} should be under nori_home {:?}",
        log_path,
        config.nori_home
    );
}

// =============================================================================
// Command Wrapping Tests
// =============================================================================

#[test]
fn test_build_wrapped_command_without_trace() {
    use codex_acp::connection::build_agent_command;
    use codex_acp::get_agent_config;

    let config = get_agent_config("mock-model").unwrap();
    let trace_config = None;

    let (command, args) = build_agent_command(&config, trace_config);

    // Without tracing, command should be unchanged
    assert!(
        command.contains("mock_acp_agent"),
        "Command should be the original agent command: {}",
        command
    );
    assert!(
        args.is_empty(),
        "Args should be unchanged (empty for mock): {:?}",
        args
    );
}

#[test]
fn test_build_wrapped_command_with_trace() {
    use codex_acp::connection::AcpTraceConfig;
    use codex_acp::connection::build_agent_command;
    use codex_acp::get_agent_config;

    let config = get_agent_config("mock-model").unwrap();
    let log_path = PathBuf::from("/tmp/test-trace.log");
    let trace_config = Some(AcpTraceConfig {
        log_file_path: log_path.clone(),
    });

    let (command, args) = build_agent_command(&config, trace_config);

    // With tracing, command should be sacp-tee
    assert_eq!(command, "sacp-tee", "Command should be sacp-tee");

    // Args should include --log-file, the path, --, and the original command
    assert!(
        args.contains(&"--log-file".to_string()),
        "Args should contain --log-file: {:?}",
        args
    );
    assert!(
        args.contains(&log_path.to_string_lossy().to_string()),
        "Args should contain log path: {:?}",
        args
    );
    assert!(
        args.contains(&"--".to_string()),
        "Args should contain -- separator: {:?}",
        args
    );
    // The original command should be after --
    let separator_pos = args.iter().position(|a| a == "--").unwrap();
    assert!(
        args[separator_pos + 1..]
            .iter()
            .any(|a| a.contains("mock_acp_agent")),
        "Original command should be after --: {:?}",
        args
    );
}

#[test]
fn test_build_wrapped_command_preserves_original_args() {
    use codex_acp::connection::AcpTraceConfig;
    use codex_acp::connection::build_agent_command;
    use codex_acp::get_agent_config;

    // Use gemini which has args (--experimental-acp)
    let config = get_agent_config("gemini").unwrap();
    let log_path = PathBuf::from("/tmp/test-trace.log");
    let trace_config = Some(AcpTraceConfig {
        log_file_path: log_path,
    });

    let (command, args) = build_agent_command(&config, trace_config);

    assert_eq!(command, "sacp-tee");

    // Original args should be preserved after --
    let separator_pos = args.iter().position(|a| a == "--").unwrap();
    let downstream_args = &args[separator_pos + 1..];

    // Should have the original command and its args
    assert!(
        downstream_args.iter().any(|a| a.contains("gemini-cli")),
        "Should contain original gemini-cli: {:?}",
        downstream_args
    );
    assert!(
        downstream_args.iter().any(|a| a == "--experimental-acp"),
        "Should preserve original --experimental-acp arg: {:?}",
        downstream_args
    );
}
