//! Configuration loading for Nori CLI

use super::types::DEFAULT_AGENT;
use super::types::NoriConfig;
use super::types::NoriConfigOverrides;
use super::types::NoriConfigToml;
use crate::AskForApproval;
use crate::McpServerConfig;
use crate::SandboxMode;
use crate::SandboxPolicy;
use crate::TrustLevel;
use crate::git_root::resolve_root_git_project_for_trust;
use anyhow::Context;
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// Environment variable to override the Nori home directory
pub const NORI_HOME_ENV: &str = "NORI_HOME";

/// Default Nori home directory path (relative to home)
pub const NORI_HOME_DIR: &str = ".nori/cli";

/// Config file name
pub const CONFIG_FILE: &str = "config.toml";

/// Find the Nori home directory (~/.nori/cli or $NORI_HOME)
pub fn find_nori_home() -> Result<PathBuf> {
    if let Ok(env_home) = std::env::var(NORI_HOME_ENV) {
        return Ok(PathBuf::from(env_home));
    }

    let home = dirs::home_dir().context("Could not determine home directory")?;

    Ok(home.join(NORI_HOME_DIR))
}

impl NoriConfig {
    /// Load configuration from ~/.nori/cli/config.toml
    pub fn load() -> Result<Self> {
        Self::load_with_overrides(NoriConfigOverrides::default())
    }

    /// Load configuration with CLI overrides
    pub fn load_with_overrides(overrides: NoriConfigOverrides) -> Result<Self> {
        let nori_home = find_nori_home()?;
        let config_path = nori_home.join(CONFIG_FILE);
        Self::load_from_path_with_overrides(&config_path, overrides)
    }

    /// Load configuration from a specific path (for testing)
    pub fn load_from_path(config_path: &Path) -> Result<Self> {
        Self::load_from_path_with_overrides(config_path, NoriConfigOverrides::default())
    }

    /// Load configuration from a specific path with raw and typed overrides.
    pub fn load_from_path_with_overrides(
        config_path: &Path,
        overrides: NoriConfigOverrides,
    ) -> Result<Self> {
        let nori_home = config_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let toml_config = load_toml(config_path, &overrides.raw_overrides)?;
        Self::from_toml(toml_config, nori_home, overrides)
    }

