use codex_protocol::config_types::SandboxMode;
use codex_protocol::config_types::TrustLevel;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::SandboxPolicy;
use nori_config::NoriConfig;
use nori_config::NoriConfigEdits;
use nori_config::NoriConfigOverrides;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

fn git(repo: &std::path::Path, args: &[&str]) -> std::io::Result<()> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()?;
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn create_git_repository(home: &TempDir) -> std::io::Result<std::path::PathBuf> {
    let repo = home.path().join("repository");
    std::fs::create_dir(&repo)?;
    git(&repo, &["init"])?;
    git(
        &repo,
        &["config", "user.email", "config-tests@nori.invalid"],
    )?;
    git(&repo, &["config", "user.name", "Nori Config Tests"])?;
    std::fs::write(repo.join("README.md"), "test repository\n")?;
    git(&repo, &["add", "README.md"])?;
    git(&repo, &["commit", "-m", "initial"])?;
    Ok(repo)
}

#[test]
fn typed_overrides_win_over_raw_overrides_and_user_config() {
    let home = TempDir::new().expect("create config home");
    let config_path = home.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"agent = "gemini"
approval_policy = "untrusted"
sandbox_mode = "read-only"
"#,
    )
    .expect("write config");

    let cwd = home.path().join("workspace");
    let extra_root = home.path().join("shared");
    std::fs::create_dir_all(&cwd).expect("create cwd");
    std::fs::create_dir_all(&extra_root).expect("create extra root");

    let config = NoriConfig::load_from_path_with_overrides(
        &config_path,
        NoriConfigOverrides {
            agent: Some("claude-code".to_string()),
            approval_policy: Some(AskForApproval::OnFailure),
            sandbox_mode: Some(SandboxMode::WorkspaceWrite),
            cwd: Some(cwd),
            additional_writable_roots: vec![extra_root.clone()],
            raw_overrides: vec![
                (
                    "agent".to_string(),
                    toml::Value::String("codex".to_string()),
                ),
                (
                    "approval_policy".to_string(),
                    toml::Value::String("never".to_string()),
                ),
            ],
        },
    )
    .expect("load config");

    assert_eq!(config.active_agent, "claude-code");
    assert_eq!(config.approval_policy, AskForApproval::OnFailure);
    let SandboxPolicy::WorkspaceWrite { writable_roots, .. } = config.sandbox_policy else {
        panic!("expected workspace-write sandbox policy");
    };
    assert_eq!(writable_roots, vec![extra_root]);
}

#[test]
fn raw_overrides_win_over_user_config_without_a_typed_override() {
    let home = TempDir::new().expect("create config home");
    let config_path = home.path().join("config.toml");
    std::fs::write(&config_path, "agent = \"gemini\"\n").expect("write config");

    let config = NoriConfig::load_from_path_with_overrides(
        &config_path,
        NoriConfigOverrides {
            raw_overrides: vec![(
                "agent".to_string(),
                toml::Value::String("codex".to_string()),
            )],
            ..NoriConfigOverrides::default()
        },
    )
    .expect("load config");

    assert_eq!(config.active_agent, "codex");
}

#[test]
fn edits_preserve_comments_and_round_trip_through_the_loader() {
    let home = TempDir::new().expect("create config home");
    let config_path = home.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"# keep this explanation
agent = "gemini"

[tui]
animations = false # and this inline note
"#,
    )
    .expect("write config");

    let project = home.path().join("project");
    std::fs::create_dir_all(&project).expect("create project");

    NoriConfigEdits::new(home.path())
        .set_agent("codex")
        .set_default_model("codex", "gpt-5")
        .set_project_trust_level(&project, TrustLevel::Trusted)
        .apply_blocking()
        .expect("persist edits");

    let contents = std::fs::read_to_string(&config_path).expect("read edited config");
    assert!(contents.contains("# keep this explanation"));
    assert!(contents.contains("animations = false # and this inline note"));

    let config = NoriConfig::load_from_path_with_overrides(
        &config_path,
        NoriConfigOverrides {
            cwd: Some(project),
            ..NoriConfigOverrides::default()
        },
    )
    .expect("reload edited config");

    assert_eq!(config.agent, "codex");
    assert!(!config.animations);
    assert_eq!(
        config.default_models.get("codex").map(String::as_str),
        Some("gpt-5")
    );
    assert_eq!(config.active_project.trust_level, Some(TrustLevel::Trusted));
}

