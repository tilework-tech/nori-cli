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
use std::collections::HashSet;
use std::fmt;
use std::process::Command;
use std::sync::OnceLock;
use std::sync::RwLock;
use std::time::Duration;

use crate::config::AgentConfigToml;
use crate::config::ResolvedDistribution;

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
    /// - Claude Code: 1M tokens
    /// - Codex: 258K tokens
    /// - Gemini: 1M tokens
    pub fn context_window_size(&self) -> i64 {
        match self {
            AgentKind::ClaudeCode => 1_000_000,
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
            AgentKind::ClaudeCode => "@agentclientprotocol/claude-agent-acp",
            // Codex uses Zed's ACP adapter
            AgentKind::Codex => "@zed-industries/codex-acp",
            // Gemini has native ACP support
            AgentKind::Gemini => "@google/gemini-cli",
        }
    }

    /// Get all agent variants
    pub fn all() -> &'static [AgentKind] {
        &[AgentKind::ClaudeCode, AgentKind::Codex, AgentKind::Gemini]
    }

    /// Get the base directory for transcript files, relative to home directory.
    ///
    /// Returns the path where this agent stores session transcript files.
    pub fn transcript_base_dir(&self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => ".claude/projects",
            AgentKind::Codex => ".codex/sessions",
            AgentKind::Gemini => ".gemini/tmp",
        }
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
// Data-driven Registry
// =============================================================================

/// A registered agent entry in the data-driven registry.
///
/// Built-in agents have `kind` set to `Some(AgentKind)` for first-class metadata.
/// Custom agents (from config) have `kind` set to `None`.
#[derive(Debug, Clone)]
pub struct RegisteredAgent {
    /// Display name shown in the agent picker (e.g. "Claude Code")
    pub name: String,
    /// Machine identifier / slug (e.g. "claude-code")
    pub slug: String,
    /// First-class agent kind, if this is a built-in agent
    pub kind: Option<AgentKind>,
    /// How this agent is invoked (None for built-in agents that use auto-detection)
    pub distribution: Option<ResolvedDistribution>,
    /// Context window size override (in tokens)
    pub context_window_size: Option<i64>,
    /// Auth hint displayed on auth failures
    pub auth_hint: Option<String>,
    /// Transcript base directory (relative to home)
    pub transcript_base_dir: Option<String>,
}

/// Global agent registry. Protected by RwLock so tests can reset it.
static AGENT_REGISTRY: RwLock<Option<Vec<RegisteredAgent>>> = RwLock::new(None);

/// Build the three default (built-in) agents.
pub fn build_default_agents() -> Vec<RegisteredAgent> {
    AgentKind::all()
        .iter()
        .map(|kind| RegisteredAgent {
            name: kind.display_name().to_string(),
            slug: kind.slug().to_string(),
            kind: Some(*kind),
            distribution: None, // built-ins use auto-detection
            context_window_size: Some(kind.context_window_size()),
            auth_hint: Some(kind.auth_hint().to_string()),
            transcript_base_dir: Some(kind.transcript_base_dir().to_string()),
        })
        .collect()
}

/// Build the full registry from built-in defaults + custom agent configs.
///
/// Custom agents with a slug matching a built-in override the built-in entry.
/// Duplicate slugs among custom agents are rejected.
pub fn build_registry(custom_agents: Vec<AgentConfigToml>) -> Result<Vec<RegisteredAgent>> {
    let mut agents = build_default_agents();

    // Check for duplicate slugs among custom agents
    let mut seen_custom_slugs = HashSet::new();
    for custom in &custom_agents {
        if !seen_custom_slugs.insert(custom.slug.clone()) {
            anyhow::bail!(
                "Duplicate custom agent slug: '{}'. Each agent must have a unique slug.",
                custom.slug
            );
        }
    }

    for custom in custom_agents {
        let resolved = custom.distribution.resolve().map_err(|e| {
            anyhow::anyhow!("Invalid distribution for agent '{}': {e}", custom.slug)
        })?;

        let registered = RegisteredAgent {
            name: custom.name,
            slug: custom.slug.clone(),
            kind: None,
            distribution: Some(resolved),
            context_window_size: custom.context_window_size,
            auth_hint: custom.auth_hint,
            transcript_base_dir: custom.transcript_base_dir,
        };

        // Override built-in if slug matches, otherwise append
        if let Some(existing) = agents.iter_mut().find(|a| a.slug == registered.slug) {
            *existing = registered;
        } else {
            agents.push(registered);
        }
    }

    Ok(agents)
}

