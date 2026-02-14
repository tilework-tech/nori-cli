use super::*;
use crate::config::edit::ConfigEdit;
use crate::config::edit::ConfigEditsBuilder;
use crate::config::edit::apply_blocking;
use crate::config::types::HistoryPersistence;
use crate::config::types::McpServerTransportConfig;
use crate::features::Feature;
use pretty_assertions::assert_eq;

use std::time::Duration;
use tempfile::TempDir;

#[test]
fn test_toml_parsing() {
    let history_with_persistence = r#"
[history]
persistence = "save-all"
"#;
    let history_with_persistence_cfg = toml::from_str::<ConfigToml>(history_with_persistence)
        .expect("TOML deserialization should succeed");
    assert_eq!(
        Some(History {
            persistence: HistoryPersistence::SaveAll,
            max_bytes: None,
        }),
        history_with_persistence_cfg.history
    );

    let history_no_persistence = r#"
[history]
persistence = "none"
"#;

    let history_no_persistence_cfg = toml::from_str::<ConfigToml>(history_no_persistence)
        .expect("TOML deserialization should succeed");
    assert_eq!(
        Some(History {
            persistence: HistoryPersistence::None,
            max_bytes: None,
        }),
        history_no_persistence_cfg.history
    );
}

#[test]
fn tui_config_missing_terminal_notifications_field_defaults_to_true() {
    let cfg = r#"
[tui]
"#;

    let parsed = toml::from_str::<ConfigToml>(cfg)
        .expect("TUI config without terminal_notifications should succeed");
    let tui = parsed.tui.expect("config should include tui section");

    assert!(tui.terminal_notifications);
}

#[test]
fn test_sandbox_config_parsing() {
    let sandbox_full_access = r#"
sandbox_mode = "danger-full-access"

[sandbox_workspace_write]
network_access = false  # This should be ignored.
"#;
    let sandbox_full_access_cfg = toml::from_str::<ConfigToml>(sandbox_full_access)
        .expect("TOML deserialization should succeed");
    let sandbox_mode_override = None;
    let resolution = sandbox_full_access_cfg.derive_sandbox_policy(
        sandbox_mode_override,
        None,
        &PathBuf::from("/tmp/test"),
    );
    assert_eq!(
        resolution,
        SandboxPolicyResolution {
            policy: SandboxPolicy::DangerFullAccess,
            forced_auto_mode_downgraded_on_windows: false,
        }
    );

    let sandbox_read_only = r#"
sandbox_mode = "read-only"

[sandbox_workspace_write]
network_access = true  # This should be ignored.
"#;

    let sandbox_read_only_cfg = toml::from_str::<ConfigToml>(sandbox_read_only)
        .expect("TOML deserialization should succeed");
    let sandbox_mode_override = None;
    let resolution = sandbox_read_only_cfg.derive_sandbox_policy(
        sandbox_mode_override,
        None,
        &PathBuf::from("/tmp/test"),
    );
    assert_eq!(
        resolution,
        SandboxPolicyResolution {
            policy: SandboxPolicy::ReadOnly,
            forced_auto_mode_downgraded_on_windows: false,
        }
    );

    let sandbox_workspace_write = r#"
sandbox_mode = "workspace-write"

[sandbox_workspace_write]
writable_roots = [
"/my/workspace",
]
exclude_tmpdir_env_var = true
exclude_slash_tmp = true
"#;

    let sandbox_workspace_write_cfg = toml::from_str::<ConfigToml>(sandbox_workspace_write)
        .expect("TOML deserialization should succeed");
    let sandbox_mode_override = None;
    let resolution = sandbox_workspace_write_cfg.derive_sandbox_policy(
        sandbox_mode_override,
        None,
        &PathBuf::from("/tmp/test"),
    );
    if cfg!(target_os = "windows") {
        assert_eq!(
            resolution,
            SandboxPolicyResolution {
                policy: SandboxPolicy::ReadOnly,
                forced_auto_mode_downgraded_on_windows: true,
            }
        );
    } else {
        assert_eq!(
            resolution,
            SandboxPolicyResolution {
                policy: SandboxPolicy::WorkspaceWrite {
                    writable_roots: vec![PathBuf::from("/my/workspace")],
                    network_access: false,
                    exclude_tmpdir_env_var: true,
                    exclude_slash_tmp: true,
                },
                forced_auto_mode_downgraded_on_windows: false,
            }
        );
    }

    let sandbox_workspace_write = r#"
sandbox_mode = "workspace-write"

[sandbox_workspace_write]
writable_roots = [
"/my/workspace",
]
exclude_tmpdir_env_var = true
exclude_slash_tmp = true

[projects."/tmp/test"]
trust_level = "trusted"
"#;

    let sandbox_workspace_write_cfg = toml::from_str::<ConfigToml>(sandbox_workspace_write)
        .expect("TOML deserialization should succeed");
    let sandbox_mode_override = None;
    let resolution = sandbox_workspace_write_cfg.derive_sandbox_policy(
        sandbox_mode_override,
        None,
        &PathBuf::from("/tmp/test"),
    );
    if cfg!(target_os = "windows") {
        assert_eq!(
            resolution,
            SandboxPolicyResolution {
                policy: SandboxPolicy::ReadOnly,
                forced_auto_mode_downgraded_on_windows: true,
            }
        );
    } else {
        assert_eq!(
            resolution,
            SandboxPolicyResolution {
                policy: SandboxPolicy::WorkspaceWrite {
                    writable_roots: vec![PathBuf::from("/my/workspace")],
                    network_access: false,
                    exclude_tmpdir_env_var: true,
                    exclude_slash_tmp: true,
                },
                forced_auto_mode_downgraded_on_windows: false,
            }
        );
    }
}

#[test]
fn add_dir_override_extends_workspace_writable_roots() -> std::io::Result<()> {
    let temp_dir = TempDir::new()?;
    let frontend = temp_dir.path().join("frontend");
    let backend = temp_dir.path().join("backend");
    std::fs::create_dir_all(&frontend)?;
    std::fs::create_dir_all(&backend)?;

    let overrides = ConfigOverrides {
        cwd: Some(frontend),
        sandbox_mode: Some(SandboxMode::WorkspaceWrite),
        additional_writable_roots: vec![PathBuf::from("../backend"), backend.clone()],
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        ConfigToml::default(),
        overrides,
        temp_dir.path().to_path_buf(),
    )?;

    let expected_backend = canonicalize(&backend).expect("canonicalize backend directory");
    if cfg!(target_os = "windows") {
        assert!(
            config.forced_auto_mode_downgraded_on_windows,
            "expected workspace-write request to be downgraded on Windows"
        );
        match config.sandbox_policy {
            SandboxPolicy::ReadOnly => {}
            other => panic!("expected read-only policy on Windows, got {other:?}"),
        }
    } else {
        match config.sandbox_policy {
            SandboxPolicy::WorkspaceWrite { writable_roots, .. } => {
                assert_eq!(
                    writable_roots
                        .iter()
                        .filter(|root| **root == expected_backend)
                        .count(),
                    1,
                    "expected single writable root entry for {}",
                    expected_backend.display()
                );
            }
            other => panic!("expected workspace-write policy, got {other:?}"),
        }
    }

    Ok(())
}

#[test]
fn config_defaults_to_file_cli_auth_store_mode() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let cfg = ConfigToml::default();

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )?;

    assert_eq!(
        config.cli_auth_credentials_store_mode,
        AuthCredentialsStoreMode::File,
    );

    Ok(())
}

#[test]
fn config_honors_explicit_keyring_auth_store_mode() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let cfg = ConfigToml {
        cli_auth_credentials_store: Some(AuthCredentialsStoreMode::Keyring),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )?;

    assert_eq!(
        config.cli_auth_credentials_store_mode,
        AuthCredentialsStoreMode::Keyring,
    );

    Ok(())
}

