use super::*;
use pretty_assertions::assert_eq;

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
