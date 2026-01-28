//! ACP agent registry
//!
//! Provides configuration for ACP agents (subprocess command and args)
//! with embedded provider info to avoid circular dependencies with core.
//!
//! ## Agent Names
//! - `claude-code` - Anthropic's Claude Code CLI agent
//! - `codex` - OpenAI's Codex CLI agent
//! - `gemini` - Google's Gemini CLI agent
//!
//! ## Provider Names
//! - `anthropic` - Anthropic
//! - `openai` - OpenAI
//! - `google` - Google

use anyhow::Result;
use std::collections::HashMap;
use std::fmt;
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

/// Default idle timeout for ACP streaming (5 minutes)
const DEFAULT_STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

// =============================================================================
// Core Enums
// =============================================================================

/// ACP agent identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AgentKind {
    /// Anthropic's Claude Code CLI
    ClaudeCode,
    /// OpenAI's Codex CLI
    Codex,
    /// Google's Gemini CLI
    Gemini,
}

impl AgentKind {
    /// Get the slug (string identifier) for this agent
    pub fn slug(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude-code",
            AgentKind::Codex => "codex",
            AgentKind::Gemini => "gemini",
        }
    }

    /// Get the display name for this agent
    pub fn display_name(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
            AgentKind::Gemini => "Gemini",
        }
    }

    /// Get the context window size (in tokens) for this agent.
    ///
    /// These are approximate values based on typical model configurations:
    /// - Claude Code: 200K tokens
    /// - Codex: 258K tokens  
    /// - Gemini: 1M tokens
    pub fn context_window_size(&self) -> i64 {
        match self {
            AgentKind::ClaudeCode => 200_000,
            AgentKind::Codex => 258_000,
            AgentKind::Gemini => 1_000_000,
        }
    }

    /// Get the provider for this agent
    pub fn provider(&self) -> Provider {
        match self {
            AgentKind::ClaudeCode => Provider::Anthropic,
            AgentKind::Codex => Provider::OpenAI,
            AgentKind::Gemini => Provider::Google,
        }
    }

    /// Get the npm package name for the underlying agent (for installation detection)
    pub fn npm_package(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "@anthropic-ai/claude-code",
            AgentKind::Codex => "@openai/codex",
            AgentKind::Gemini => "@google/gemini-cli",
        }
    }

    /// Get the ACP adapter package name for launching this agent
    pub fn acp_package(&self) -> &'static str {
        match self {
            // Claude and Codex use Zed's ACP adapters
            AgentKind::ClaudeCode => "@zed-industries/claude-code-acp",
            AgentKind::Codex => "@zed-industries/codex-acp",
            // Gemini has native ACP support
            AgentKind::Gemini => "@google/gemini-cli",
        }
    }

    /// Get all agent variants
    pub fn all() -> &'static [AgentKind] {
        &[AgentKind::ClaudeCode, AgentKind::Codex, AgentKind::Gemini]
    }

    /// Get authentication hint for this agent.
    ///
    /// Returns actionable instructions on how to authenticate with this agent's provider.
    pub fn auth_hint(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Run /login for instructions, or set ANTHROPIC_API_KEY.",
            AgentKind::Codex => "Run /login to authenticate, or set OPENAI_API_KEY.",
            AgentKind::Gemini => "Run /login for instructions, or set GOOGLE_API_KEY.",
        }
    }

    /// Parse an agent from a string slug
    pub fn from_slug(slug: &str) -> Option<AgentKind> {
        let normalized = slug.to_lowercase();
        match normalized.as_str() {
            "claude-code" | "claude" | "claude-acp" | "claude-4.5" => Some(AgentKind::ClaudeCode),
            "codex" | "codex-acp" => Some(AgentKind::Codex),
            "gemini" | "gemini-acp" | "gemini-2.5-flash" => Some(AgentKind::Gemini),
            _ => None,
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.slug())
    }
}

/// Model provider identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    /// Anthropic
    Anthropic,
    /// OpenAI
    OpenAI,
    /// Google
    Google,
}