    /// Build resolved config from TOML + overrides
    fn from_toml(
        toml: NoriConfigToml,
        nori_home: PathBuf,
        overrides: NoriConfigOverrides,
    ) -> Result<Self> {
        let has_explicit_approval_or_sandbox_policy = toml.approval_policy.is_some()
            || toml.sandbox_mode.is_some()
            || toml.sandbox_workspace_write.is_some()
            || overrides.approval_policy.is_some()
            || overrides.sandbox_mode.is_some()
            || overrides.raw_overrides.iter().any(|(path, _)| {
                matches!(path.as_str(), "approval_policy" | "sandbox_mode")
                    || path.starts_with("sandbox_workspace_write.")
            });
        let NoriConfigOverrides {
            agent: agent_override,
            sandbox_mode: sandbox_mode_override,
            approval_policy: approval_policy_override,
            cwd,
            additional_writable_roots,
            raw_overrides: _,
        } = overrides;

        let process_cwd = std::env::current_dir().unwrap_or_default();
        let cwd = resolve_path(cwd.unwrap_or_else(|| process_cwd.clone()), &process_cwd);
        let active_project = toml
            .projects
            .get(cwd.to_string_lossy().as_ref())
            .cloned()
            .or_else(|| {
                resolve_root_git_project_for_trust(&cwd).and_then(|repo_root| {
                    toml.projects
                        .get(repo_root.to_string_lossy().as_ref())
                        .cloned()
                })
            })
            .unwrap_or_default();
        let sandbox_mode = sandbox_mode_override
            .or(toml.sandbox_mode)
            .unwrap_or(SandboxMode::WorkspaceWrite);
        let mut sandbox_policy = match sandbox_mode {
            SandboxMode::ReadOnly => SandboxPolicy::new_read_only_policy(),
            SandboxMode::WorkspaceWrite => toml
                .sandbox_workspace_write
                .as_ref()
                .map(|settings| SandboxPolicy::WorkspaceWrite {
                    writable_roots: settings.writable_roots.clone(),
                    network_access: settings.network_access,
                    exclude_tmpdir_env_var: settings.exclude_tmpdir_env_var,
                    exclude_slash_tmp: settings.exclude_slash_tmp,
                })
                .unwrap_or_else(SandboxPolicy::new_workspace_write_policy),
            SandboxMode::DangerFullAccess => SandboxPolicy::DangerFullAccess,
        };
        if let SandboxPolicy::WorkspaceWrite { writable_roots, .. } = &mut sandbox_policy {
            for root in additional_writable_roots {
                let root = resolve_path(root, &cwd);
                if !writable_roots.contains(&root) {
                    writable_roots.push(root);
                }
            }
        }
        let approval_policy = approval_policy_override.or(toml.approval_policy).unwrap_or(
            match active_project.trust_level {
                Some(TrustLevel::Untrusted) => AskForApproval::UnlessTrusted,
                Some(TrustLevel::Trusted) | None => AskForApproval::OnRequest,
            },
        );

        // Resolve MCP servers
        let mcp_servers = resolve_mcp_servers(toml.mcp_servers)?;

        // Resolve hooks
        let session_start_hooks = super::types::resolve_hook_paths(toml.hooks.session_start);
        let session_end_hooks = super::types::resolve_hook_paths(toml.hooks.session_end);
        let pre_user_prompt_hooks = super::types::resolve_hook_paths(toml.hooks.pre_user_prompt);
        let post_user_prompt_hooks = super::types::resolve_hook_paths(toml.hooks.post_user_prompt);
        let pre_tool_call_hooks = super::types::resolve_hook_paths(toml.hooks.pre_tool_call);
        let post_tool_call_hooks = super::types::resolve_hook_paths(toml.hooks.post_tool_call);
        let pre_agent_response_hooks =
            super::types::resolve_hook_paths(toml.hooks.pre_agent_response);
        let post_agent_response_hooks =
            super::types::resolve_hook_paths(toml.hooks.post_agent_response);

        // Resolve async (fire-and-forget) hooks
        let async_session_start_hooks =
            super::types::resolve_hook_paths(toml.hooks.async_session_start);
        let async_session_end_hooks =
            super::types::resolve_hook_paths(toml.hooks.async_session_end);
        let async_pre_user_prompt_hooks =
            super::types::resolve_hook_paths(toml.hooks.async_pre_user_prompt);
        let async_post_user_prompt_hooks =
            super::types::resolve_hook_paths(toml.hooks.async_post_user_prompt);
        let async_pre_tool_call_hooks =
            super::types::resolve_hook_paths(toml.hooks.async_pre_tool_call);
        let async_post_tool_call_hooks =
            super::types::resolve_hook_paths(toml.hooks.async_post_tool_call);
        let async_pre_agent_response_hooks =
            super::types::resolve_hook_paths(toml.hooks.async_pre_agent_response);
        let async_post_agent_response_hooks =
            super::types::resolve_hook_paths(toml.hooks.async_post_agent_response);

        // Agent is the user's persisted preference, defaults to DEFAULT_AGENT
        let agent = toml.agent.unwrap_or_else(|| DEFAULT_AGENT.to_string());

        let skillset_per_session = toml.tui.skillset_per_session.unwrap_or(false);
        let auto_worktree = toml.tui.auto_worktree.unwrap_or_default();
        let acp_proxy = super::types::AcpProxyConfig::from_toml(toml.acp_proxy, &nori_home);

        // The CLI override is session-only; otherwise use the persisted agent.
        let active_agent = agent_override.unwrap_or_else(|| agent.clone());

        Ok(Self {
            agent,
            active_agent,
            sandbox_mode,
            sandbox_policy,
            approval_policy,
            has_explicit_approval_or_sandbox_policy,
            forced_auto_mode_downgraded_on_windows: false,
            windows_sandbox_enabled: toml
                .features
                .enable_experimental_windows_sandbox
                .unwrap_or(false),
            shell_environment_policy: toml.shell_environment_policy.into(),
            active_project,
            notices: toml.notice,
            check_for_update_on_startup: toml.check_for_update_on_startup.unwrap_or(true),
            disable_paste_burst: toml.disable_paste_burst.unwrap_or(false),
            history_persistence: toml
                .history_persistence
                .unwrap_or(super::types::HistoryPersistence::SaveAll),
            browser_profile: toml
                .browser_profile
                .unwrap_or(super::types::BrowserProfileMode::Throwaway),
            notify: toml.notify,
            acp_proxy,
            animations: toml.tui.animations.unwrap_or(true),
            terminal_notifications: toml
                .tui
                .terminal_notifications
                .unwrap_or(super::types::TerminalNotifications::Enabled),
            os_notifications: toml
                .tui
                .os_notifications
                .unwrap_or(super::types::OsNotifications::Enabled),
            vertical_footer: toml.tui.vertical_footer.unwrap_or(false),
            notify_after_idle: toml
                .tui
                .notify_after_idle
                .unwrap_or(super::types::NotifyAfterIdle::FiveSeconds),
            vim_mode: toml.tui.vim_mode.unwrap_or_default(),
            hotkeys: super::types::HotkeyConfig::from_toml(&toml.tui.hotkeys),
            script_timeout: toml.tui.script_timeout.unwrap_or_default(),
            loop_count: toml.tui.loop_count,
            skillset_per_session,
            file_manager: toml.tui.file_manager,
            pinned_plan_drawer: toml.tui.pinned_plan_drawer.unwrap_or(false),
            custom_working_messages: toml.tui.custom_working_messages.unwrap_or(true),
            custom_working_message_list: toml
                .tui
                .custom_working_message_list
                .clone()
                .unwrap_or_default(),
            auto_worktree,
            footer_segment_config: super::types::FooterSegmentConfig::from_toml(
                &toml.tui.footer_segments,
            ),
            footer_layout_config: super::types::FooterLayoutConfig::from_toml(
                &toml.tui.footer_layout,
            ),
            nori_home,
            cwd,
            mcp_servers,
            session_start_hooks,
            session_end_hooks,
            pre_user_prompt_hooks,
            post_user_prompt_hooks,
            pre_tool_call_hooks,
            post_tool_call_hooks,
            pre_agent_response_hooks,
            post_agent_response_hooks,
            async_session_start_hooks,
            async_session_end_hooks,
            async_pre_user_prompt_hooks,
            async_post_user_prompt_hooks,
            async_pre_tool_call_hooks,
            async_post_tool_call_hooks,
            async_pre_agent_response_hooks,
            async_post_agent_response_hooks,
            default_models: toml.default_models,
            agents: toml.agents,
            cloud_broker_url: toml.cloud.broker_url,
        })
    }
}

