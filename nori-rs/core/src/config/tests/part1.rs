use super::*;
use pretty_assertions::assert_eq;

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