impl Provider {
    /// Get the slug (string identifier) for this provider
    pub fn slug(&self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAI => "openai",
            Provider::Google => "google",
        }
    }

    /// Get the display name for this provider
    pub fn display_name(&self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::OpenAI => "OpenAI",
            Provider::Google => "Google",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.slug())
    }
}

/// Package manager used to install/run the agent
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PackageManager {
    /// npm (Node Package Manager)
    Npm,
    /// bun
    Bun,
}

impl PackageManager {
    /// Get the command name for this package manager
    pub fn command(&self) -> &'static str {
        match self {
            PackageManager::Npm => "npx",
            PackageManager::Bun => "bunx",
        }
    }

    /// Get the display name for this package manager
    pub fn display_name(&self) -> &'static str {
        match self {
            PackageManager::Npm => "npm",
            PackageManager::Bun => "bun",
        }
    }
}

impl fmt::Display for PackageManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

// =============================================================================
// Agent Info (for UI)
// =============================================================================

/// Information about an available ACP agent for display in the picker
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAgentInfo {
    /// Agent identifier
    pub agent: AgentKind,
    /// Model name used to select this agent (e.g., "claude-code", "gemini")
    pub model_name: String,
    /// Display name shown in the picker
    pub display_name: String,
    /// Description of the agent
    pub description: String,
    /// Provider slug for this agent
    pub provider_slug: String,
    /// Whether the agent is currently installed
    pub is_installed: bool,
    /// Package manager used to manage this agent (if installed)
    pub managed_by: Option<PackageManager>,
}

impl AcpAgentInfo {
    /// Create agent info from an AgentKind enum
    pub fn from_agent(agent: AgentKind) -> Self {
        let (is_installed, managed_by) = detect_agent_installation(agent);

        Self {
            agent,
            model_name: agent.slug().to_string(),
            display_name: agent.display_name().to_string(),
            description: agent.provider().display_name().to_string(),
            provider_slug: agent.slug().to_string(),
            is_installed,
            managed_by,
        }
    }
}

// =============================================================================
// Installation Detection
// =============================================================================

/// Cache for agent installation detection (PATH-based, fast).
/// Caching avoids repeated subprocess calls on every picker open.
static INSTALLATION_CACHE: OnceLock<HashMap<AgentKind, (bool, Option<PackageManager>)>> =
    OnceLock::new();

/// Pre-warm the installation detection cache.
///
/// Detection is now fast (PATH lookup only), so prewarming is optional.
/// Call this at app startup to avoid any startup latency from `which` commands.
pub fn prewarm_installation_cache() {
    let _ = INSTALLATION_CACHE.get_or_init(|| {
        let mut map = HashMap::new();
        for kind in AgentKind::all() {
            map.insert(*kind, detect_agent_installation_uncached(*kind));
        }
        map
    });
}

/// Detect if an agent is installed and which package manager manages it.
///
/// Uses PATH-based detection (fast `which` command) with caching.
/// Agents not in PATH can still be launched via npx/bunx.
fn detect_agent_installation(agent: AgentKind) -> (bool, Option<PackageManager>) {
    let cache = INSTALLATION_CACHE.get_or_init(|| {
        let mut map = HashMap::new();
        for kind in AgentKind::all() {
            map.insert(*kind, detect_agent_installation_uncached(*kind));
        }
        map
    });

    cache.get(&agent).copied().unwrap_or((false, None))
}

/// Perform the actual installation detection (uncached).
///
/// Only checks PATH for fast detection. Agents not in PATH can still be
/// launched via npx/bunx (which downloads and runs on-the-fly if needed).
fn detect_agent_installation_uncached(agent: AgentKind) -> (bool, Option<PackageManager>) {
    let binary_name = match agent {
        AgentKind::ClaudeCode => "claude",
        AgentKind::Codex => "codex",
        AgentKind::Gemini => "gemini",
    };

    // Check if the binary exists in PATH (fast check)
    if let Ok(output) = Command::new("which").arg(binary_name).output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let managed_by = detect_package_manager_from_path(&path);
        return (true, managed_by);
    }

    // On Windows, try `where` instead
    #[cfg(target_os = "windows")]
    if let Ok(output) = Command::new("where").arg(binary_name).output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let managed_by = detect_package_manager_from_path(&path);
            return (true, managed_by);
        }
    }

    // Binary not in PATH - agent is not locally installed.
    // It can still be launched via npx/bunx which will download on-the-fly.
    (false, None)
}