fn load_toml(
    config_path: &Path,
    raw_overrides: &[(String, toml::Value)],
) -> Result<NoriConfigToml> {
    let mut value = if config_path.exists() {
        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read {}", config_path.display()))?;
        if content.trim().is_empty() {
            toml::Value::Table(toml::Table::new())
        } else {
            toml::from_str::<toml::Value>(&content)
                .with_context(|| format!("Failed to parse {}", config_path.display()))?
        }
    } else {
        toml::Value::Table(toml::Table::new())
    };

    for (path, override_value) in raw_overrides {
        apply_toml_override(&mut value, path, override_value.clone());
    }

    if value.get("profile").is_some() || value.get("profiles").is_some() {
        anyhow::bail!(
            "Codex configuration profiles are no longer supported; use Nori skillsets to select dedicated agent behavior"
        );
    }
    if value.get("model").is_some() {
        anyhow::bail!("the legacy `model` key is no longer supported; use `agent` instead");
    }

    value
        .try_into()
        .with_context(|| format!("Failed to parse {}", config_path.display()))
}

fn apply_toml_override(root: &mut toml::Value, path: &str, value: toml::Value) {
    let segments: Vec<&str> = path.split('.').collect();
    let mut current = root;

    for (index, segment) in segments.iter().enumerate() {
        let is_last = index == segments.len() - 1;
        if is_last {
            if !current.is_table() {
                *current = toml::Value::Table(toml::Table::new());
            }
            if let toml::Value::Table(table) = current {
                table.insert((*segment).to_string(), value);
            }
            return;
        }

        if !current.is_table() {
            *current = toml::Value::Table(toml::Table::new());
        }
        if let toml::Value::Table(table) = current {
            current = table
                .entry((*segment).to_string())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        }
    }
}