#[test]
fn config_defaults_to_auto_oauth_store_mode() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let cfg = ConfigToml::default();

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )?;

    assert_eq!(
        config.mcp_oauth_credentials_store_mode,
        OAuthCredentialsStoreMode::Auto,
    );

    Ok(())
}

#[test]
fn profile_legacy_toggles_override_base() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let mut profiles = HashMap::new();
    profiles.insert(
        "work".to_string(),
        ConfigProfile {
            tools_view_image: Some(false),
            ..Default::default()
        },
    );
    let cfg = ConfigToml {
        profiles,
        profile: Some("work".to_string()),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )?;

    assert!(!config.features.enabled(Feature::ViewImageTool));

    Ok(())
}

#[test]
fn profile_sandbox_mode_overrides_base() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let mut profiles = HashMap::new();
    profiles.insert(
        "work".to_string(),
        ConfigProfile {
            sandbox_mode: Some(SandboxMode::DangerFullAccess),
            ..Default::default()
        },
    );
    let cfg = ConfigToml {
        profiles,
        profile: Some("work".to_string()),
        sandbox_mode: Some(SandboxMode::ReadOnly),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )?;

    assert!(matches!(
        config.sandbox_policy,
        SandboxPolicy::DangerFullAccess
    ));
    assert!(config.did_user_set_custom_approval_policy_or_sandbox_mode);

    Ok(())
}

#[test]
fn cli_override_takes_precedence_over_profile_sandbox_mode() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let mut profiles = HashMap::new();
    profiles.insert(
        "work".to_string(),
        ConfigProfile {
            sandbox_mode: Some(SandboxMode::DangerFullAccess),
            ..Default::default()
        },
    );
    let cfg = ConfigToml {
        profiles,
        profile: Some("work".to_string()),
        ..Default::default()
    };

    let overrides = ConfigOverrides {
        sandbox_mode: Some(SandboxMode::WorkspaceWrite),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        overrides,
        codex_home.path().to_path_buf(),
    )?;

    if cfg!(target_os = "windows") {
        assert!(matches!(config.sandbox_policy, SandboxPolicy::ReadOnly));
        assert!(config.forced_auto_mode_downgraded_on_windows);
    } else {
        assert!(matches!(
            config.sandbox_policy,
            SandboxPolicy::WorkspaceWrite { .. }
        ));
        assert!(!config.forced_auto_mode_downgraded_on_windows);
    }

    Ok(())
}

#[test]
fn feature_table_overrides_legacy_flags() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let mut entries = BTreeMap::new();
    entries.insert("apply_patch_freeform".to_string(), false);
    let cfg = ConfigToml {
        features: Some(crate::features::FeaturesToml { entries }),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )?;

    assert!(!config.features.enabled(Feature::ApplyPatchFreeform));
    assert!(!config.include_apply_patch_tool);

    Ok(())
}

#[test]
fn legacy_toggles_map_to_features() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let cfg = ConfigToml {
        experimental_use_unified_exec_tool: Some(true),
        experimental_use_rmcp_client: Some(true),
        experimental_use_freeform_apply_patch: Some(true),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )?;

    assert!(config.features.enabled(Feature::ApplyPatchFreeform));
    assert!(config.features.enabled(Feature::UnifiedExec));
    assert!(config.features.enabled(Feature::RmcpClient));

    assert!(config.include_apply_patch_tool);

    assert!(config.use_experimental_unified_exec_tool);
    assert!(config.use_experimental_use_rmcp_client);

    Ok(())
}

#[test]
fn config_honors_explicit_file_oauth_store_mode() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let cfg = ConfigToml {
        mcp_oauth_credentials_store: Some(OAuthCredentialsStoreMode::File),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )?;

    assert_eq!(
        config.mcp_oauth_credentials_store_mode,
        OAuthCredentialsStoreMode::File,
    );

    Ok(())
}

#[tokio::test]
async fn managed_config_overrides_oauth_store_mode() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let managed_path = codex_home.path().join("managed_config.toml");
    let config_path = codex_home.path().join(CONFIG_TOML_FILE);

    std::fs::write(&config_path, "mcp_oauth_credentials_store = \"file\"\n")?;
    std::fs::write(&managed_path, "mcp_oauth_credentials_store = \"keyring\"\n")?;

    let overrides = crate::config_loader::LoaderOverrides {
        managed_config_path: Some(managed_path.clone()),
        #[cfg(target_os = "macos")]
        managed_preferences_base64: None,
    };

    let root_value = load_resolved_config(codex_home.path(), Vec::new(), overrides).await?;
    let cfg: ConfigToml = root_value.try_into().map_err(|e| {
        tracing::error!("Failed to deserialize overridden config: {e}");
        std::io::Error::new(std::io::ErrorKind::InvalidData, e)
    })?;
    assert_eq!(
        cfg.mcp_oauth_credentials_store,
        Some(OAuthCredentialsStoreMode::Keyring),
    );

    let final_config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides::default(),
        codex_home.path().to_path_buf(),
    )?;
    assert_eq!(
        final_config.mcp_oauth_credentials_store_mode,
        OAuthCredentialsStoreMode::Keyring,
    );

    Ok(())
}

#[tokio::test]
async fn load_global_mcp_servers_returns_empty_if_missing() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    assert!(servers.is_empty());

    Ok(())
}

#[tokio::test]
async fn replace_mcp_servers_round_trips_entries() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    let mut servers = BTreeMap::new();
    servers.insert(
        "docs".to_string(),
        McpServerConfig {
            transport: McpServerTransportConfig::Stdio {
                command: "echo".to_string(),
                args: vec!["hello".to_string()],
                env: None,
                env_vars: Vec::new(),
                cwd: None,
            },
            enabled: true,
            startup_timeout_sec: Some(Duration::from_secs(3)),
            tool_timeout_sec: Some(Duration::from_secs(5)),
            enabled_tools: None,
            disabled_tools: None,
        },
    );

    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(servers.clone())],
    )?;

    let loaded = load_global_mcp_servers(codex_home.path()).await?;
    assert_eq!(loaded.len(), 1);
    let docs = loaded.get("docs").expect("docs entry");
    match &docs.transport {
        McpServerTransportConfig::Stdio {
            command,
            args,
            env,
            env_vars,
            cwd,
        } => {
            assert_eq!(command, "echo");
            assert_eq!(args, &vec!["hello".to_string()]);
            assert!(env.is_none());
            assert!(env_vars.is_empty());
            assert!(cwd.is_none());
        }
        other => panic!("unexpected transport {other:?}"),
    }
    assert_eq!(docs.startup_timeout_sec, Some(Duration::from_secs(3)));
    assert_eq!(docs.tool_timeout_sec, Some(Duration::from_secs(5)));
    assert!(docs.enabled);

    let empty = BTreeMap::new();
    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(empty.clone())],
    )?;
    let loaded = load_global_mcp_servers(codex_home.path()).await?;
    assert!(loaded.is_empty());

    Ok(())
}

#[tokio::test]
async fn managed_config_wins_over_cli_overrides() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let managed_path = codex_home.path().join("managed_config.toml");

    std::fs::write(
        codex_home.path().join(CONFIG_TOML_FILE),
        "model = \"base\"\n",
    )?;
    std::fs::write(&managed_path, "model = \"managed_config\"\n")?;

    let overrides = crate::config_loader::LoaderOverrides {
        managed_config_path: Some(managed_path),
        #[cfg(target_os = "macos")]
        managed_preferences_base64: None,
    };

    let root_value = load_resolved_config(
        codex_home.path(),
        vec![("model".to_string(), TomlValue::String("cli".to_string()))],
        overrides,
    )
    .await?;

    let cfg: ConfigToml = root_value.try_into().map_err(|e| {
        tracing::error!("Failed to deserialize overridden config: {e}");
        std::io::Error::new(std::io::ErrorKind::InvalidData, e)
    })?;

    assert_eq!(cfg.model.as_deref(), Some("managed_config"));
    Ok(())
}