/// Detect package manager from binary path
fn detect_package_manager_from_path(path: &str) -> Option<PackageManager> {
    let path_lower = path.to_lowercase();
    if path_lower.contains(".bun") || path_lower.contains("bun/bin") {
        Some(PackageManager::Bun)
    } else if path_lower.contains("npm")
        || path_lower.contains("node_modules")
        || path_lower.contains(".npm")
    {
        Some(PackageManager::Npm)
    } else {
        // Default to npm if we can't determine
        Some(PackageManager::Npm)
    }
}

/// Detect the preferred package manager for launching ACP agents.
///
/// Priority:
/// 1. NORI_MANAGED_BY_BUN env var → use bunx
/// 2. NORI_MANAGED_BY_NPM env var → use npx
/// 3. bun in PATH → use bunx
/// 4. Default to npx
pub fn detect_preferred_package_manager() -> PackageManager {
    // Check explicit env var overrides first
    if std::env::var("NORI_MANAGED_BY_BUN").is_ok() {
        return PackageManager::Bun;
    }
    if std::env::var("NORI_MANAGED_BY_NPM").is_ok() {
        return PackageManager::Npm;
    }

    // Check if bun is available in PATH
    if Command::new("bun").arg("--version").output().is_ok() {
        return PackageManager::Bun;
    }

    // Default to npm
    PackageManager::Npm
}

// =============================================================================
// Agent Config (for spawning)
// =============================================================================

/// Provider information embedded in ACP agent config.
/// This mirrors relevant fields from `ModelProviderInfo` to avoid circular dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpProviderInfo {
    /// Friendly display name (e.g., "Gemini ACP", "Mock ACP")
    pub name: String,
    /// Maximum number of request retries
    pub request_max_retries: u64,
    /// Maximum number of stream reconnection attempts
    pub stream_max_retries: u64,
    /// Idle timeout for streaming responses
    pub stream_idle_timeout: Duration,
}

impl Default for AcpProviderInfo {
    fn default() -> Self {
        Self {
            name: "ACP".to_string(),
            request_max_retries: 1,
            stream_max_retries: 1,
            stream_idle_timeout: DEFAULT_STREAM_IDLE_TIMEOUT,
        }
    }
}

/// Configuration for an ACP agent subprocess
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpAgentConfig {
    /// Agent identifier
    pub agent: AgentKind,
    /// Provider identifier (e.g., "claude-code", "gemini")
    /// Used to determine when subprocess can be reused vs needs replacement
    pub provider_slug: String,
    /// Command to execute (binary path or command name)
    pub command: String,
    /// Arguments to pass to the command
    pub args: Vec<String>,
    /// Environment variables to set for the subprocess
    pub env: HashMap<String, String>,
    /// Provider information for this ACP agent
    pub provider_info: AcpProviderInfo,
    /// Authentication hint for this agent (displayed on auth failures)
    pub auth_hint: String,
}

/// Get list of all available ACP agents for the agent picker
pub fn list_available_agents() -> Vec<AcpAgentInfo> {
    let mut agents = Vec::new();

    // Mock agents are only available in debug builds (for testing)
    #[cfg(debug_assertions)]
    {
        agents.push(AcpAgentInfo {
            agent: AgentKind::ClaudeCode, // Dummy, not really used
            model_name: "mock-model".to_string(),
            display_name: "Mock ACP".to_string(),
            description: "Mock agent for testing".to_string(),
            provider_slug: "mock-acp".to_string(),
            is_installed: true,
            managed_by: None,
        });
        agents.push(AcpAgentInfo {
            agent: AgentKind::ClaudeCode, // Dummy, not really used
            model_name: "mock-model-alt".to_string(),
            display_name: "Mock ACP Alt".to_string(),
            description: "Alternate mock agent for testing".to_string(),
            provider_slug: "mock-acp-alt".to_string(),
            is_installed: true,
            managed_by: None,
        });
    }

    // Production agents
    for agent in AgentKind::all() {
        agents.push(AcpAgentInfo::from_agent(*agent));
    }

    agents
}

