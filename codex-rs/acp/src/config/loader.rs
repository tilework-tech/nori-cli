//! Configuration loading for Nori CLI

use super::types::ApprovalPolicy;
use super::types::DEFAULT_AGENT;
use super::types::McpServerConfig;
use super::types::NoriConfig;
use super::types::NoriConfigOverrides;
use super::types::NoriConfigToml;
use anyhow::Context;
use anyhow::Result;
use codex_protocol::config_types::SandboxMode;
use std::collections::HashMap;
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

        let toml_config = if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read {}", config_path.display()))?;
            toml::from_str::<NoriConfigToml>(&content)
                .with_context(|| format!("Failed to parse {}", config_path.display()))?
        } else {
            NoriConfigToml::default()
        };

        Self::from_toml(toml_config, nori_home, overrides)
    }

    /// Load configuration from a specific path (for testing)
    pub fn load_from_path(config_path: &PathBuf) -> Result<Self> {
        let nori_home = config_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));

        let toml_config = if config_path.exists() {
            let content = std::fs::read_to_string(config_path)
                .with_context(|| format!("Failed to read {}", config_path.display()))?;
            toml::from_str::<NoriConfigToml>(&content)
                .with_context(|| format!("Failed to parse {}", config_path.display()))?
        } else {
            NoriConfigToml::default()
        };

        Self::from_toml(toml_config, nori_home, NoriConfigOverrides::default())
    }

    /// Build resolved config from TOML + overrides
    fn from_toml(
        toml: NoriConfigToml,
        nori_home: PathBuf,
        overrides: NoriConfigOverrides,
    ) -> Result<Self> {
        let cwd = overrides
            .cwd
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();

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

        // Resolve skillset_per_session and auto_worktree (skillset_per_session forces auto_worktree on)
        let skillset_per_session = toml.tui.skillset_per_session.unwrap_or(false);
        let auto_worktree = skillset_per_session || toml.tui.auto_worktree.unwrap_or(false);

        // Active agent is the runtime value: CLI override > config model > persisted agent > DEFAULT_AGENT
        // Using agent as fallback ensures the persisted preference is honored at startup
        let active_agent = overrides
            .agent
            .or(toml.model)
            .unwrap_or_else(|| agent.clone());

        Ok(Self {
            agent,
            active_agent,
            sandbox_mode: overrides
                .sandbox_mode
                .or(toml.sandbox_mode)
                .unwrap_or(SandboxMode::WorkspaceWrite),
            approval_policy: overrides
                .approval_policy
                .or(toml.approval_policy)
                .unwrap_or(ApprovalPolicy::OnRequest),
            history_persistence: toml
                .history_persistence
                .unwrap_or(super::types::HistoryPersistence::SaveAll),
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
            vim_mode: toml.tui.vim_mode.unwrap_or(false),
            hotkeys: super::types::HotkeyConfig::from_toml(&toml.tui.hotkeys),
            script_timeout: toml.tui.script_timeout.unwrap_or_default(),
            loop_count: toml.tui.loop_count,
            skillset_per_session,
            auto_worktree,
            footer_segment_config: super::types::FooterSegmentConfig::from_toml(
                &toml.tui.footer_segments,
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
        })
    }
}

/// Resolve MCP server configurations from TOML
fn resolve_mcp_servers(
    toml_servers: HashMap<String, super::types::McpServerConfigToml>,
) -> Result<HashMap<String, McpServerConfig>> {
    let mut resolved = HashMap::new();

    for (name, server_toml) in toml_servers {
        match server_toml.resolve() {
            Ok(config) => {
                resolved.insert(name, config);
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "Invalid MCP server configuration '{name}': {e}"
                ));
            }
        }
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
model = "claude-code"

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
        let err = result.unwrap_err().to_string();
        assert!(err.contains("invalid"));
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

        // The agent field should be loaded and used to determine the model
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
    fn test_model_uses_persisted_agent_as_fallback() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        // Write a config with only agent set (no model specified)
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
    fn test_auto_worktree_enabled_from_config() {
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
        assert!(config.auto_worktree);
    }

    #[test]
    fn test_auto_worktree_defaults_to_false() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(&config_path, "").unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(!config.auto_worktree);
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
        assert!(
            config.auto_worktree,
            "auto_worktree should be forced true when skillset_per_session is enabled"
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
    fn test_skillset_per_session_forces_auto_worktree() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join(CONFIG_FILE);

        std::fs::write(
            &config_path,
            r#"
[tui]
skillset_per_session = true
auto_worktree = false
"#,
        )
        .unwrap();

        let config = NoriConfig::load_from_path(&config_path).unwrap();
        assert!(
            config.auto_worktree,
            "auto_worktree should be true even when explicitly set to false, because skillset_per_session forces it"
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
        assert!(config.vim_mode);
    }
}