#[tokio::test]
async fn load_global_mcp_servers_accepts_legacy_ms_field() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let config_path = codex_home.path().join(CONFIG_TOML_FILE);

    std::fs::write(
        &config_path,
        r#"
[mcp_servers]
[mcp_servers.docs]
command = "echo"
startup_timeout_ms = 2500
"#,
    )?;

    let servers = load_global_mcp_servers(codex_home.path()).await?;
    let docs = servers.get("docs").expect("docs entry");
    assert_eq!(docs.startup_timeout_sec, Some(Duration::from_millis(2500)));

    Ok(())
}

#[tokio::test]
async fn load_global_mcp_servers_rejects_inline_bearer_token() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let config_path = codex_home.path().join(CONFIG_TOML_FILE);

    std::fs::write(
        &config_path,
        r#"
[mcp_servers.docs]
url = "https://example.com/mcp"
bearer_token = "secret"
"#,
    )?;

    let err = load_global_mcp_servers(codex_home.path())
        .await
        .expect_err("bearer_token entries should be rejected");

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().contains("bearer_token"));
    assert!(err.to_string().contains("bearer_token_env_var"));

    Ok(())
}

#[tokio::test]
async fn replace_mcp_servers_serializes_env_sorted() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    let servers = BTreeMap::from([(
        "docs".to_string(),
        McpServerConfig {
            transport: McpServerTransportConfig::Stdio {
                command: "docs-server".to_string(),
                args: vec!["--verbose".to_string()],
                env: Some(HashMap::from([
                    ("ZIG_VAR".to_string(), "3".to_string()),
                    ("ALPHA_VAR".to_string(), "1".to_string()),
                ])),
                env_vars: Vec::new(),
                cwd: None,
            },
            enabled: true,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
        },
    )]);

    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(servers.clone())],
    )?;

    let config_path = codex_home.path().join(CONFIG_TOML_FILE);
    let serialized = std::fs::read_to_string(&config_path)?;
    assert_eq!(
        serialized,
        r#"[mcp_servers.docs]
command = "docs-server"
args = ["--verbose"]

[mcp_servers.docs.env]
ALPHA_VAR = "1"
ZIG_VAR = "3"
"#
    );

    let loaded = load_global_mcp_servers(codex_home.path()).await?;
    let docs = loaded.get("docs").expect("docs entry");
    match &docs.transport {
        McpServerTransportConfig::Stdio {
            command,
            args,
            env,
            env_vars,
            cwd,
        } => {
            assert_eq!(command, "docs-server");
            assert_eq!(args, &vec!["--verbose".to_string()]);
            let env = env
                .as_ref()
                .expect("env should be preserved for stdio transport");
            assert_eq!(env.get("ALPHA_VAR"), Some(&"1".to_string()));
            assert_eq!(env.get("ZIG_VAR"), Some(&"3".to_string()));
            assert!(env_vars.is_empty());
            assert!(cwd.is_none());
        }
        other => panic!("unexpected transport {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn replace_mcp_servers_serializes_env_vars() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    let servers = BTreeMap::from([(
        "docs".to_string(),
        McpServerConfig {
            transport: McpServerTransportConfig::Stdio {
                command: "docs-server".to_string(),
                args: Vec::new(),
                env: None,
                env_vars: vec!["ALPHA".to_string(), "BETA".to_string()],
                cwd: None,
            },
            enabled: true,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
        },
    )]);

    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(servers.clone())],
    )?;

    let config_path = codex_home.path().join(CONFIG_TOML_FILE);
    let serialized = std::fs::read_to_string(&config_path)?;
    assert!(
        serialized.contains(r#"env_vars = ["ALPHA", "BETA"]"#),
        "serialized config missing env_vars field:\n{serialized}"
    );

    let loaded = load_global_mcp_servers(codex_home.path()).await?;
    let docs = loaded.get("docs").expect("docs entry");
    match &docs.transport {
        McpServerTransportConfig::Stdio { env_vars, .. } => {
            assert_eq!(env_vars, &vec!["ALPHA".to_string(), "BETA".to_string()]);
        }
        other => panic!("unexpected transport {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn replace_mcp_servers_serializes_cwd() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    let cwd_path = PathBuf::from("/tmp/codex-mcp");
    let servers = BTreeMap::from([(
        "docs".to_string(),
        McpServerConfig {
            transport: McpServerTransportConfig::Stdio {
                command: "docs-server".to_string(),
                args: Vec::new(),
                env: None,
                env_vars: Vec::new(),
                cwd: Some(cwd_path.clone()),
            },
            enabled: true,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
        },
    )]);

    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(servers.clone())],
    )?;

    let config_path = codex_home.path().join(CONFIG_TOML_FILE);
    let serialized = std::fs::read_to_string(&config_path)?;
    assert!(
        serialized.contains(r#"cwd = "/tmp/codex-mcp""#),
        "serialized config missing cwd field:\n{serialized}"
    );

    let loaded = load_global_mcp_servers(codex_home.path()).await?;
    let docs = loaded.get("docs").expect("docs entry");
    match &docs.transport {
        McpServerTransportConfig::Stdio { cwd, .. } => {
            assert_eq!(cwd.as_deref(), Some(Path::new("/tmp/codex-mcp")));
        }
        other => panic!("unexpected transport {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn replace_mcp_servers_streamable_http_serializes_bearer_token() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    let servers = BTreeMap::from([(
        "docs".to_string(),
        McpServerConfig {
            transport: McpServerTransportConfig::StreamableHttp {
                url: "https://example.com/mcp".to_string(),
                bearer_token_env_var: Some("MCP_TOKEN".to_string()),
                http_headers: None,
                env_http_headers: None,
            },
            enabled: true,
            startup_timeout_sec: Some(Duration::from_secs(2)),
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
        },
    )]);

    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(servers.clone())],
    )?;

    let config_path = codex_home.path().join(CONFIG_TOML_FILE);
    let serialized = std::fs::read_to_string(&config_path)?;
    assert_eq!(
        serialized,
        r#"[mcp_servers.docs]
url = "https://example.com/mcp"
bearer_token_env_var = "MCP_TOKEN"
startup_timeout_sec = 2.0
"#
    );

    let loaded = load_global_mcp_servers(codex_home.path()).await?;
    let docs = loaded.get("docs").expect("docs entry");
    match &docs.transport {
        McpServerTransportConfig::StreamableHttp {
            url,
            bearer_token_env_var,
            http_headers,
            env_http_headers,
        } => {
            assert_eq!(url, "https://example.com/mcp");
            assert_eq!(bearer_token_env_var.as_deref(), Some("MCP_TOKEN"));
            assert!(http_headers.is_none());
            assert!(env_http_headers.is_none());
        }
        other => panic!("unexpected transport {other:?}"),
    }
    assert_eq!(docs.startup_timeout_sec, Some(Duration::from_secs(2)));

    Ok(())
}