fn resolve_path(path: PathBuf, cwd: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    std::fs::canonicalize(&absolute).unwrap_or(absolute)
}

/// Resolve MCP server configurations from TOML
fn resolve_mcp_servers(
    toml_servers: HashMap<String, McpServerConfig>,
) -> Result<HashMap<String, McpServerConfig>> {
    const RESERVED_NORI_CLIENT_MCP_SERVER_NAME: &str = "nori-client";

    let mut resolved = HashMap::new();

    for (name, server) in toml_servers {
        if name == RESERVED_NORI_CLIENT_MCP_SERVER_NAME {
            return Err(anyhow::anyhow!(
                "MCP server name '{name}' is reserved for Nori's backend-owned nori-client server"
            ));
        }

        resolved.insert(name, server);
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_load_with_mcp_servers() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
agent = "claude-code"

[mcp_servers.filesystem]
command = "npx"
args = ["@modelcontextprotocol/server-filesystem", "/tmp"]

[mcp_servers.web]
url = "https://mcp.example.com"
bearer_token_env_var = "MCP_TOKEN"
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();

        assert_eq!(config.mcp_servers.len(), 2);
        assert!(config.mcp_servers.contains_key("filesystem"));
        assert!(config.mcp_servers.contains_key("web"));
    }

    #[test]
    fn test_load_invalid_mcp_server() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[mcp_servers.invalid]
# Missing both command and url
enabled = true
"#,
        )
        .unwrap();

        let result = NoriConfig::load_from_path(&config_path);
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(err.contains("invalid transport"));
    }

    #[test]
    fn test_load_rejects_reserved_nori_client_mcp_server() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[mcp_servers.nori-client]
command = "npx"
args = ["@example/not-allowed"]
"#,
        )
        .unwrap();

        let result = NoriConfig::load_from_path(&config_path);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("reserved"));
        assert!(err.contains("nori-client"));
    }

    #[test]
    fn test_cwd_override_with_from_toml() {
        let toml = NoriConfigToml::default();
        let nori_home = PathBuf::from("/tmp/nori");
        let custom_cwd = PathBuf::from("/custom/path");

        let overrides = NoriConfigOverrides {
            cwd: Some(custom_cwd.clone()),
            ..Default::default()
        };

        let config = NoriConfig::from_toml(toml, nori_home, overrides).unwrap();
        assert_eq!(config.cwd, custom_cwd);
    }

    #[test]
    fn test_load_persisted_agent_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        // Write a config file with an agent field
        std::fs::write(
            &config_path,
            r#"
agent = "gemini"
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();

        // The persisted agent should be loaded directly.
        assert_eq!(
            config.agent, "gemini",
            "Agent should be loaded from config.toml"
        );
    }

    #[test]
    fn test_agent_defaults_to_claude_code() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        // Write an empty config file (no agent specified)
        std::fs::write(&config_path, "").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();

        // The agent should default to "claude-code"
        assert_eq!(
            config.agent, "claude-code",
            "Agent should default to 'claude-code' when not specified"
        );
    }

    #[test]
    fn test_load_notify_after_idle_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[tui]