/// Get ACP agent configuration for a given model name
///
/// # Arguments
/// * `model_name` - The model identifier (e.g., "claude-code", "gemini")
///   Names are normalized to lowercase for case-insensitive matching.
///
/// # Returns
/// Configuration with provider_slug, command and args to spawn the agent subprocess
///
/// # Errors
/// Returns error if model_name is not recognized
pub fn get_agent_config(model_name: &str) -> Result<AcpAgentConfig> {
    // Normalize model name: lowercase
    let normalized = model_name.to_lowercase();

    // Try mock agents first (only available in debug builds)
    #[cfg(debug_assertions)]
    if let Some(config) = get_mock_agent_config(&normalized) {
        return Ok(config);
    }

    // Try to parse as an AgentKind
    if let Some(agent) = AgentKind::from_slug(&normalized) {
        let package_manager = detect_preferred_package_manager();

        let (command, args) = match agent {
            // Claude and Codex use Zed's ACP adapters
            AgentKind::ClaudeCode => (
                package_manager.command().to_string(),
                vec!["@zed-industries/claude-code-acp".to_string()],
            ),
            AgentKind::Codex => (
                package_manager.command().to_string(),
                vec!["@zed-industries/codex-acp".to_string()],
            ),
            // Gemini has native ACP support via --experimental-acp flag
            AgentKind::Gemini => (
                package_manager.command().to_string(),
                vec![
                    "@google/gemini-cli".to_string(),
                    "--experimental-acp".to_string(),
                ],
            ),
        };

        return Ok(AcpAgentConfig {
            agent,
            provider_slug: agent.slug().to_string(),
            command,
            args,
            env: HashMap::new(),
            provider_info: AcpProviderInfo {
                name: format!("{} ACP", agent.display_name()),
                ..Default::default()
            },
            auth_hint: agent.auth_hint().to_string(),
        });
    }

    anyhow::bail!("Unknown ACP model: {model_name}")
}

/// Get the display name for an agent by model name.
///
/// Returns the human-readable display name if the agent is registered.
/// Falls back to the model_name itself if not recognized.
pub fn get_agent_display_name(model_name: &str) -> String {
    let normalized = model_name.to_lowercase();

    // Mock agents (debug builds only)
    #[cfg(debug_assertions)]
    {
        if normalized == "mock-model" {
            return "Mock ACP".to_string();
        }
        if normalized == "mock-model-alt" {
            return "Mock ACP Alt".to_string();
        }
    }

    // Production agents
    if let Some(agent) = AgentKind::from_slug(&normalized) {
        return agent.display_name().to_string();
    }

    // Fallback to model name
    model_name.to_string()
}