#[tokio::test]
async fn replace_mcp_servers_streamable_http_serializes_custom_headers() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    let servers = BTreeMap::from([(
        "docs".to_string(),
        McpServerConfig {
            transport: McpServerTransportConfig::StreamableHttp {
                url: "https://example.com/mcp".to_string(),
                bearer_token_env_var: Some("MCP_TOKEN".to_string()),
                http_headers: Some(HashMap::from([("X-Doc".to_string(), "42".to_string())])),
                env_http_headers: Some(HashMap::from([(
                    "X-Auth".to_string(),
                    "DOCS_AUTH".to_string(),
                )])),
            },
            enabled: true,
            startup_timeout_sec: Some(Duration::from_secs(2)),
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
        },
    )]);
    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(servers.clone())],
    )?;

    let config_path = codex_home.path().join(CONFIG_TOML_FILE);
    let serialized = std::fs::read_to_string(&config_path)?;
    assert_eq!(
        serialized,
        r#"[mcp_servers.docs]
url = "https://example.com/mcp"
bearer_token_env_var = "MCP_TOKEN"
startup_timeout_sec = 2.0

[mcp_servers.docs.http_headers]
X-Doc = "42"

[mcp_servers.docs.env_http_headers]
X-Auth = "DOCS_AUTH"
"#
    );

    let loaded = load_global_mcp_servers(codex_home.path()).await?;
    let docs = loaded.get("docs").expect("docs entry");
    match &docs.transport {
        McpServerTransportConfig::StreamableHttp {
            http_headers,
            env_http_headers,
            ..
        } => {
            assert_eq!(
                http_headers,
                &Some(HashMap::from([("X-Doc".to_string(), "42".to_string())]))
            );
            assert_eq!(
                env_http_headers,
                &Some(HashMap::from([(
                    "X-Auth".to_string(),
                    "DOCS_AUTH".to_string()
                )]))
            );
        }
        other => panic!("unexpected transport {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn replace_mcp_servers_streamable_http_removes_optional_sections() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    let config_path = codex_home.path().join(CONFIG_TOML_FILE);

    let mut servers = BTreeMap::from([(
        "docs".to_string(),
        McpServerConfig {
            transport: McpServerTransportConfig::StreamableHttp {
                url: "https://example.com/mcp".to_string(),
                bearer_token_env_var: Some("MCP_TOKEN".to_string()),
                http_headers: Some(HashMap::from([("X-Doc".to_string(), "42".to_string())])),
                env_http_headers: Some(HashMap::from([(
                    "X-Auth".to_string(),
                    "DOCS_AUTH".to_string(),
                )])),
            },
            enabled: true,
            startup_timeout_sec: Some(Duration::from_secs(2)),
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
        },
    )]);

    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(servers.clone())],
    )?;
    let serialized_with_optional = std::fs::read_to_string(&config_path)?;
    assert!(serialized_with_optional.contains("bearer_token_env_var = \"MCP_TOKEN\""));
    assert!(serialized_with_optional.contains("[mcp_servers.docs.http_headers]"));
    assert!(serialized_with_optional.contains("[mcp_servers.docs.env_http_headers]"));

    servers.insert(
        "docs".to_string(),
        McpServerConfig {
            transport: McpServerTransportConfig::StreamableHttp {
                url: "https://example.com/mcp".to_string(),
                bearer_token_env_var: None,
                http_headers: None,
                env_http_headers: None,
            },
            enabled: true,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
        },
    );
    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(servers.clone())],
    )?;

    let serialized = std::fs::read_to_string(&config_path)?;
    assert_eq!(
        serialized,
        r#"[mcp_servers.docs]
url = "https://example.com/mcp"
"#
    );

    let loaded = load_global_mcp_servers(codex_home.path()).await?;
    let docs = loaded.get("docs").expect("docs entry");
    match &docs.transport {
        McpServerTransportConfig::StreamableHttp {
            url,
            bearer_token_env_var,
            http_headers,
            env_http_headers,
        } => {
            assert_eq!(url, "https://example.com/mcp");
            assert!(bearer_token_env_var.is_none());
            assert!(http_headers.is_none());
            assert!(env_http_headers.is_none());
        }
        other => panic!("unexpected transport {other:?}"),
    }

    assert!(docs.startup_timeout_sec.is_none());

    Ok(())
}

#[tokio::test]
async fn replace_mcp_servers_streamable_http_isolates_headers_between_servers() -> anyhow::Result<()>
{
    let codex_home = TempDir::new()?;
    let config_path = codex_home.path().join(CONFIG_TOML_FILE);

    let servers = BTreeMap::from([
        (
            "docs".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::StreamableHttp {
                    url: "https://example.com/mcp".to_string(),
                    bearer_token_env_var: Some("MCP_TOKEN".to_string()),
                    http_headers: Some(HashMap::from([("X-Doc".to_string(), "42".to_string())])),
                    env_http_headers: Some(HashMap::from([(
                        "X-Auth".to_string(),
                        "DOCS_AUTH".to_string(),
                    )])),
                },
                enabled: true,
                startup_timeout_sec: Some(Duration::from_secs(2)),
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        ),
        (
            "logs".to_string(),
            McpServerConfig {
                transport: McpServerTransportConfig::Stdio {
                    command: "logs-server".to_string(),
                    args: vec!["--follow".to_string()],
                    env: None,
                    env_vars: Vec::new(),
                    cwd: None,
                },
                enabled: true,
                startup_timeout_sec: None,
                tool_timeout_sec: None,
                enabled_tools: None,
                disabled_tools: None,
            },
        ),
    ]);

    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(servers.clone())],
    )?;

    let serialized = std::fs::read_to_string(&config_path)?;
    assert!(
        serialized.contains("[mcp_servers.docs.http_headers]"),
        "serialized config missing docs headers section:\n{serialized}"
    );
    assert!(
        !serialized.contains("[mcp_servers.logs.http_headers]"),
        "serialized config should not add logs headers section:\n{serialized}"
    );
    assert!(
        !serialized.contains("[mcp_servers.logs.env_http_headers]"),
        "serialized config should not add logs env headers section:\n{serialized}"
    );
    assert!(
        !serialized.contains("mcp_servers.logs.bearer_token_env_var"),
        "serialized config should not add bearer token to logs:\n{serialized}"
    );

    let loaded = load_global_mcp_servers(codex_home.path()).await?;
    let docs = loaded.get("docs").expect("docs entry");
    match &docs.transport {
        McpServerTransportConfig::StreamableHttp {
            http_headers,
            env_http_headers,
            ..
        } => {
            assert_eq!(
                http_headers,
                &Some(HashMap::from([("X-Doc".to_string(), "42".to_string())]))
            );
            assert_eq!(
                env_http_headers,
                &Some(HashMap::from([(
                    "X-Auth".to_string(),
                    "DOCS_AUTH".to_string()
                )]))
            );
        }
        other => panic!("unexpected transport {other:?}"),
    }
    let logs = loaded.get("logs").expect("logs entry");
    match &logs.transport {
        McpServerTransportConfig::Stdio { env, .. } => {
            assert!(env.is_none());
        }
        other => panic!("unexpected transport {other:?}"),
    }

    Ok(())
}

#[tokio::test]
async fn replace_mcp_servers_serializes_disabled_flag() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    let servers = BTreeMap::from([(
        "docs".to_string(),
        McpServerConfig {
            transport: McpServerTransportConfig::Stdio {
                command: "docs-server".to_string(),
                args: Vec::new(),
                env: None,
                env_vars: Vec::new(),
                cwd: None,
            },
            enabled: false,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            enabled_tools: None,
            disabled_tools: None,
        },
    )]);

    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(servers.clone())],
    )?;

    let config_path = codex_home.path().join(CONFIG_TOML_FILE);
    let serialized = std::fs::read_to_string(&config_path)?;
    assert!(
        serialized.contains("enabled = false"),
        "serialized config missing disabled flag:\n{serialized}"
    );

    let loaded = load_global_mcp_servers(codex_home.path()).await?;
    let docs = loaded.get("docs").expect("docs entry");
    assert!(!docs.enabled);

    Ok(())
}