notify_after_idle = "30s"
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(
            config.notify_after_idle,
            super::super::types::NotifyAfterIdle::ThirtySeconds
        );
    }

    #[test]
    fn test_notify_after_idle_defaults_to_five_seconds() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(&config_path, "").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(
            config.notify_after_idle,
            super::super::types::NotifyAfterIdle::FiveSeconds
        );
    }

    #[test]
    fn test_acp_proxy_defaults_disabled() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(&config_path, "").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(!config.acp_proxy.enabled);
        assert_eq!(config.acp_proxy.log_dir, temp_dir.path().join("acp-wire"));
    }

    #[test]
    fn test_acp_proxy_enabled_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[acp_proxy]
enabled = true
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(config.acp_proxy.enabled);
        assert_eq!(config.acp_proxy.log_dir, temp_dir.path().join("acp-wire"));
    }

    #[test]
    fn test_active_agent_uses_persisted_agent_as_fallback() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        // Write a config with the persisted agent set.
        std::fs::write(&config_path, "agent = \"gemini\"").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();

        // Active agent should fall back to the persisted agent value
        assert_eq!(
            config.active_agent, "gemini",
            "Active agent should use persisted agent as fallback when not overridden"
        );
        assert_eq!(config.agent, "gemini");
    }

    #[test]
    fn test_auto_worktree_automatic_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[tui]
auto_worktree = "automatic"
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(
            config.auto_worktree,
            super::super::types::AutoWorktree::Automatic
        );
    }

    #[test]
    fn test_auto_worktree_ask_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[tui]
auto_worktree = "ask"
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.auto_worktree, super::super::types::AutoWorktree::Ask);
    }

    #[test]
    fn test_auto_worktree_off_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[tui]
auto_worktree = "off"
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.auto_worktree, super::super::types::AutoWorktree::Off);
    }

    #[test]
    fn test_auto_worktree_defaults_to_off() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(&config_path, "").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.auto_worktree, super::super::types::AutoWorktree::Off);
    }

    #[test]
    fn test_auto_worktree_backwards_compat_true() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[tui]
auto_worktree = true
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(
            config.auto_worktree,
            super::super::types::AutoWorktree::Automatic
        );
    }

    #[test]
    fn test_auto_worktree_backwards_compat_false() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[tui]
auto_worktree = false
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.auto_worktree, super::super::types::AutoWorktree::Off);
    }

    // ========================================================================
    // File Manager Config Tests
    // ========================================================================

    #[test]
    fn test_file_manager_loaded_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[tui]
file_manager = "vifm"
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(
            config.file_manager,
            Some(super::super::types::FileManager::Vifm)
        );
    }

    #[test]
    fn test_file_manager_ranger_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[tui]