/// Get mock agent configuration (only available in debug builds)
#[cfg(debug_assertions)]
fn get_mock_agent_config(normalized: &str) -> Option<AcpAgentConfig> {
    match normalized {
        "mock-model" => {
            // Resolve path to mock_acp_agent binary.
            //
            // Priority:
            // 1. MOCK_ACP_AGENT_BIN environment variable (set by CI)
            // 2. Relative to current executable (for local development)
            let exe_path = if let Ok(env_path) = std::env::var("MOCK_ACP_AGENT_BIN") {
                tracing::debug!("Mock ACP agent path from MOCK_ACP_AGENT_BIN: {}", env_path);
                std::path::PathBuf::from(env_path)
            } else {
                // Fall back to resolving relative to current executable.
                // This handles both:
                // - Running as `codex` binary: target/{profile}/codex -> target/{profile}/
                // - Running as test binary: target/{profile}/deps/test -> target/{profile}/
                let mock_path = std::env::current_exe()
                    .ok()
                    .and_then(|p| {
                        p.parent().map(|parent| {
                            // Check if we're in a "deps" directory (test binary context)
                            let in_deps_dir = parent
                                .file_name()
                                .map(|name| name == "deps")
                                .unwrap_or(false);

                            if in_deps_dir {
                                parent.parent().map(|p| p.join("mock_acp_agent"))
                            } else {
                                Some(parent.join("mock_acp_agent"))
                            }
                        })
                    })
                    .flatten()
                    .unwrap_or_else(|| std::path::PathBuf::from("mock_acp_agent"));
                tracing::debug!("Mock ACP agent path resolved to: {}", mock_path.display());
                mock_path
            };

            Some(AcpAgentConfig {
                agent: AgentKind::ClaudeCode, // Dummy
                provider_slug: "mock-acp".to_string(),
                command: exe_path.to_string_lossy().to_string(),
                args: vec![],
                env: HashMap::from([(
                    "MOCK_AGENT_MODEL_NAME".to_string(),
                    "mock-model".to_string(),
                )]),
                provider_info: AcpProviderInfo {
                    name: "Mock ACP".to_string(),
                    ..Default::default()
                },
                auth_hint: "Mock agent - no authentication required.".to_string(),
            })
        }
        "mock-model-alt" => {
            // Alternate mock model for E2E testing agent switching.
            // Uses the same binary as mock-model but with a different provider_slug
            // to verify that different agent configurations spawn separate subprocesses.
            let exe_path = if let Ok(env_path) = std::env::var("MOCK_ACP_AGENT_BIN") {
                tracing::debug!("Mock ACP agent path from MOCK_ACP_AGENT_BIN: {}", env_path);
                std::path::PathBuf::from(env_path)
            } else {
                let mock_path = std::env::current_exe()
                    .ok()
                    .and_then(|p| {
                        p.parent().map(|parent| {
                            let in_deps_dir = parent
                                .file_name()
                                .map(|name| name == "deps")
                                .unwrap_or(false);

                            if in_deps_dir {
                                parent.parent().map(|p| p.join("mock_acp_agent"))
                            } else {
                                Some(parent.join("mock_acp_agent"))
                            }
                        })
                    })
                    .flatten()
                    .unwrap_or_else(|| std::path::PathBuf::from("mock_acp_agent"));
                tracing::debug!("Mock ACP agent path resolved to: {}", mock_path.display());
                mock_path
            };

            Some(AcpAgentConfig {
                agent: AgentKind::ClaudeCode, // Dummy
                provider_slug: "mock-acp-alt".to_string(),
                command: exe_path.to_string_lossy().to_string(),
                args: vec![],
                env: HashMap::from([(
                    "MOCK_AGENT_MODEL_NAME".to_string(),
                    "mock-model-alt".to_string(),
                )]),
                provider_info: AcpProviderInfo {
                    name: "Mock ACP Alt".to_string(),
                    ..Default::default()
                },
                auth_hint: "Mock agent - no authentication required.".to_string(),
            })
        }
        _ => None,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_slugs() {
        assert_eq!(AgentKind::ClaudeCode.slug(), "claude-code");
        assert_eq!(AgentKind::Codex.slug(), "codex");
        assert_eq!(AgentKind::Gemini.slug(), "gemini");
    }

    #[test]
    fn test_provider_slugs() {
        assert_eq!(Provider::Anthropic.slug(), "anthropic");
        assert_eq!(Provider::OpenAI.slug(), "openai");
        assert_eq!(Provider::Google.slug(), "google");
    }

    #[test]
    fn test_agent_from_slug() {
        // Direct slugs
        assert_eq!(
            AgentKind::from_slug("claude-code"),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(AgentKind::from_slug("codex"), Some(AgentKind::Codex));
        assert_eq!(AgentKind::from_slug("gemini"), Some(AgentKind::Gemini));

        // Legacy aliases
        assert_eq!(
            AgentKind::from_slug("claude-acp"),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(
            AgentKind::from_slug("claude-4.5"),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(AgentKind::from_slug("codex-acp"), Some(AgentKind::Codex));
        assert_eq!(AgentKind::from_slug("gemini-acp"), Some(AgentKind::Gemini));
        assert_eq!(
            AgentKind::from_slug("gemini-2.5-flash"),
            Some(AgentKind::Gemini)
        );

        // Case insensitive
        assert_eq!(
            AgentKind::from_slug("Claude-Code"),
            Some(AgentKind::ClaudeCode)
        );
        assert_eq!(AgentKind::from_slug("CODEX"), Some(AgentKind::Codex));

        // Unknown
        assert_eq!(AgentKind::from_slug("unknown"), None);
    }

    #[test]
    fn test_agent_provider_relationship() {
        assert_eq!(AgentKind::ClaudeCode.provider(), Provider::Anthropic);
        assert_eq!(AgentKind::Codex.provider(), Provider::OpenAI);
        assert_eq!(AgentKind::Gemini.provider(), Provider::Google);
    }

    #[test]
    fn test_package_manager_commands() {
        assert_eq!(PackageManager::Npm.command(), "npx");
        assert_eq!(PackageManager::Bun.command(), "bunx");
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_get_mock_model_config() {
        let config = get_agent_config("mock-model").expect("Should return config for mock-model");

        assert_eq!(config.provider_slug, "mock-acp");
        assert!(
            config.command.contains("mock_acp_agent"),
            "Command should contain 'mock_acp_agent', got: {}",
            config.command
        );
        assert_eq!(config.args, Vec::<String>::new());
        assert_eq!(config.provider_info.name, "Mock ACP");
        assert_eq!(config.provider_info.request_max_retries, 1);
        assert_eq!(config.provider_info.stream_max_retries, 1);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_get_mock_model_alt_config() {
        let config =
            get_agent_config("mock-model-alt").expect("Should return config for mock-model-alt");

        assert_eq!(config.provider_slug, "mock-acp-alt");
        assert!(
            config.command.contains("mock_acp_agent"),
            "Command should contain 'mock_acp_agent', got: {}",
            config.command
        );
        assert_eq!(config.args, Vec::<String>::new());
        assert_eq!(config.provider_info.name, "Mock ACP Alt");
    }

    #[test]
    fn test_get_claude_code_config() {
        let config = get_agent_config("claude-code").expect("Should return config for claude-code");

        assert_eq!(config.provider_slug, "claude-code");
        assert_eq!(config.agent, AgentKind::ClaudeCode);
        // Command should be npx or bunx
        assert!(
            config.command == "npx" || config.command == "bunx",
            "Command should be npx or bunx, got: {}",
            config.command
        );
        // Uses Zed's ACP adapter
        assert!(
            config
                .args
                .contains(&"@zed-industries/claude-code-acp".to_string())
        );
        assert_eq!(config.provider_info.name, "Claude Code ACP");
    }

    #[test]
    fn test_get_codex_config() {
        let config = get_agent_config("codex").expect("Should return config for codex");

        assert_eq!(config.provider_slug, "codex");
        assert_eq!(config.agent, AgentKind::Codex);
        assert!(
            config.command == "npx" || config.command == "bunx",
            "Command should be npx or bunx, got: {}",
            config.command
        );
        // Uses Zed's ACP adapter
        assert!(
            config
                .args
                .contains(&"@zed-industries/codex-acp".to_string())
        );
        assert_eq!(config.provider_info.name, "Codex ACP");
    }

    #[test]
    fn test_get_gemini_config() {
        let config = get_agent_config("gemini").expect("Should return config for gemini");

        assert_eq!(config.provider_slug, "gemini");
        assert_eq!(config.agent, AgentKind::Gemini);
        assert!(
            config.command == "npx" || config.command == "bunx",
            "Command should be npx or bunx, got: {}",
            config.command
        );
        assert!(config.args.contains(&"@google/gemini-cli".to_string()));
        assert!(config.args.contains(&"--experimental-acp".to_string()));
        assert_eq!(config.provider_info.name, "Gemini ACP");
    }

    #[test]
    fn test_legacy_model_names() {
        // Claude legacy names
        assert!(get_agent_config("claude-acp").is_ok());
        assert!(get_agent_config("claude-4.5").is_ok());

        // Codex legacy names
        assert!(get_agent_config("codex-acp").is_ok());

        // Gemini legacy names
        assert!(get_agent_config("gemini-acp").is_ok());
        assert!(get_agent_config("gemini-2.5-flash").is_ok());
    }

    #[test]
    fn test_get_unknown_model_returns_error() {
        let result = get_agent_config("unknown-model-xyz");

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("unknown-model-xyz"));
    }

    #[test]
    fn test_get_agent_config_normalizes_model_names() {
        // Should work with lowercase model names
        assert!(
            get_agent_config("claude-code").is_ok(),
            "Lowercase 'claude-code' should work"
        );

        // Should work with mixed case (normalized to lowercase)
        let claude_result = get_agent_config("Claude-Code");
        assert!(
            claude_result.is_ok(),
            "Mixed case 'Claude-Code' should work"
        );
        assert_eq!(
            claude_result.unwrap().provider_slug,
            "claude-code",
            "Should resolve to claude-code provider"
        );

        // Should still reject unknown models
        let unknown_result = get_agent_config("unknown-model-xyz");
        assert!(unknown_result.is_err(), "Unknown model should return error");
        let err_msg = unknown_result.unwrap_err().to_string();
        assert!(
            err_msg.contains("unknown-model-xyz"),
            "Error message should contain original input"
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_list_available_agents_debug_build() {
        let agents = list_available_agents();
        // Debug build should have 5 agents: mock, mock-alt, claude-code, codex, gemini
        assert_eq!(agents.len(), 5, "Debug build should have 5 agents");

        let names: Vec<&str> = agents.iter().map(|a| a.display_name.as_str()).collect();
        assert!(names.contains(&"Mock ACP"), "Should have Mock ACP");
        assert!(names.contains(&"Mock ACP Alt"), "Should have Mock ACP Alt");
        assert!(names.contains(&"Claude Code"), "Should have Claude Code");
        assert!(names.contains(&"Codex"), "Should have Codex");
        assert!(names.contains(&"Gemini"), "Should have Gemini");
    }

    #[test]
    fn test_list_available_agents_contains_production_agents() {
        let agents = list_available_agents();
        let names: Vec<&str> = agents.iter().map(|a| a.display_name.as_str()).collect();

        // Production agents should always be present
        assert!(names.contains(&"Claude Code"), "Should have Claude Code");
        assert!(names.contains(&"Codex"), "Should have Codex");
        assert!(names.contains(&"Gemini"), "Should have Gemini");
    }

    #[test]
    fn test_auth_hint_returns_actionable_instructions() {
        // Claude Code should mention `/login` for instructions
        let claude_hint = AgentKind::ClaudeCode.auth_hint();
        assert!(
            claude_hint.contains("/login"),
            "Claude hint should mention '/login', got: {claude_hint}"
        );

        // Codex should mention `/login` to authenticate
        let codex_hint = AgentKind::Codex.auth_hint();
        assert!(
            codex_hint.contains("/login"),
            "Codex hint should mention '/login', got: {codex_hint}"
        );

        // Gemini should mention `/login` for instructions
        let gemini_hint = AgentKind::Gemini.auth_hint();
        assert!(
            gemini_hint.contains("/login"),
            "Gemini hint should mention '/login', got: {gemini_hint}"
        );
    }

    #[test]
    fn test_agent_config_includes_auth_hint() {
        // Get config for claude-code and verify it has an auth hint with /login
        let config = get_agent_config("claude-code").expect("Should return config");
        assert!(
            !config.auth_hint.is_empty(),
            "Config should have a non-empty auth_hint"
        );
        assert!(
            config.auth_hint.contains("/login"),
            "Claude config auth_hint should mention '/login', got: {}",
            config.auth_hint
        );

        // Get config for codex and verify it has an auth hint with /login
        let config = get_agent_config("codex").expect("Should return config");
        assert!(
            !config.auth_hint.is_empty(),
            "Codex config should have a non-empty auth_hint"
        );
        assert!(
            config.auth_hint.contains("/login"),
            "Codex config auth_hint should mention '/login', got: {}",
            config.auth_hint
        );

        // Get config for gemini and verify it has an auth hint with /login
        let config = get_agent_config("gemini").expect("Should return config");
        assert!(
            !config.auth_hint.is_empty(),
            "Gemini config should have a non-empty auth_hint"
        );
        assert!(
            config.auth_hint.contains("/login"),
            "Gemini config auth_hint should mention '/login', got: {}",
            config.auth_hint
        );
    }
}