#[tokio::test]
async fn replace_mcp_servers_serializes_tool_filters() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    let servers = BTreeMap::from([(
        "docs".to_string(),
        McpServerConfig {
            transport: McpServerTransportConfig::Stdio {
                command: "docs-server".to_string(),
                args: Vec::new(),
                env: None,
                env_vars: Vec::new(),
                cwd: None,
            },
            enabled: true,
            startup_timeout_sec: None,
            tool_timeout_sec: None,
            enabled_tools: Some(vec!["allowed".to_string()]),
            disabled_tools: Some(vec!["blocked".to_string()]),
        },
    )]);

    apply_blocking(
        codex_home.path(),
        None,
        &[ConfigEdit::ReplaceMcpServers(servers.clone())],
    )?;

    let config_path = codex_home.path().join(CONFIG_TOML_FILE);
    let serialized = std::fs::read_to_string(&config_path)?;
    assert!(serialized.contains(r#"enabled_tools = ["allowed"]"#));
    assert!(serialized.contains(r#"disabled_tools = ["blocked"]"#));

    let loaded = load_global_mcp_servers(codex_home.path()).await?;
    let docs = loaded.get("docs").expect("docs entry");
    assert_eq!(
        docs.enabled_tools.as_ref(),
        Some(&vec!["allowed".to_string()])
    );
    assert_eq!(
        docs.disabled_tools.as_ref(),
        Some(&vec!["blocked".to_string()])
    );

    Ok(())
}

#[tokio::test]
async fn set_model_updates_defaults() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    ConfigEditsBuilder::new(codex_home.path())
        .set_model(Some("gpt-5.1-codex"), Some(ReasoningEffort::High))
        .apply()
        .await?;

    let serialized = tokio::fs::read_to_string(codex_home.path().join(CONFIG_TOML_FILE)).await?;
    let parsed: ConfigToml = toml::from_str(&serialized)?;

    assert_eq!(parsed.model.as_deref(), Some("gpt-5.1-codex"));
    assert_eq!(parsed.model_reasoning_effort, Some(ReasoningEffort::High));

    Ok(())
}

#[tokio::test]
async fn set_model_overwrites_existing_model() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let config_path = codex_home.path().join(CONFIG_TOML_FILE);

    tokio::fs::write(
        &config_path,
        r#"
model = "gpt-5.1-codex"
model_reasoning_effort = "medium"

[profiles.dev]
model = "gpt-4.1"
"#,
    )
    .await?;

    ConfigEditsBuilder::new(codex_home.path())
        .set_model(Some("o4-mini"), Some(ReasoningEffort::High))
        .apply()
        .await?;

    let serialized = tokio::fs::read_to_string(config_path).await?;
    let parsed: ConfigToml = toml::from_str(&serialized)?;

    assert_eq!(parsed.model.as_deref(), Some("o4-mini"));
    assert_eq!(parsed.model_reasoning_effort, Some(ReasoningEffort::High));
    assert_eq!(
        parsed
            .profiles
            .get("dev")
            .and_then(|profile| profile.model.as_deref()),
        Some("gpt-4.1"),
    );

    Ok(())
}

#[tokio::test]
async fn set_model_updates_profile() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;

    ConfigEditsBuilder::new(codex_home.path())
        .with_profile(Some("dev"))
        .set_model(Some("gpt-5.1-codex"), Some(ReasoningEffort::Medium))
        .apply()
        .await?;

    let serialized = tokio::fs::read_to_string(codex_home.path().join(CONFIG_TOML_FILE)).await?;
    let parsed: ConfigToml = toml::from_str(&serialized)?;
    let profile = parsed
        .profiles
        .get("dev")
        .expect("profile should be created");

    assert_eq!(profile.model.as_deref(), Some("gpt-5.1-codex"));
    assert_eq!(
        profile.model_reasoning_effort,
        Some(ReasoningEffort::Medium)
    );

    Ok(())
}

#[tokio::test]
async fn set_model_updates_existing_profile() -> anyhow::Result<()> {
    let codex_home = TempDir::new()?;
    let config_path = codex_home.path().join(CONFIG_TOML_FILE);

    tokio::fs::write(
        &config_path,
        r#"
[profiles.dev]
model = "gpt-4"
model_reasoning_effort = "medium"

[profiles.prod]
model = "gpt-5.1-codex"
"#,
    )
    .await?;

    ConfigEditsBuilder::new(codex_home.path())
        .with_profile(Some("dev"))
        .set_model(Some("o4-high"), Some(ReasoningEffort::Medium))
        .apply()
        .await?;

    let serialized = tokio::fs::read_to_string(config_path).await?;
    let parsed: ConfigToml = toml::from_str(&serialized)?;

    let dev_profile = parsed
        .profiles
        .get("dev")
        .expect("dev profile should survive updates");
    assert_eq!(dev_profile.model.as_deref(), Some("o4-high"));
    assert_eq!(
        dev_profile.model_reasoning_effort,
        Some(ReasoningEffort::Medium)
    );

    assert_eq!(
        parsed
            .profiles
            .get("prod")
            .and_then(|profile| profile.model.as_deref()),
        Some("gpt-5.1-codex"),
    );

    Ok(())
}

struct PrecedenceTestFixture {
    cwd: TempDir,
    codex_home: TempDir,
    cfg: ConfigToml,
    model_provider_map: HashMap<String, ModelProviderInfo>,
    openai_provider: ModelProviderInfo,
    openai_chat_completions_provider: ModelProviderInfo,
}

impl PrecedenceTestFixture {
    fn cwd(&self) -> PathBuf {
        self.cwd.path().to_path_buf()
    }

    fn codex_home(&self) -> PathBuf {
        self.codex_home.path().to_path_buf()
    }
}

#[test]
fn cli_override_sets_compact_prompt() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let overrides = ConfigOverrides {
        compact_prompt: Some("Use the compact override".to_string()),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        ConfigToml::default(),
        overrides,
        codex_home.path().to_path_buf(),
    )?;

    assert_eq!(
        config.compact_prompt.as_deref(),
        Some("Use the compact override")
    );

    Ok(())
}

#[test]
fn loads_compact_prompt_from_file() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let workspace = codex_home.path().join("workspace");
    std::fs::create_dir_all(&workspace)?;

    let prompt_path = workspace.join("compact_prompt.txt");
    std::fs::write(&prompt_path, "  summarize differently  ")?;

    let cfg = ConfigToml {
        experimental_compact_prompt_file: Some(PathBuf::from("compact_prompt.txt")),
        ..Default::default()
    };

    let overrides = ConfigOverrides {
        cwd: Some(workspace),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        overrides,
        codex_home.path().to_path_buf(),
    )?;

    assert_eq!(
        config.compact_prompt.as_deref(),
        Some("summarize differently")
    );

    Ok(())
}

fn create_test_fixture() -> std::io::Result<PrecedenceTestFixture> {
    let toml = r#"
model = "o3"
approval_policy = "untrusted"

# Can be used to determine which profile to use if not specified by
# `ConfigOverrides`.
profile = "gpt3"

[model_providers.openai-chat-completions]
name = "OpenAI using Chat Completions"
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"
wire_api = "chat"
request_max_retries = 4            # retry failed HTTP requests
stream_max_retries = 10            # retry dropped SSE streams
stream_idle_timeout_ms = 300000    # 5m idle timeout

[profiles.o3]
model = "o3"
model_provider = "openai"
approval_policy = "never"
model_reasoning_effort = "high"
model_reasoning_summary = "detailed"

[profiles.gpt3]
model = "gpt-3.5-turbo"
model_provider = "openai-chat-completions"

[profiles.zdr]
model = "o3"
model_provider = "openai"
approval_policy = "on-failure"

[profiles.gpt5]
model = "gpt-5.1"
model_provider = "openai"
approval_policy = "on-failure"
model_reasoning_effort = "high"
model_reasoning_summary = "detailed"
model_verbosity = "high"
"#;

    let cfg: ConfigToml = toml::from_str(toml).expect("TOML deserialization should succeed");

    // Use a temporary directory for the cwd so it does not contain an
    // AGENTS.md file.
    let cwd_temp_dir = TempDir::new().unwrap();
    let cwd = cwd_temp_dir.path().to_path_buf();
    // Make it look like a Git repo so it does not search for AGENTS.md in
    // a parent folder, either.
    std::fs::write(cwd.join(".git"), "gitdir: nowhere")?;

    let codex_home_temp_dir = TempDir::new().unwrap();

    let openai_chat_completions_provider = ModelProviderInfo {
        name: "OpenAI using Chat Completions".to_string(),
        base_url: Some("https://api.openai.com/v1".to_string()),
        env_key: Some("OPENAI_API_KEY".to_string()),
        wire_api: crate::WireApi::Chat,
        env_key_instructions: None,
        experimental_bearer_token: None,
        query_params: None,
        http_headers: None,
        env_http_headers: None,
        request_max_retries: Some(4),
        stream_max_retries: Some(10),
        stream_idle_timeout_ms: Some(300_000),
        requires_openai_auth: false,
    };
    let model_provider_map = {
        let mut model_provider_map = built_in_model_providers();
        model_provider_map.insert(
            "openai-chat-completions".to_string(),
            openai_chat_completions_provider.clone(),
        );
        model_provider_map
    };

    let openai_provider = model_provider_map
        .get("openai")
        .expect("openai provider should exist")
        .clone();

    Ok(PrecedenceTestFixture {
        cwd: cwd_temp_dir,
        codex_home: codex_home_temp_dir,
        cfg,
        model_provider_map,
        openai_provider,
        openai_chat_completions_provider,
    })
}