file_manager = "ranger"
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(
            config.file_manager,
            Some(super::super::types::FileManager::Ranger)
        );
    }

    #[test]
    fn test_file_manager_defaults_to_none() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(&config_path, "").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.file_manager, None);
    }

    #[test]
    fn test_file_manager_chooser_args_vifm() {
        use super::super::types::FileManager;
        use std::path::Path;

        let args = FileManager::Vifm.chooser_args(Path::new("/tmp/chooser.txt"));
        assert_eq!(args, vec!["--choose-files", "/tmp/chooser.txt"]);
    }

    #[test]
    fn test_file_manager_chooser_args_ranger() {
        use super::super::types::FileManager;
        use std::path::Path;

        let args = FileManager::Ranger.chooser_args(Path::new("/tmp/chooser.txt"));
        assert_eq!(args, vec!["--choosefile=/tmp/chooser.txt"]);
    }

    #[test]
    fn test_file_manager_chooser_args_lf() {
        use super::super::types::FileManager;
        use std::path::Path;

        let args = FileManager::Lf.chooser_args(Path::new("/tmp/chooser.txt"));
        assert_eq!(args, vec!["-selection-path", "/tmp/chooser.txt"]);
    }

    #[test]
    fn test_file_manager_chooser_args_nnn() {
        use super::super::types::FileManager;
        use std::path::Path;

        let args = FileManager::Nnn.chooser_args(Path::new("/tmp/chooser.txt"));
        assert_eq!(args, vec!["-p", "/tmp/chooser.txt"]);
    }

    #[test]
    fn test_file_manager_command_names() {
        use super::super::types::FileManager;

        assert_eq!(FileManager::Vifm.command_name(), "vifm");
        assert_eq!(FileManager::Ranger.command_name(), "ranger");
        assert_eq!(FileManager::Lf.command_name(), "lf");
        assert_eq!(FileManager::Nnn.command_name(), "nnn");
    }

    // ========================================================================
    // Session Hooks Config Tests
    // ========================================================================

    #[test]
    fn test_hooks_loaded_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[hooks]
session_start = ["/path/to/start.sh", "/path/to/init.py"]
session_end = ["/path/to/cleanup.sh"]
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.session_start_hooks.len(), 2);
        assert_eq!(
            config.session_start_hooks[0],
            PathBuf::from("/path/to/start.sh")
        );
        assert_eq!(
            config.session_start_hooks[1],
            PathBuf::from("/path/to/init.py")
        );
        assert_eq!(config.session_end_hooks.len(), 1);
        assert_eq!(
            config.session_end_hooks[0],
            PathBuf::from("/path/to/cleanup.sh")
        );
    }

    #[test]
    fn test_hooks_default_to_empty_when_absent() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(&config_path, "").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(config.session_start_hooks.is_empty());
        assert!(config.session_end_hooks.is_empty());
    }

    #[test]
    fn test_hooks_tilde_expansion() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[hooks]
session_start = ["~/hooks/start.sh"]
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.session_start_hooks.len(), 1);
        // Should have expanded ~ to home dir, not kept literal ~
        let path = &config.session_start_hooks[0];
        assert!(!path.starts_with("~"));
        assert!(path.ends_with("hooks/start.sh"));
    }

    #[test]
    fn test_lifecycle_hooks_loaded_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[hooks]
pre_user_prompt = ["/path/to/pre-prompt.sh"]
post_user_prompt = ["/path/to/post-prompt.sh"]
pre_tool_call = ["/path/to/pre-tool.sh"]
post_tool_call = ["/path/to/post-tool.sh"]
pre_agent_response = ["/path/to/pre-response.sh"]
post_agent_response = ["/path/to/post-response.sh"]
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.pre_user_prompt_hooks.len(), 1);
        assert_eq!(
            config.pre_user_prompt_hooks[0],
            PathBuf::from("/path/to/pre-prompt.sh")
        );
        assert_eq!(config.post_user_prompt_hooks.len(), 1);
        assert_eq!(
            config.post_user_prompt_hooks[0],
            PathBuf::from("/path/to/post-prompt.sh")
        );
        assert_eq!(config.pre_tool_call_hooks.len(), 1);
        assert_eq!(
            config.pre_tool_call_hooks[0],
            PathBuf::from("/path/to/pre-tool.sh")
        );
        assert_eq!(config.post_tool_call_hooks.len(), 1);
        assert_eq!(
            config.post_tool_call_hooks[0],
            PathBuf::from("/path/to/post-tool.sh")
        );
        assert_eq!(config.pre_agent_response_hooks.len(), 1);
        assert_eq!(
            config.pre_agent_response_hooks[0],
            PathBuf::from("/path/to/pre-response.sh")
        );
        assert_eq!(config.post_agent_response_hooks.len(), 1);
        assert_eq!(
            config.post_agent_response_hooks[0],
            PathBuf::from("/path/to/post-response.sh")
        );
    }

    #[test]
    fn test_lifecycle_hooks_default_to_empty() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(&config_path, "").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(config.pre_user_prompt_hooks.is_empty());
        assert!(config.post_user_prompt_hooks.is_empty());
        assert!(config.pre_tool_call_hooks.is_empty());
        assert!(config.post_tool_call_hooks.is_empty());
        assert!(config.pre_agent_response_hooks.is_empty());
        assert!(config.post_agent_response_hooks.is_empty());
    }

    #[test]
    fn test_lifecycle_hooks_mixed_with_session_hooks() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[hooks]