#[test]
fn repository_trust_applies_to_nested_directories_and_linked_worktrees() {
    let home = TempDir::new().expect("create config home");
    let repo = create_git_repository(&home).expect("create git repository");
    let nested = repo.join("nested/directory");
    std::fs::create_dir_all(&nested).expect("create nested directory");
    let worktree = home.path().join("linked-worktree");
    git(
        &repo,
        &[
            "worktree",
            "add",
            worktree.to_str().expect("utf-8 worktree path"),
            "-b",
            "config-test-worktree",
        ],
    )
    .expect("create linked worktree");

    let config_path = home.path().join("config.toml");
    NoriConfigEdits::new(home.path())
        .set_project_trust_level(&repo, TrustLevel::Trusted)
        .apply_blocking()
        .expect("persist repository trust");

    for cwd in [nested, worktree] {
        let config = NoriConfig::load_from_path_with_overrides(
            &config_path,
            NoriConfigOverrides {
                cwd: Some(cwd),
                ..NoriConfigOverrides::default()
            },
        )
        .expect("load config");

        assert_eq!(
            config.active_project.trust_level,
            Some(TrustLevel::Trusted),
            "{} should inherit trust from the primary repository",
            config.cwd.display()
        );
    }
}

#[test]
fn legacy_profiles_fail_with_an_actionable_error() {
    let home = TempDir::new().expect("create config home");
    let config_path = home.path().join("config.toml");
    for legacy_config in [
        "profile = \"focused\"\n",
        "[profiles.focused]\nagent = \"claude-code\"\n",
    ] {
        std::fs::write(&config_path, legacy_config).expect("write legacy config");

        let error = NoriConfig::load_from_path(&config_path).expect_err("profiles are unsupported");
        let message = format!("{error:#}");

        assert!(message.contains("profiles are no longer supported"));
        assert!(message.contains("skillsets"));
    }
}

#[test]
fn legacy_model_key_is_rejected() {
    let home = TempDir::new().expect("create config home");
    let config_path = home.path().join("config.toml");
    std::fs::write(&config_path, "model = \"gemini\"\n").expect("write legacy config");

    let error = NoriConfig::load_from_path(&config_path).expect_err("model is unsupported");
    assert!(format!("{error:#}").contains("use `agent`"));
}

#[test]
fn generic_edits_set_and_clear_nori_owned_paths_without_reformatting() {
    let home = TempDir::new().expect("create config home");
    let config_path = home.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"# user explanation
[tui]
animations = true
vertical_footer = false # preserve this note
"#,
    )
    .expect("write config");

    NoriConfigEdits::new(home.path())
        .set_path(&["tui", "vertical_footer"], true)
        .set_path(&["acp_proxy", "enabled"], true)
        .clear_path(&["tui", "animations"])
        .apply_blocking()
        .expect("persist generic edits");

    let contents = std::fs::read_to_string(config_path).expect("read edited config");
    assert!(contents.contains("# user explanation"));
    assert!(contents.contains("vertical_footer = true # preserve this note"));
    assert!(contents.contains("[acp_proxy]"));
    assert!(contents.contains("enabled = true"));
    assert!(!contents.contains("animations"));
}

#[test]
fn resolved_config_uses_protocol_mcp_types_and_keeps_the_notifier_command() {
    let home = TempDir::new().expect("create config home");
    let config_path = home.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"notify = ["notify-send", "Nori"]

[mcp_servers.files]
command = "npx"
args = ["@modelcontextprotocol/server-filesystem", "."]
"#,
    )
    .expect("write config");

    let config = NoriConfig::load_from_path(&config_path).expect("load config");
    let server: &codex_protocol::config_types::McpServerConfig = config
        .mcp_servers
        .get("files")
        .expect("resolved MCP server");

    assert!(matches!(
        server.transport,
        codex_protocol::config_types::McpServerTransportConfig::Stdio { .. }
    ));
    assert_eq!(
        config.notify,
        Some(vec!["notify-send".to_string(), "Nori".to_string()])
    );
}