/// Users can specify config values at multiple levels that have the
/// following precedence:
///
/// 1. custom command-line argument, e.g. `--model o3`
/// 2. as part of a profile, where the `--profile` is specified via a CLI
///    (or in the config file itself)
/// 3. as an entry in `config.toml`, e.g. `model = "o3"`
/// 4. the default value for a required field defined in code, e.g.,
///    `crate::flags::OPENAI_DEFAULT_MODEL`
///
/// Note that profiles are the recommended way to specify a group of
/// configuration options together.
#[test]
fn test_precedence_fixture_with_o3_profile() -> std::io::Result<()> {
    let fixture = create_test_fixture()?;

    let o3_profile_overrides = ConfigOverrides {
        config_profile: Some("o3".to_string()),
        cwd: Some(fixture.cwd()),
        ..Default::default()
    };
    let o3_profile_config: Config = Config::load_from_base_config_with_overrides(
        fixture.cfg.clone(),
        o3_profile_overrides,
        fixture.codex_home(),
    )?;
    assert_eq!(
        Config {
            model: "o3".to_string(),
            model_family: find_family_for_model("o3").expect("known model slug"),
            model_context_window: Some(200_000),
            model_auto_compact_token_limit: Some(180_000),
            model_provider_id: "openai".to_string(),
            model_provider: fixture.openai_provider.clone(),
            approval_policy: AskForApproval::Never,
            sandbox_policy: SandboxPolicy::new_read_only_policy(),
            did_user_set_custom_approval_policy_or_sandbox_mode: true,
            forced_auto_mode_downgraded_on_windows: false,
            shell_environment_policy: ShellEnvironmentPolicy::default(),
            user_instructions: None,
            notify: None,
            cwd: fixture.cwd(),
            cli_auth_credentials_store_mode: Default::default(),
            mcp_servers: HashMap::new(),
            mcp_oauth_credentials_store_mode: Default::default(),
            model_providers: fixture.model_provider_map.clone(),
            project_doc_max_bytes: PROJECT_DOC_MAX_BYTES,
            project_doc_fallback_filenames: Vec::new(),
            tool_output_token_limit: None,
            codex_home: fixture.codex_home(),
            history: History::default(),
            file_opener: UriBasedFileOpener::VsCode,
            codex_linux_sandbox_exe: None,
            hide_agent_reasoning: false,
            show_raw_agent_reasoning: false,
            model_reasoning_effort: Some(ReasoningEffort::High),
            model_reasoning_summary: ReasoningSummary::Detailed,
            model_verbosity: None,
            chatgpt_base_url: "https://chatgpt.com/backend-api/".to_string(),
            base_instructions: None,
            developer_instructions: None,
            compact_prompt: None,
            forced_chatgpt_workspace_id: None,
            forced_login_method: None,
            include_apply_patch_tool: false,
            tools_web_search_request: false,
            experimental_sandbox_command_assessment: false,
            use_experimental_unified_exec_tool: false,
            use_experimental_use_rmcp_client: false,
            features: Features::with_defaults(),
            active_profile: Some("o3".to_string()),
            active_project: ProjectConfig { trust_level: None },
            windows_wsl_setup_acknowledged: false,
            notices: Default::default(),
            check_for_update_on_startup: true,
            disable_paste_burst: false,
            tui_notifications: true,
            animations: true,
            otel: OtelConfig::default(),
            acp_allow_http_fallback: false,
        },
        o3_profile_config
    );
    Ok(())
}

#[test]
fn test_precedence_fixture_with_gpt3_profile() -> std::io::Result<()> {
    let fixture = create_test_fixture()?;

    let gpt3_profile_overrides = ConfigOverrides {
        config_profile: Some("gpt3".to_string()),
        cwd: Some(fixture.cwd()),
        ..Default::default()
    };
    let gpt3_profile_config = Config::load_from_base_config_with_overrides(
        fixture.cfg.clone(),
        gpt3_profile_overrides,
        fixture.codex_home(),
    )?;
    let expected_gpt3_profile_config = Config {
        model: "gpt-3.5-turbo".to_string(),
        model_family: find_family_for_model("gpt-3.5-turbo").expect("known model slug"),
        model_context_window: Some(16_385),
        model_auto_compact_token_limit: Some(14_746),
        model_provider_id: "openai-chat-completions".to_string(),
        model_provider: fixture.openai_chat_completions_provider.clone(),
        approval_policy: AskForApproval::UnlessTrusted,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        did_user_set_custom_approval_policy_or_sandbox_mode: true,
        forced_auto_mode_downgraded_on_windows: false,
        shell_environment_policy: ShellEnvironmentPolicy::default(),
        user_instructions: None,
        notify: None,
        cwd: fixture.cwd(),
        cli_auth_credentials_store_mode: Default::default(),
        mcp_servers: HashMap::new(),
        mcp_oauth_credentials_store_mode: Default::default(),
        model_providers: fixture.model_provider_map.clone(),
        project_doc_max_bytes: PROJECT_DOC_MAX_BYTES,
        project_doc_fallback_filenames: Vec::new(),
        tool_output_token_limit: None,
        codex_home: fixture.codex_home(),
        history: History::default(),
        file_opener: UriBasedFileOpener::VsCode,
        codex_linux_sandbox_exe: None,
        hide_agent_reasoning: false,
        show_raw_agent_reasoning: false,
        model_reasoning_effort: None,
        model_reasoning_summary: ReasoningSummary::default(),
        model_verbosity: None,
        chatgpt_base_url: "https://chatgpt.com/backend-api/".to_string(),
        base_instructions: None,
        developer_instructions: None,
        compact_prompt: None,
        forced_chatgpt_workspace_id: None,
        forced_login_method: None,
        include_apply_patch_tool: false,
        tools_web_search_request: false,
        experimental_sandbox_command_assessment: false,
        use_experimental_unified_exec_tool: false,
        use_experimental_use_rmcp_client: false,
        features: Features::with_defaults(),
        active_profile: Some("gpt3".to_string()),
        active_project: ProjectConfig { trust_level: None },
        windows_wsl_setup_acknowledged: false,
        notices: Default::default(),
        check_for_update_on_startup: true,
        disable_paste_burst: false,
        tui_notifications: true,
        animations: true,
        otel: OtelConfig::default(),
        acp_allow_http_fallback: false,
    };

    assert_eq!(expected_gpt3_profile_config, gpt3_profile_config);

    // Verify that loading without specifying a profile in ConfigOverrides
    // uses the default profile from the config file (which is "gpt3").
    let default_profile_overrides = ConfigOverrides {
        cwd: Some(fixture.cwd()),
        ..Default::default()
    };

    let default_profile_config = Config::load_from_base_config_with_overrides(
        fixture.cfg.clone(),
        default_profile_overrides,
        fixture.codex_home(),
    )?;

    assert_eq!(expected_gpt3_profile_config, default_profile_config);
    Ok(())
}