session_start = ["/path/to/start.sh"]
session_end = ["/path/to/end.sh"]
pre_user_prompt = ["/path/to/pre-prompt.sh"]
post_tool_call = ["/path/to/post-tool.sh"]
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.session_start_hooks.len(), 1);
        assert_eq!(config.session_end_hooks.len(), 1);
        assert_eq!(config.pre_user_prompt_hooks.len(), 1);
        assert_eq!(config.post_tool_call_hooks.len(), 1);
        assert!(config.post_user_prompt_hooks.is_empty());
        assert!(config.pre_tool_call_hooks.is_empty());
        assert!(config.pre_agent_response_hooks.is_empty());
        assert!(config.post_agent_response_hooks.is_empty());
    }

    #[test]
    fn test_hooks_partial_section_only_start() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[hooks]
session_start = ["/path/to/start.sh"]
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.session_start_hooks.len(), 1);
        assert!(config.session_end_hooks.is_empty());
    }

    // ========================================================================
    // Async Hooks Config Tests
    // ========================================================================

    #[test]
    fn test_async_hooks_loaded_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[hooks]
async_session_start = ["/path/to/async-start.sh", "/path/to/async-init.py"]
async_session_end = ["/path/to/async-cleanup.sh"]
async_pre_user_prompt = ["/path/to/async-pre-prompt.sh"]
async_post_user_prompt = ["/path/to/async-post-prompt.sh"]
async_pre_tool_call = ["/path/to/async-pre-tool.sh"]
async_post_tool_call = ["/path/to/async-post-tool.sh"]
async_pre_agent_response = ["/path/to/async-pre-response.sh"]
async_post_agent_response = ["/path/to/async-post-response.sh"]
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.async_session_start_hooks.len(), 2);
        assert_eq!(
            config.async_session_start_hooks[0],
            PathBuf::from("/path/to/async-start.sh")
        );
        assert_eq!(
            config.async_session_start_hooks[1],
            PathBuf::from("/path/to/async-init.py")
        );
        assert_eq!(config.async_session_end_hooks.len(), 1);
        assert_eq!(config.async_pre_user_prompt_hooks.len(), 1);
        assert_eq!(config.async_post_user_prompt_hooks.len(), 1);
        assert_eq!(config.async_pre_tool_call_hooks.len(), 1);
        assert_eq!(config.async_post_tool_call_hooks.len(), 1);
        assert_eq!(config.async_pre_agent_response_hooks.len(), 1);
        assert_eq!(config.async_post_agent_response_hooks.len(), 1);
    }

    #[test]
    fn test_async_hooks_default_to_empty_when_absent() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(&config_path, "").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(config.async_session_start_hooks.is_empty());
        assert!(config.async_session_end_hooks.is_empty());
        assert!(config.async_pre_user_prompt_hooks.is_empty());
        assert!(config.async_post_user_prompt_hooks.is_empty());
        assert!(config.async_pre_tool_call_hooks.is_empty());
        assert!(config.async_post_tool_call_hooks.is_empty());
        assert!(config.async_pre_agent_response_hooks.is_empty());
        assert!(config.async_post_agent_response_hooks.is_empty());
    }

    #[test]
    fn test_async_hooks_coexist_with_sync_hooks() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[hooks]