#[test]
fn replacing_mcp_servers_round_trips_canonical_protocol_config() {
    let home = TempDir::new().expect("create config home");
    let config_path = home.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"# keep user comments
agent = "claude-code"

[mcp_servers.old]
command = "old-server"
"#,
    )
    .expect("write config");

    let mut servers = std::collections::BTreeMap::new();
    servers.insert(
        "docs".to_string(),
        codex_protocol::config_types::McpServerConfig {
            transport: codex_protocol::config_types::McpServerTransportConfig::StreamableHttp {
                url: "https://docs.example.test/mcp".to_string(),
                bearer_token_env_var: Some("DOCS_TOKEN".to_string()),
                http_headers: None,
                env_http_headers: None,
                client_id: None,
                client_secret_env_var: None,
            },
            enabled: true,
            startup_timeout_sec: Some(std::time::Duration::from_secs(12)),
            tool_timeout_sec: None,
            enabled_tools: Some(vec!["search".to_string()]),
            disabled_tools: None,
        },
    );

    NoriConfigEdits::new(home.path())
        .replace_mcp_servers(&servers)
        .apply_blocking()
        .expect("persist MCP servers");

    let contents = std::fs::read_to_string(&config_path).expect("read edited config");
    assert!(contents.contains("# keep user comments"));
    let config = NoriConfig::load_from_path(&config_path).expect("reload config");
    assert!(!config.mcp_servers.contains_key("old"));
    assert_eq!(
        config.mcp_servers.get("docs"),
        servers.get("docs"),
        "canonical MCP config should survive persistence"
    );
}

#[test]
fn loader_resolves_active_safety_and_environment_settings() {
    let home = TempDir::new().expect("create config home");
    let config_path = home.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"approval_policy = "never"
check_for_update_on_startup = false
disable_paste_burst = true

[shell_environment_policy]
inherit = "core"
ignore_default_excludes = true

[notice]
hide_full_access_warning = true
hide_world_writable_warning = false
"#,
    )
    .expect("write config");

    let config = NoriConfig::load_from_path(&config_path).expect("load config");

    assert!(config.has_explicit_approval_or_sandbox_policy);
    assert!(!config.check_for_update_on_startup);
    assert!(config.disable_paste_burst);
    assert_eq!(
        config.shell_environment_policy.inherit,
        codex_protocol::config_types::ShellEnvironmentPolicyInherit::Core
    );
    assert!(config.shell_environment_policy.ignore_default_excludes);
    assert_eq!(config.notices.hide_full_access_warning, Some(true));
    assert_eq!(config.notices.hide_world_writable_warning, Some(false));
}

#[test]
fn safety_and_environment_settings_have_nori_defaults() {
    let config = NoriConfig::default();

    assert!(!config.has_explicit_approval_or_sandbox_policy);
    assert!(!config.forced_auto_mode_downgraded_on_windows);
    assert!(!config.windows_sandbox_enabled);
    assert!(config.check_for_update_on_startup);
    assert!(!config.disable_paste_burst);
    assert_eq!(
        config.shell_environment_policy,
        codex_protocol::config_types::ShellEnvironmentPolicy::default()
    );
    assert_eq!(config.notices, nori_config::Notice::default());
}

#[test]
fn explicit_policy_metadata_includes_sandbox_config_and_typed_overrides() {
    let home = TempDir::new().expect("create config home");
    let config_path = home.path().join("config.toml");
    std::fs::write(&config_path, "sandbox_mode = \"read-only\"\n").expect("write config");

    let from_file = NoriConfig::load_from_path(&config_path).expect("load file config");
    assert!(from_file.has_explicit_approval_or_sandbox_policy);

    std::fs::write(&config_path, "").expect("clear config");
    let from_typed_override = NoriConfig::load_from_path_with_overrides(
        &config_path,
        NoriConfigOverrides {
            approval_policy: Some(AskForApproval::Never),
            ..NoriConfigOverrides::default()
        },
    )
    .expect("load typed override");
    assert!(from_typed_override.has_explicit_approval_or_sandbox_policy);
}

#[test]
fn windows_sandbox_setting_and_availability_resolve_the_runtime_policy() {
    let home = TempDir::new().expect("create config home");
    let config_path = home.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"[features]
enable_experimental_windows_sandbox = true
"#,
    )
    .expect("write config");
    let mut config = NoriConfig::load_from_path(&config_path).expect("load config");

    assert!(config.windows_sandbox_enabled);
    config.apply_windows_sandbox_availability(false);
    assert!(config.forced_auto_mode_downgraded_on_windows);
    assert_eq!(config.sandbox_policy, SandboxPolicy::ReadOnly);

    let mut available = NoriConfig::load_from_path(&config_path).expect("reload config");
    available.apply_windows_sandbox_availability(true);
    assert!(!available.forced_auto_mode_downgraded_on_windows);
    assert!(matches!(
        available.sandbox_policy,
        SandboxPolicy::WorkspaceWrite { .. }
    ));
}