/// Initialize the global agent registry with custom agents from config.
///
/// This should be called once at startup after loading config.
/// If called multiple times, later calls replace the registry.
pub fn initialize_registry(custom_agents: Vec<AgentConfigToml>) -> Result<()> {
    let agents = build_registry(custom_agents)?;
    let mut registry = AGENT_REGISTRY
        .write()
        .map_err(|e| anyhow::anyhow!("agent registry lock poisoned: {e}"))?;
    *registry = Some(agents);
    Ok(())
}

/// Get the current registry, falling back to defaults if not initialized.
fn get_registry() -> Vec<RegisteredAgent> {
    match AGENT_REGISTRY.read() {
        Ok(guard) => match &*guard {
            Some(agents) => agents.clone(),
            None => build_default_agents(),
        },
        Err(_) => build_default_agents(),
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
    /// Agent name used to select this agent (e.g., "claude-code", "gemini")
    pub agent_name: String,
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
            agent_name: agent.slug().to_string(),
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
    /// Agent identifier (placeholder for custom agents)
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
    /// Display name for this agent (avoids going through AgentKind for custom agents)
    pub display_name: String,
    /// Install command hint for error messages (e.g. "npm install -g @pkg" or "ensure /usr/bin/x is in PATH")
    pub install_hint: String,
}

impl AcpAgentConfig {
    /// Context window size for this agent, if known.
    pub fn context_window_size(&self) -> Option<i64> {
        // Check registry first (may have custom override)
        let registry = get_registry();
        registry
            .iter()
            .find(|a| a.slug == self.provider_slug)
            .and_then(|a| a.context_window_size)
    }

    /// Transcript base directory for this agent, if known.
    pub fn transcript_base_dir(&self) -> Option<String> {
        let registry = get_registry();
        registry
            .iter()
            .find(|a| a.slug == self.provider_slug)
            .and_then(|a| a.transcript_base_dir.clone())
    }
}

/// Get list of all available ACP agents for the agent picker
pub fn list_available_agents() -> Vec<AcpAgentInfo> {
    let mut agents = Vec::new();

    // Mock agents are only available in debug builds (for testing)
    #[cfg(debug_assertions)]
    {
        agents.push(AcpAgentInfo {
            agent: AgentKind::ClaudeCode, // Dummy, not really used
            agent_name: "mock-model".to_string(),
            display_name: "Mock ACP".to_string(),
            description: "Mock agent for testing".to_string(),
            provider_slug: "mock-acp".to_string(),
            is_installed: true,
            managed_by: None,
        });
        agents.push(AcpAgentInfo {
            agent: AgentKind::ClaudeCode, // Dummy, not really used
            agent_name: "mock-model-alt".to_string(),
            display_name: "Mock ACP Alt".to_string(),
            description: "Alternate mock agent for testing".to_string(),
            provider_slug: "mock-acp-alt".to_string(),
            is_installed: true,
            managed_by: None,
        });
    }

    // Include all agents from the registry (built-in + custom)
    for registered in get_registry() {
        if let Some(kind) = registered.kind {
            // Built-in agent: use existing detection logic
            agents.push(AcpAgentInfo::from_agent(kind));
        } else {
            // Custom agent: always shown, installation detection not applicable
            agents.push(AcpAgentInfo {
                agent: AgentKind::ClaudeCode, // Placeholder for custom agents
                agent_name: registered.slug.clone(),
                display_name: registered.name.clone(),
                description: registered.slug.clone(),
                provider_slug: registered.slug,
                is_installed: true,
                managed_by: None,
            });
        }
    }

    agents
}

/// Get ACP agent configuration for a given agent name
///
/// # Arguments
/// * `agent_name` - The agent identifier (e.g., "claude-code", "gemini")
///   Names are normalized to lowercase for case-insensitive matching.
///
/// # Returns
/// Configuration with provider_slug, command and args to spawn the agent subprocess
///
/// # Errors
/// Returns error if agent_name is not recognized
pub fn get_agent_config(agent_name: &str) -> Result<AcpAgentConfig> {
    // Normalize agent name: lowercase
    let normalized = agent_name.to_lowercase();

    // Try mock agents first (only available in debug builds)
    #[cfg(debug_assertions)]
    if let Some(config) = get_mock_agent_config(&normalized) {
        return Ok(config);
    }

    // Check the data-driven registry first for custom/overridden agents
    let registry = get_registry();
    if let Some(registered) = registry.iter().find(|a| a.slug == normalized)
        && let Some(ref dist) = registered.distribution
    {
        // Custom distribution: use the resolved distribution directly
        let (command, args, env) = match dist {
            ResolvedDistribution::Local { command, args, env } => {
                (command.clone(), args.clone(), env.clone())
            }
            ResolvedDistribution::Npx { package, args } => {
                let mut full_args = vec![package.clone()];
                full_args.extend(args.iter().cloned());
                ("npx".to_string(), full_args, HashMap::new())
            }
            ResolvedDistribution::Bunx { package, args } => {
                let mut full_args = vec![package.clone()];
                full_args.extend(args.iter().cloned());
                ("bunx".to_string(), full_args, HashMap::new())
            }
            ResolvedDistribution::Pipx { package, args } => {
                let mut full_args = vec!["run".to_string(), package.clone()];
                full_args.extend(args.iter().cloned());
                ("pipx".to_string(), full_args, HashMap::new())
            }
            ResolvedDistribution::Uvx { package, args } => {
                let mut full_args = vec![package.clone()];
                full_args.extend(args.iter().cloned());
                ("uvx".to_string(), full_args, HashMap::new())
            }
        };

        let install_hint = match dist {
            ResolvedDistribution::Local { command, .. } => {
                format!("ensure '{command}' is in your PATH")
            }
            ResolvedDistribution::Npx { package, .. } => {
                format!("npm install -g {package}")
            }
            ResolvedDistribution::Bunx { package, .. } => {
                format!("bun add -g {package}")
            }
            ResolvedDistribution::Pipx { package, .. } => {
                format!("pipx install {package}")
            }
            ResolvedDistribution::Uvx { package, .. } => {
                format!("uv tool install {package}")
            }
        };

        return Ok(AcpAgentConfig {
            agent: registered.kind.unwrap_or(AgentKind::ClaudeCode),
            provider_slug: registered.slug.clone(),
            command,
            args,
            env,
            provider_info: AcpProviderInfo {
                name: format!("{} ACP", registered.name),
                ..Default::default()
            },
            auth_hint: registered.auth_hint.clone().unwrap_or_default(),
            display_name: registered.name.clone(),
            install_hint,
        });
    }

    // Try to parse as a built-in AgentKind (auto-detection path)
    if let Some(agent) = AgentKind::from_slug(&normalized) {
        let package_manager = detect_preferred_package_manager();

        let (command, args) = match agent {
            // Claude and Codex use external ACP adapters
            AgentKind::ClaudeCode | AgentKind::Codex => (
                package_manager.command().to_string(),
                vec![agent.acp_package().to_string()],
            ),
            // Gemini has native ACP support via --experimental-acp flag
            AgentKind::Gemini => (
                package_manager.command().to_string(),
                vec![
                    agent.acp_package().to_string(),
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
            display_name: agent.display_name().to_string(),
            install_hint: format!("npm install -g {}", agent.npm_package()),
        });
    }

    anyhow::bail!("Unknown ACP agent: {agent_name}")
}

/// Get the display name for an agent by agent name.
///
/// Returns the human-readable display name if the agent is registered.
/// Falls back to the agent_name itself if not recognized.
pub fn get_agent_display_name(agent_name: &str) -> String {
    let normalized = agent_name.to_lowercase();

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

    // Check the data-driven registry (includes both built-in and custom)
    let registry = get_registry();
    if let Some(registered) = registry.iter().find(|a| a.slug == normalized) {
        return registered.name.clone();
    }

    // Fallback: try AgentKind parsing for legacy aliases
    if let Some(agent) = AgentKind::from_slug(&normalized) {
        return agent.display_name().to_string();
    }

    // Fallback to agent name
    agent_name.to_string()
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
                display_name: "Mock ACP".to_string(),
                install_hint: "Mock agent - no installation required".to_string(),
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
                display_name: "Mock ACP Alt".to_string(),
                install_hint: "Mock agent - no installation required".to_string(),
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
    use serial_test::serial;

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
    fn test_get_mock_agent_config() {
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
    fn test_get_mock_agent_alt_config() {
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
        assert!(
            config
                .args
                .contains(&"@agentclientprotocol/claude-agent-acp".to_string())
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
    #[serial]
    #[cfg(debug_assertions)]
    fn test_list_available_agents_debug_build() {
        reset_registry();
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
    fn test_agent_transcript_base_dir() {
        // Each agent should return the correct base directory path relative to home
        assert_eq!(
            AgentKind::ClaudeCode.transcript_base_dir(),
            ".claude/projects"
        );
        assert_eq!(AgentKind::Codex.transcript_base_dir(), ".codex/sessions");
        assert_eq!(AgentKind::Gemini.transcript_base_dir(), ".gemini/tmp");
    }

    // ========================================================================
    // Data-driven registry tests
    // ========================================================================

    /// Reset the global registry so tests don't interfere with each other.
    fn reset_registry() {
        let mut registry = AGENT_REGISTRY.write().unwrap();
        *registry = None;
    }

    #[test]
    fn test_build_default_agents_returns_three_builtins() {
        let agents = build_default_agents();
        assert_eq!(agents.len(), 3);
        let slugs: Vec<&str> = agents.iter().map(|a| a.slug.as_str()).collect();
        assert!(slugs.contains(&"claude-code"));
        assert!(slugs.contains(&"codex"));
        assert!(slugs.contains(&"gemini"));
    }

    #[test]
    fn test_build_default_agents_have_correct_names() {
        let agents = build_default_agents();
        let claude = agents.iter().find(|a| a.slug == "claude-code").unwrap();
        assert_eq!(claude.name, "Claude Code");
        let codex = agents.iter().find(|a| a.slug == "codex").unwrap();
        assert_eq!(codex.name, "Codex");
        let gemini = agents.iter().find(|a| a.slug == "gemini").unwrap();
        assert_eq!(gemini.name, "Gemini");
    }

    #[test]
    fn test_build_default_agents_have_agent_kind() {
        let agents = build_default_agents();
        for agent in &agents {
            assert!(
                agent.kind.is_some(),
                "Built-in agent {} should have AgentKind",
                agent.slug
            );
        }
    }

    #[test]
    fn test_build_registry_with_no_custom_agents() {
        let registry = build_registry(vec![]).unwrap();
        assert_eq!(registry.len(), 3);
    }

    #[test]
    fn test_build_registry_appends_custom_agent() {
        use crate::config::AgentConfigToml;
        use crate::config::AgentDistributionToml;
        use crate::config::PackageDistribution;
        let custom = AgentConfigToml {
            name: "Kimi".to_string(),
            slug: "kimi".to_string(),
            distribution: AgentDistributionToml {
                uvx: Some(PackageDistribution {
                    package: "kimi-cli".to_string(),
                    args: vec!["acp".to_string()],
                }),
                ..Default::default()
            },
            context_window_size: None,
            auth_hint: None,
            transcript_base_dir: None,
        };
        let registry = build_registry(vec![custom]).unwrap();
        assert_eq!(registry.len(), 4);
        let kimi = registry.iter().find(|a| a.slug == "kimi").unwrap();
        assert_eq!(kimi.name, "Kimi");
        assert!(kimi.kind.is_none());
    }

    #[test]
    fn test_build_registry_custom_overrides_builtin() {
        use crate::config::AgentConfigToml;
        use crate::config::AgentDistributionToml;
        use crate::config::LocalDistribution;
        let custom_claude = AgentConfigToml {
            name: "My Claude".to_string(),
            slug: "claude-code".to_string(),
            distribution: AgentDistributionToml {
                local: Some(LocalDistribution {
                    command: "/my/claude".to_string(),
                    args: vec![],
                    env: std::collections::HashMap::new(),
                }),
                ..Default::default()
            },
            context_window_size: Some(300_000),
            auth_hint: Some("Use my auth".to_string()),
            transcript_base_dir: None,
        };
        let registry = build_registry(vec![custom_claude]).unwrap();
        // Should still be 3 agents (override, not add)
        assert_eq!(registry.len(), 3);
        let claude = registry.iter().find(|a| a.slug == "claude-code").unwrap();
        assert_eq!(claude.name, "My Claude");
        assert_eq!(claude.context_window_size, Some(300_000));
    }

    #[test]
    fn test_build_registry_rejects_duplicate_custom_slugs() {
        use crate::config::AgentConfigToml;
        use crate::config::AgentDistributionToml;
        use crate::config::PackageDistribution;
        let agents = vec![
            AgentConfigToml {
                name: "Agent A".to_string(),
                slug: "my-agent".to_string(),
                distribution: AgentDistributionToml {
                    npx: Some(PackageDistribution {
                        package: "pkg-a".to_string(),
                        args: vec![],
                    }),
                    ..Default::default()
                },
                context_window_size: None,
                auth_hint: None,
                transcript_base_dir: None,
            },
            AgentConfigToml {
                name: "Agent B".to_string(),
                slug: "my-agent".to_string(),
                distribution: AgentDistributionToml {
                    npx: Some(PackageDistribution {
                        package: "pkg-b".to_string(),
                        args: vec![],
                    }),
                    ..Default::default()
                },
                context_window_size: None,
                auth_hint: None,
                transcript_base_dir: None,
            },
        ];
        let result = build_registry(agents);
        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn test_get_agent_config_resolves_custom_uvx_agent() {
        reset_registry();
        use crate::config::AgentConfigToml;
        use crate::config::AgentDistributionToml;
        use crate::config::PackageDistribution;
        let custom = AgentConfigToml {
            name: "Kimi".to_string(),
            slug: "kimi".to_string(),
            distribution: AgentDistributionToml {
                uvx: Some(PackageDistribution {
                    package: "kimi-cli".to_string(),
                    args: vec!["acp".to_string()],
                }),
                ..Default::default()
            },
            context_window_size: None,
            auth_hint: None,
            transcript_base_dir: None,
        };
        initialize_registry(vec![custom]).unwrap();
        let config = get_agent_config("kimi").expect("should find kimi");
        assert_eq!(config.command, "uvx");
        assert_eq!(config.args, vec!["kimi-cli", "acp"]);
        assert_eq!(config.provider_slug, "kimi");
        reset_registry();
    }

    #[test]
    #[serial]
    fn test_get_agent_config_resolves_custom_local_agent() {
        reset_registry();
        use crate::config::AgentConfigToml;
        use crate::config::AgentDistributionToml;
        use crate::config::LocalDistribution;
        let custom = AgentConfigToml {
            name: "Local Agent".to_string(),
            slug: "local-test".to_string(),
            distribution: AgentDistributionToml {
                local: Some(LocalDistribution {
                    command: "/usr/bin/my-agent".to_string(),
                    args: vec!["--mode".to_string(), "acp".to_string()],
                    env: std::collections::HashMap::from([("KEY".to_string(), "val".to_string())]),
                }),
                ..Default::default()
            },
            context_window_size: None,
            auth_hint: Some("Set KEY env var".to_string()),
            transcript_base_dir: None,
        };
        initialize_registry(vec![custom]).unwrap();
        let config = get_agent_config("local-test").expect("should find local-test");
        assert_eq!(config.command, "/usr/bin/my-agent");
        assert_eq!(config.args, vec!["--mode", "acp"]);
        assert_eq!(config.env.get("KEY").unwrap(), "val");
        assert_eq!(config.auth_hint, "Set KEY env var");
        reset_registry();
    }

    #[test]
    #[serial]
    fn test_list_available_agents_includes_custom() {
        reset_registry();
        use crate::config::AgentConfigToml;
        use crate::config::AgentDistributionToml;
        use crate::config::PackageDistribution;
        let custom = AgentConfigToml {
            name: "Kimi".to_string(),
            slug: "kimi".to_string(),
            distribution: AgentDistributionToml {
                uvx: Some(PackageDistribution {
                    package: "kimi-cli".to_string(),
                    args: vec![],
                }),
                ..Default::default()
            },
            context_window_size: None,
            auth_hint: None,
            transcript_base_dir: None,
        };
        initialize_registry(vec![custom]).unwrap();
        let agents = list_available_agents();
        assert!(
            agents.iter().any(|a| a.display_name.as_str() == "Kimi"),
            "Should contain custom agent Kimi"
        );
        reset_registry();
    }

    #[test]
    #[serial]
    fn test_get_agent_display_name_custom_agent() {
        reset_registry();
        use crate::config::AgentConfigToml;
        use crate::config::AgentDistributionToml;
        use crate::config::PackageDistribution;
        let custom = AgentConfigToml {
            name: "My Custom Agent".to_string(),
            slug: "my-custom".to_string(),
            distribution: AgentDistributionToml {
                npx: Some(PackageDistribution {
                    package: "my-custom-pkg".to_string(),
                    args: vec![],
                }),
                ..Default::default()
            },
            context_window_size: None,
            auth_hint: None,
            transcript_base_dir: None,
        };
        initialize_registry(vec![custom]).unwrap();
        assert_eq!(get_agent_display_name("my-custom"), "My Custom Agent");
        reset_registry();
    }

    #[test]
    fn test_claude_code_context_window_is_1m() {
        assert_eq!(AgentKind::ClaudeCode.context_window_size(), 1_000_000);
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