session_start = ["/path/to/sync-start.sh"]
async_session_start = ["/path/to/async-start.sh"]
pre_tool_call = ["/path/to/sync-pre-tool.sh"]
async_pre_tool_call = ["/path/to/async-pre-tool.sh"]
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.session_start_hooks.len(), 1);
        assert_eq!(
            config.session_start_hooks[0],
            PathBuf::from("/path/to/sync-start.sh")
        );
        assert_eq!(config.async_session_start_hooks.len(), 1);
        assert_eq!(
            config.async_session_start_hooks[0],
            PathBuf::from("/path/to/async-start.sh")
        );
        assert_eq!(config.pre_tool_call_hooks.len(), 1);
        assert_eq!(config.async_pre_tool_call_hooks.len(), 1);
    }

    #[test]
    fn test_async_hooks_tilde_expansion() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[hooks]
async_session_start = ["~/hooks/async-start.sh"]
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.async_session_start_hooks.len(), 1);
        let path = &config.async_session_start_hooks[0];
        assert!(!path.starts_with("~"));
        assert!(path.ends_with("hooks/async-start.sh"));
    }

    // ========================================================================
    // Default Models Config Tests
    // ========================================================================

    #[test]
    fn test_default_models_loaded_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
agent = "claude-code"

[default_models]
claude-code = "haiku"
gemini = "flash"
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.default_models.len(), 2);
        assert_eq!(config.default_models.get("claude-code").unwrap(), "haiku");
        assert_eq!(config.default_models.get("gemini").unwrap(), "flash");
    }

    #[test]
    fn test_default_models_empty_when_absent() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(&config_path, "agent = \"claude-code\"").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(config.default_models.is_empty());
    }

    // ========================================================================
    // Skillset Per-Session Config Tests
    // ========================================================================

    #[test]
    fn test_skillset_per_session_enabled_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[tui]
skillset_per_session = true
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(
            config.skillset_per_session,
            "skillset_per_session should be true when set in config"
        );
        assert_eq!(
            config.auto_worktree,
            super::super::types::AutoWorktree::Off,
            "auto_worktree should remain off (default) since the settings are independent"
        );
    }

    #[test]
    fn test_skillset_per_session_defaults_to_false() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(&config_path, "").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(
            !config.skillset_per_session,
            "skillset_per_session should default to false"
        );
    }

    #[test]
    fn test_skillset_per_session_does_not_force_auto_worktree() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[tui]
skillset_per_session = true
auto_worktree = "off"
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(
            config.skillset_per_session,
            "skillset_per_session should be true"
        );
        assert_eq!(
            config.auto_worktree,
            super::super::types::AutoWorktree::Off,
            "auto_worktree should be off because the two settings are independent"
        );
    }

    #[test]
    fn test_agents_loaded_from_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
agent = "claude-code"

[[agents]]
name = "Kimi"
slug = "kimi"

[agents.distribution.uvx]
package = "kimi-cli"
args = ["acp"]

[[agents]]
name = "My Local Agent"
slug = "my-local"

[agents.distribution.local]
command = "/usr/bin/my-agent"
args = ["--acp"]
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.agents.len(), 2);
        assert_eq!(config.agents[0].slug, "kimi");
        assert_eq!(config.agents[1].slug, "my-local");
    }

    #[test]
    fn test_agents_default_to_empty() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(&config_path, "agent = \"claude-code\"").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(config.agents.is_empty());
    }

    #[test]
    fn test_default_models_coexist_with_other_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
agent = "gemini"
sandbox_mode = "workspace-write"

[default_models]
claude-code = "haiku"

[tui]
vim_mode = true
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert_eq!(config.agent, "gemini");
        assert_eq!(config.default_models.get("claude-code").unwrap(), "haiku");
        assert_eq!(config.vim_mode, crate::VimEnterBehavior::Submit);
    }
}