#[test]
fn test_precedence_fixture_with_zdr_profile() -> std::io::Result<()> {
    let fixture = create_test_fixture()?;

    let zdr_profile_overrides = ConfigOverrides {
        config_profile: Some("zdr".to_string()),
        cwd: Some(fixture.cwd()),
        ..Default::default()
    };
    let zdr_profile_config = Config::load_from_base_config_with_overrides(
        fixture.cfg.clone(),
        zdr_profile_overrides,
        fixture.codex_home(),
    )?;
    let expected_zdr_profile_config = Config {
        model: "o3".to_string(),
        model_family: find_family_for_model("o3").expect("known model slug"),
        model_context_window: Some(200_000),
        model_auto_compact_token_limit: Some(180_000),
        model_provider_id: "openai".to_string(),
        model_provider: fixture.openai_provider.clone(),
        approval_policy: AskForApproval::OnFailure,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        did_user_set_custom_approval_policy_or_sandbox_mode: true,
        forced_auto_mode_downgraded_on_windows: false,
        shell_environment_policy: ShellEnvironmentPolicy::default(),
        user_instructions: None,
        notify: None,
        cwd: fixture.cwd(),
        cli_auth_credentials_store_mode: Default::default(),
        mcp_servers: HashMap::new(),
        mcp_oauth_credentials_store_mode: Default::default(),
        model_providers: fixture.model_provider_map.clone(),
        project_doc_max_bytes: PROJECT_DOC_MAX_BYTES,
        project_doc_fallback_filenames: Vec::new(),
        tool_output_token_limit: None,
        codex_home: fixture.codex_home(),
        history: History::default(),
        file_opener: UriBasedFileOpener::VsCode,
        codex_linux_sandbox_exe: None,
        hide_agent_reasoning: false,
        show_raw_agent_reasoning: false,
        model_reasoning_effort: None,
        model_reasoning_summary: ReasoningSummary::default(),
        model_verbosity: None,
        chatgpt_base_url: "https://chatgpt.com/backend-api/".to_string(),
        base_instructions: None,
        developer_instructions: None,
        compact_prompt: None,
        forced_chatgpt_workspace_id: None,
        forced_login_method: None,
        include_apply_patch_tool: false,
        tools_web_search_request: false,
        experimental_sandbox_command_assessment: false,
        use_experimental_unified_exec_tool: false,
        use_experimental_use_rmcp_client: false,
        features: Features::with_defaults(),
        active_profile: Some("zdr".to_string()),
        active_project: ProjectConfig { trust_level: None },
        windows_wsl_setup_acknowledged: false,
        notices: Default::default(),
        check_for_update_on_startup: true,
        disable_paste_burst: false,
        tui_notifications: true,
        animations: true,
        otel: OtelConfig::default(),
        acp_allow_http_fallback: false,
    };

    assert_eq!(expected_zdr_profile_config, zdr_profile_config);

    Ok(())
}

#[test]
fn test_precedence_fixture_with_gpt5_profile() -> std::io::Result<()> {
    let fixture = create_test_fixture()?;

    let gpt5_profile_overrides = ConfigOverrides {
        config_profile: Some("gpt5".to_string()),
        cwd: Some(fixture.cwd()),
        ..Default::default()
    };
    let gpt5_profile_config = Config::load_from_base_config_with_overrides(
        fixture.cfg.clone(),
        gpt5_profile_overrides,
        fixture.codex_home(),
    )?;
    let expected_gpt5_profile_config = Config {
        model: "gpt-5.1".to_string(),
        model_family: find_family_for_model("gpt-5.1").expect("known model slug"),
        model_context_window: Some(272_000),
        model_auto_compact_token_limit: Some(244_800),
        model_provider_id: "openai".to_string(),
        model_provider: fixture.openai_provider.clone(),
        approval_policy: AskForApproval::OnFailure,
        sandbox_policy: SandboxPolicy::new_read_only_policy(),
        did_user_set_custom_approval_policy_or_sandbox_mode: true,
        forced_auto_mode_downgraded_on_windows: false,
        shell_environment_policy: ShellEnvironmentPolicy::default(),
        user_instructions: None,
        notify: None,
        cwd: fixture.cwd(),
        cli_auth_credentials_store_mode: Default::default(),
        mcp_servers: HashMap::new(),
        mcp_oauth_credentials_store_mode: Default::default(),
        model_providers: fixture.model_provider_map.clone(),
        project_doc_max_bytes: PROJECT_DOC_MAX_BYTES,
        project_doc_fallback_filenames: Vec::new(),
        tool_output_token_limit: None,
        codex_home: fixture.codex_home(),
        history: History::default(),
        file_opener: UriBasedFileOpener::VsCode,
        codex_linux_sandbox_exe: None,
        hide_agent_reasoning: false,
        show_raw_agent_reasoning: false,
        model_reasoning_effort: Some(ReasoningEffort::High),
        model_reasoning_summary: ReasoningSummary::Detailed,
        model_verbosity: Some(Verbosity::High),
        chatgpt_base_url: "https://chatgpt.com/backend-api/".to_string(),
        base_instructions: None,
        developer_instructions: None,
        compact_prompt: None,
        forced_chatgpt_workspace_id: None,
        forced_login_method: None,
        include_apply_patch_tool: false,
        tools_web_search_request: false,
        experimental_sandbox_command_assessment: false,
        use_experimental_unified_exec_tool: false,
        use_experimental_use_rmcp_client: false,
        features: Features::with_defaults(),
        active_profile: Some("gpt5".to_string()),
        active_project: ProjectConfig { trust_level: None },
        windows_wsl_setup_acknowledged: false,
        notices: Default::default(),
        check_for_update_on_startup: true,
        disable_paste_burst: false,
        tui_notifications: true,
        animations: true,
        otel: OtelConfig::default(),
        acp_allow_http_fallback: false,
    };

    assert_eq!(expected_gpt5_profile_config, gpt5_profile_config);

    Ok(())
}

#[test]
fn test_did_user_set_custom_approval_policy_or_sandbox_mode_defaults_no() -> anyhow::Result<()> {
    let fixture = create_test_fixture()?;

    let config = Config::load_from_base_config_with_overrides(
        fixture.cfg.clone(),
        ConfigOverrides {
            ..Default::default()
        },
        fixture.codex_home(),
    )?;

    assert!(config.did_user_set_custom_approval_policy_or_sandbox_mode);

    Ok(())
}

#[test]
fn test_set_project_trusted_writes_explicit_tables() -> anyhow::Result<()> {
    let project_dir = Path::new("/some/path");
    let mut doc = DocumentMut::new();

    set_project_trust_level_inner(&mut doc, project_dir, TrustLevel::Trusted)?;

    let contents = doc.to_string();

    let raw_path = project_dir.to_string_lossy();
    let path_str = if raw_path.contains('\\') {
        format!("'{raw_path}'")
    } else {
        format!("\"{raw_path}\"")
    };
    let expected = format!(
        r#"[projects.{path_str}]
trust_level = "trusted"
"#
    );
    assert_eq!(contents, expected);

    Ok(())
}

#[test]
fn test_set_project_trusted_converts_inline_to_explicit() -> anyhow::Result<()> {
    let project_dir = Path::new("/some/path");

    // Seed config.toml with an inline project entry under [projects]
    let raw_path = project_dir.to_string_lossy();
    let path_str = if raw_path.contains('\\') {
        format!("'{raw_path}'")
    } else {
        format!("\"{raw_path}\"")
    };
    // Use a quoted key so backslashes don't require escaping on Windows
    let initial = format!(
        r#"[projects]
{path_str} = {{ trust_level = "untrusted" }}
"#
    );
    let mut doc = initial.parse::<DocumentMut>()?;

    // Run the function; it should convert to explicit tables and set trusted
    set_project_trust_level_inner(&mut doc, project_dir, TrustLevel::Trusted)?;

    let contents = doc.to_string();

    // Assert exact output after conversion to explicit table
    let expected = format!(
        r#"[projects]

[projects.{path_str}]
trust_level = "trusted"
"#
    );
    assert_eq!(contents, expected);

    Ok(())
}

#[test]
fn test_set_project_trusted_migrates_top_level_inline_projects_preserving_entries()
-> anyhow::Result<()> {
    let initial = r#"toplevel = "baz"
projects = { "/Users/mbolin/code/codex4" = { trust_level = "trusted", foo = "bar" } , "/Users/mbolin/code/codex3" = { trust_level = "trusted" } }
model = "foo""#;
    let mut doc = initial.parse::<DocumentMut>()?;

    // Approve a new directory
    let new_project = Path::new("/Users/mbolin/code/codex2");
    set_project_trust_level_inner(&mut doc, new_project, TrustLevel::Trusted)?;

    let contents = doc.to_string();

    // Since we created the [projects] table as part of migration, it is kept implicit.
    // Expect explicit per-project tables, preserving prior entries and appending the new one.
    let expected = r#"toplevel = "baz"
model = "foo"

[projects."/Users/mbolin/code/codex4"]
trust_level = "trusted"
foo = "bar"

[projects."/Users/mbolin/code/codex3"]
trust_level = "trusted"

[projects."/Users/mbolin/code/codex2"]
trust_level = "trusted"
"#;
    assert_eq!(contents, expected);

    Ok(())
}

#[test]
fn test_set_default_oss_provider() -> std::io::Result<()> {
    let temp_dir = TempDir::new()?;
    let codex_home = temp_dir.path();
    let config_path = codex_home.join(CONFIG_TOML_FILE);

    // Test setting valid provider on empty config
    set_default_oss_provider(codex_home, OLLAMA_OSS_PROVIDER_ID)?;
    let content = std::fs::read_to_string(&config_path)?;
    assert!(content.contains("oss_provider = \"ollama\""));

    // Test updating existing config
    std::fs::write(&config_path, "model = \"gpt-4\"\n")?;
    set_default_oss_provider(codex_home, LMSTUDIO_OSS_PROVIDER_ID)?;
    let content = std::fs::read_to_string(&config_path)?;
    assert!(content.contains("oss_provider = \"lmstudio\""));
    assert!(content.contains("model = \"gpt-4\""));

    // Test overwriting existing oss_provider
    set_default_oss_provider(codex_home, OLLAMA_OSS_PROVIDER_ID)?;
    let content = std::fs::read_to_string(&config_path)?;
    assert!(content.contains("oss_provider = \"ollama\""));
    assert!(!content.contains("oss_provider = \"lmstudio\""));

    // Test invalid provider
    let result = set_default_oss_provider(codex_home, "invalid_provider");
    assert!(result.is_err());
    let error = result.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    assert!(error.to_string().contains("Invalid OSS provider"));
    assert!(error.to_string().contains("invalid_provider"));

    Ok(())
}

#[test]
fn test_untrusted_project_gets_workspace_write_sandbox() -> anyhow::Result<()> {
    let config_with_untrusted = r#"
[projects."/tmp/test"]
trust_level = "untrusted"
"#;

    let cfg = toml::from_str::<ConfigToml>(config_with_untrusted)
        .expect("TOML deserialization should succeed");

    let resolution = cfg.derive_sandbox_policy(None, None, &PathBuf::from("/tmp/test"));

    // Verify that untrusted projects get WorkspaceWrite (or ReadOnly on Windows due to downgrade)
    if cfg!(target_os = "windows") {
        assert!(
            matches!(resolution.policy, SandboxPolicy::ReadOnly),
            "Expected ReadOnly on Windows, got {:?}",
            resolution.policy
        );
    } else {
        assert!(
            matches!(resolution.policy, SandboxPolicy::WorkspaceWrite { .. }),
            "Expected WorkspaceWrite for untrusted project, got {:?}",
            resolution.policy
        );
    }

    Ok(())
}

#[test]
fn test_resolve_oss_provider_explicit_override() {
    let config_toml = ConfigToml::default();
    let result = resolve_oss_provider(Some("custom-provider"), &config_toml, None);
    assert_eq!(result, Some("custom-provider".to_string()));
}

#[test]
fn test_resolve_oss_provider_from_profile() {
    let mut profiles = std::collections::HashMap::new();
    let profile = ConfigProfile {
        oss_provider: Some("profile-provider".to_string()),
        ..Default::default()
    };
    profiles.insert("test-profile".to_string(), profile);
    let config_toml = ConfigToml {
        profiles,
        ..Default::default()
    };

    let result = resolve_oss_provider(None, &config_toml, Some("test-profile".to_string()));
    assert_eq!(result, Some("profile-provider".to_string()));
}

#[test]
fn test_resolve_oss_provider_from_global_config() {
    let config_toml = ConfigToml {
        oss_provider: Some("global-provider".to_string()),
        ..Default::default()
    };

    let result = resolve_oss_provider(None, &config_toml, None);
    assert_eq!(result, Some("global-provider".to_string()));
}

#[test]
fn test_resolve_oss_provider_profile_fallback_to_global() {
    let mut profiles = std::collections::HashMap::new();
    let profile = ConfigProfile::default(); // No oss_provider set
    profiles.insert("test-profile".to_string(), profile);
    let config_toml = ConfigToml {
        oss_provider: Some("global-provider".to_string()),
        profiles,
        ..Default::default()
    };

    let result = resolve_oss_provider(None, &config_toml, Some("test-profile".to_string()));
    assert_eq!(result, Some("global-provider".to_string()));
}

#[test]
fn test_resolve_oss_provider_none_when_not_configured() {
    let config_toml = ConfigToml::default();
    let result = resolve_oss_provider(None, &config_toml, None);
    assert_eq!(result, None);
}

#[test]
fn test_resolve_oss_provider_explicit_overrides_all() {
    let mut profiles = std::collections::HashMap::new();
    let profile = ConfigProfile {
        oss_provider: Some("profile-provider".to_string()),
        ..Default::default()
    };
    profiles.insert("test-profile".to_string(), profile);
    let config_toml = ConfigToml {
        oss_provider: Some("global-provider".to_string()),
        profiles,
        ..Default::default()
    };

    let result = resolve_oss_provider(
        Some("explicit-provider"),
        &config_toml,
        Some("test-profile".to_string()),
    );
    assert_eq!(result, Some("explicit-provider".to_string()));
}

#[test]
fn test_untrusted_project_gets_unless_trusted_approval_policy() -> std::io::Result<()> {
    let codex_home = TempDir::new()?;
    let test_project_dir = TempDir::new()?;
    let test_path = test_project_dir.path();

    let mut projects = std::collections::HashMap::new();
    projects.insert(
        test_path.to_string_lossy().to_string(),
        ProjectConfig {
            trust_level: Some(TrustLevel::Untrusted),
        },
    );

    let cfg = ConfigToml {
        projects: Some(projects),
        ..Default::default()
    };

    let config = Config::load_from_base_config_with_overrides(
        cfg,
        ConfigOverrides {
            cwd: Some(test_path.to_path_buf()),
            ..Default::default()
        },
        codex_home.path().to_path_buf(),
    )?;

    // Verify that untrusted projects get UnlessTrusted approval policy
    assert_eq!(
        config.approval_policy,
        AskForApproval::UnlessTrusted,
        "Expected UnlessTrusted approval policy for untrusted project"
    );

    // Verify that untrusted projects still get WorkspaceWrite sandbox (or ReadOnly on Windows)
    if cfg!(target_os = "windows") {
        assert!(
            matches!(config.sandbox_policy, SandboxPolicy::ReadOnly),
            "Expected ReadOnly on Windows"
        );
    } else {
        assert!(
            matches!(config.sandbox_policy, SandboxPolicy::WorkspaceWrite { .. }),
            "Expected WorkspaceWrite sandbox for untrusted project"
        );
    }

    Ok(())
}
