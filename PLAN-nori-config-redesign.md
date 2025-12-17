# Nori Config Redesign Implementation Plan

**Goal:** Create a minimal, standalone config system in the ACP module for ACP-only mode, stored in `~/.nori/cli/config.toml` with no mentions of codex/chatgpt/openai.

**Architecture:** A new `config` submodule in `codex-rs/acp/` provides `NoriConfig` - a simplified config struct for ACP-only operations. The TUI opts in via a feature flag `nori-config`. When enabled, the TUI loads `~/.nori/cli/config.toml` instead of `~/.codex/config.toml`. The ACP config reuses types from `codex-protocol` where possible to avoid duplication.

**Tech Stack:** Rust, serde, toml, codex-protocol (for shared types like SandboxMode)

---

## Testing Plan

I will add unit tests that ensure:
1. `NoriConfig` correctly loads from a TOML file at `~/.nori/cli/config.toml`
2. Default values are applied when fields are missing
3. The `find_nori_home()` function correctly identifies `~/.nori/cli` or respects `NORI_HOME` env var
4. CLI overrides work with `NoriConfigOverrides`
5. Invalid TOML produces helpful error messages

I will add an integration test that:
1. Creates a temp directory with a `config.toml`
2. Loads the config using `NoriConfig::load()`
3. Verifies all fields are correctly parsed

NOTE: I will write *all* tests before I add any implementation behavior.

---

## Phase 1: Create Config Module in ACP Crate

### Step 1.1: Add dependencies to acp/Cargo.toml

**File:** `/home/user/nori-cli/codex-rs/acp/Cargo.toml`

Add these dependencies:
- `toml = { workspace = true }` - for TOML parsing
- `dirs = { workspace = true }` - for home directory resolution

### Step 1.2: Create config module file structure

**Create directory:** `/home/user/nori-cli/codex-rs/acp/src/config/`

**Create files:**
- `/home/user/nori-cli/codex-rs/acp/src/config/mod.rs` - Module entry point, exports
- `/home/user/nori-cli/codex-rs/acp/src/config/types.rs` - NoriConfig, NoriConfigToml structs
- `/home/user/nori-cli/codex-rs/acp/src/config/loader.rs` - Config loading logic

### Step 1.3: Write failing test for find_nori_home

**File:** `/home/user/nori-cli/codex-rs/acp/src/config/mod.rs`

Write a test that:
- Sets `NORI_HOME` env var to a temp path
- Calls `find_nori_home()`
- Expects it to return the env var path
- Unsets env var, expects `~/.nori/cli`

### Step 1.4: Run test to verify it fails

```bash
cargo test -p codex-acp find_nori_home
```

Expected: Compilation error (function doesn't exist)

### Step 1.5: Implement find_nori_home

**File:** `/home/user/nori-cli/codex-rs/acp/src/config/loader.rs`

```rust
use anyhow::{Result, Context};
use std::path::PathBuf;

/// Environment variable to override the Nori home directory
pub const NORI_HOME_ENV: &str = "NORI_HOME";

/// Default Nori home directory name
pub const NORI_HOME_DIR: &str = ".nori/cli";

/// Config file name
pub const CONFIG_FILE: &str = "config.toml";

/// Find the Nori home directory (~/.nori/cli or $NORI_HOME)
pub fn find_nori_home() -> Result<PathBuf> {
    if let Ok(env_home) = std::env::var(NORI_HOME_ENV) {
        return Ok(PathBuf::from(env_home));
    }

    let home = dirs::home_dir()
        .context("Could not determine home directory")?;

    Ok(home.join(NORI_HOME_DIR))
}
```

### Step 1.6: Run test to verify it passes

```bash
cargo test -p codex-acp find_nori_home
```

Expected: Test passes

### Step 1.7: Commit

```bash
git add -A && git commit -m "feat(acp): Add find_nori_home for ~/.nori/cli config location"
```

---

## Phase 2: Define NoriConfig Types

### Step 2.1: Write failing test for NoriConfigToml deserialization

**File:** `/home/user/nori-cli/codex-rs/acp/src/config/types.rs`

Test that deserializing an empty TOML `""` produces `NoriConfigToml` with all fields as `None`.

### Step 2.2: Run test to verify it fails

```bash
cargo test -p codex-acp nori_config_toml
```

### Step 2.3: Implement NoriConfigToml

**File:** `/home/user/nori-cli/codex-rs/acp/src/config/types.rs`

```rust
use serde::Deserialize;
use codex_protocol::config_types::{SandboxMode, McpServerConfigToml};
use std::collections::HashMap;

/// TOML-deserializable config structure (all fields optional)
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct NoriConfigToml {
    /// The ACP agent model to use (e.g., "claude-acp", "gemini-acp")
    pub model: Option<String>,

    /// Sandbox mode for command execution
    pub sandbox_mode: Option<SandboxMode>,

    /// Approval policy for commands
    pub approval_policy: Option<ApprovalPolicy>,

    /// TUI settings
    #[serde(default)]
    pub tui: TuiConfigToml,

    /// MCP server configurations (optional)
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfigToml>,
}

/// TUI-specific settings
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TuiConfigToml {
    /// Enable animations (shimmer effects, spinners)
    pub animations: Option<bool>,

    /// Enable desktop notifications
    pub notifications: Option<bool>,
}

/// Approval policy for command execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalPolicy {
    /// Always ask for approval
    Always,
    /// Ask on potentially dangerous operations
    OnRequest,
    /// Never ask (dangerous)
    Never,
}
```

### Step 2.4: Run test to verify it passes

```bash
cargo test -p codex-acp nori_config_toml
```

### Step 2.5: Write failing test for NoriConfig with defaults

Test that `NoriConfig::default()` produces sensible defaults:
- `model` = "claude-acp"
- `animations` = true
- `notifications` = true
- `sandbox_mode` = WorkspaceWrite
- `approval_policy` = OnRequest

### Step 2.6: Run test to verify it fails

### Step 2.7: Implement NoriConfig resolved struct

**File:** `/home/user/nori-cli/codex-rs/acp/src/config/types.rs`

```rust
use std::path::PathBuf;

/// Resolved configuration with defaults applied
#[derive(Debug, Clone)]
pub struct NoriConfig {
    /// The ACP agent model to use
    pub model: String,

    /// Sandbox mode for command execution
    pub sandbox_mode: SandboxMode,

    /// Approval policy for commands
    pub approval_policy: ApprovalPolicy,

    /// Enable TUI animations
    pub animations: bool,

    /// Enable TUI notifications
    pub notifications: bool,

    /// Nori home directory (~/.nori/cli)
    pub nori_home: PathBuf,

    /// Current working directory
    pub cwd: PathBuf,

    /// MCP server configurations
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

impl Default for NoriConfig {
    fn default() -> Self {
        Self {
            model: "claude-acp".to_string(),
            sandbox_mode: SandboxMode::WorkspaceWrite,
            approval_policy: ApprovalPolicy::OnRequest,
            animations: true,
            notifications: true,
            nori_home: PathBuf::from(".nori/cli"),
            cwd: std::env::current_dir().unwrap_or_default(),
            mcp_servers: HashMap::new(),
        }
    }
}
```

### Step 2.8: Run test to verify it passes

```bash
cargo test -p codex-acp nori_config_defaults
```

### Step 2.9: Commit

```bash
git add -A && git commit -m "feat(acp): Add NoriConfig and NoriConfigToml types"
```

---

## Phase 3: Implement Config Loading

### Step 3.1: Write failing test for config file loading

**File:** `/home/user/nori-cli/codex-rs/acp/src/config/loader.rs`

Test that:
1. Creates temp dir with `config.toml` containing `model = "gemini-acp"`
2. Sets `NORI_HOME` to temp dir
3. Calls `NoriConfig::load()`
4. Verifies `config.model == "gemini-acp"`
5. Verifies other fields have defaults

### Step 3.2: Run test to verify it fails

```bash
cargo test -p codex-acp load_config_from_file
```

### Step 3.3: Implement NoriConfig::load()

**File:** `/home/user/nori-cli/codex-rs/acp/src/config/loader.rs`

```rust
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

    /// Build resolved config from TOML + overrides
    fn from_toml(
        toml: NoriConfigToml,
        nori_home: PathBuf,
        overrides: NoriConfigOverrides,
    ) -> Result<Self> {
        let cwd = overrides.cwd
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();

        Ok(Self {
            model: overrides.model
                .or(toml.model)
                .unwrap_or_else(|| "claude-acp".to_string()),
            sandbox_mode: overrides.sandbox_mode
                .or(toml.sandbox_mode)
                .unwrap_or(SandboxMode::WorkspaceWrite),
            approval_policy: overrides.approval_policy
                .or(toml.approval_policy)
                .unwrap_or(ApprovalPolicy::OnRequest),
            animations: toml.tui.animations.unwrap_or(true),
            notifications: toml.tui.notifications.unwrap_or(true),
            nori_home,
            cwd,
            mcp_servers: resolve_mcp_servers(toml.mcp_servers)?,
        })
    }
}
```

### Step 3.4: Run test to verify it passes

```bash
cargo test -p codex-acp load_config_from_file
```

### Step 3.5: Commit

```bash
git add -A && git commit -m "feat(acp): Implement NoriConfig::load() from ~/.nori/cli/config.toml"
```

---

## Phase 4: Add CLI Overrides

### Step 4.1: Write failing test for CLI overrides

Test that overrides take precedence over TOML values:
1. Create config with `model = "gemini-acp"`
2. Call `load_with_overrides(NoriConfigOverrides { model: Some("claude-acp") })`
3. Verify `config.model == "claude-acp"`

### Step 4.2: Run test to verify it fails

### Step 4.3: Implement NoriConfigOverrides

**File:** `/home/user/nori-cli/codex-rs/acp/src/config/types.rs`

```rust
/// CLI overrides for config values
#[derive(Debug, Clone, Default)]
pub struct NoriConfigOverrides {
    /// Override the model selection
    pub model: Option<String>,

    /// Override sandbox mode
    pub sandbox_mode: Option<SandboxMode>,

    /// Override approval policy
    pub approval_policy: Option<ApprovalPolicy>,

    /// Override current working directory
    pub cwd: Option<PathBuf>,
}
```

### Step 4.4: Run test to verify it passes

### Step 4.5: Commit

```bash
git add -A && git commit -m "feat(acp): Add NoriConfigOverrides for CLI overrides"
```

---

## Phase 5: Export from ACP Module

### Step 5.1: Update acp/src/lib.rs exports

**File:** `/home/user/nori-cli/codex-rs/acp/src/lib.rs`

Add:
```rust
pub mod config;

pub use config::NoriConfig;
pub use config::NoriConfigOverrides;
pub use config::find_nori_home;
pub use config::ApprovalPolicy;
```

### Step 5.2: Verify compilation

```bash
cargo build -p codex-acp
```

### Step 5.3: Commit

```bash
git add -A && git commit -m "feat(acp): Export config module from codex-acp"
```

---

## Phase 6: Add TUI Feature Flag

### Step 6.1: Add nori-config feature to TUI Cargo.toml

**File:** `/home/user/nori-cli/codex-rs/tui/Cargo.toml`

Add feature:
```toml
[features]
# ... existing features ...

# Use Nori's simplified ACP-only config instead of upstream codex-core config
nori-config = []
```

### Step 6.2: Create config adapter module in TUI

**File:** `/home/user/nori-cli/codex-rs/tui/src/nori/config_adapter.rs`

This module bridges `NoriConfig` to the interfaces TUI expects:

```rust
//! Adapter to use NoriConfig where codex_core::Config is expected

use codex_acp::{NoriConfig, NoriConfigOverrides};
use codex_protocol::config_types::SandboxMode;
use std::path::PathBuf;

/// Load config using Nori's ACP config system
pub fn load_nori_config(overrides: NoriConfigOverrides) -> anyhow::Result<NoriConfig> {
    NoriConfig::load_with_overrides(overrides)
}

/// Convert CLI args to NoriConfigOverrides
pub fn cli_to_overrides(cli: &crate::Cli) -> NoriConfigOverrides {
    NoriConfigOverrides {
        model: cli.model.clone(),
        sandbox_mode: cli.sandbox_mode.map(Into::into),
        approval_policy: cli.approval_policy.map(Into::into),
        cwd: cli.cwd.clone(),
    }
}
```

### Step 6.3: Update TUI lib.rs with conditional compilation

**File:** `/home/user/nori-cli/codex-rs/tui/src/lib.rs`

Add conditional imports at top:
```rust
#[cfg(feature = "nori-config")]
mod nori_config_adapter;

#[cfg(feature = "nori-config")]
use nori_config_adapter::{load_nori_config, cli_to_overrides};
```

### Step 6.4: Create alternate run_main for nori-config

**File:** `/home/user/nori-cli/codex-rs/tui/src/nori/config_adapter.rs`

Add a simplified `run_main_nori()` function that:
1. Parses CLI args
2. Converts to `NoriConfigOverrides`
3. Loads `NoriConfig`
4. Initializes TUI with Nori-specific settings
5. Uses ACP backend exclusively

### Step 6.5: Commit

```bash
git add -A && git commit -m "feat(tui): Add nori-config feature flag for ACP-only config"
```

---

## Phase 7: Wire Up Main Binary

### Step 7.1: Update main.rs to use feature flag

**File:** `/home/user/nori-cli/codex-rs/tui/src/main.rs`

Add conditional entry point:
```rust
#[cfg(feature = "nori-config")]
fn main() {
    // Use Nori config system
    let cli = codex_tui::Cli::parse();
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(codex_tui::run_main_nori(cli));
    // ...
}

#[cfg(not(feature = "nori-config"))]
fn main() {
    // Existing codex-core config system
    // ... existing code ...
}
```

### Step 7.2: Test build with nori-config feature

```bash
cargo build -p codex-tui --features nori-config
```

### Step 7.3: Test build without nori-config feature

```bash
cargo build -p codex-tui
```

### Step 7.4: Commit

```bash
git add -A && git commit -m "feat(tui): Wire up nori-config feature to main binary"
```

---

## Phase 8: Enable nori-config by Default

### Step 8.1: Update default features in TUI Cargo.toml

**File:** `/home/user/nori-cli/codex-rs/tui/Cargo.toml`

Change:
```toml
[features]
default = ["unstable", "nori-config"]
```

### Step 8.2: Verify default build uses Nori config

```bash
cargo build -p codex-tui
./target/debug/codex-tui --help
```

Verify help text shows Nori-specific options.

### Step 8.3: Commit

```bash
git add -A && git commit -m "feat(tui): Enable nori-config by default"
```

---

## Phase 9: Remove Codex/OpenAI/ChatGPT References

### Step 9.1: Audit config module for forbidden terms

Search for and replace:
- "codex" → "nori" (in comments, strings, identifiers)
- "chatgpt" → remove/replace
- "openai" → remove/replace

**Files to check:**
- `/home/user/nori-cli/codex-rs/acp/src/config/mod.rs`
- `/home/user/nori-cli/codex-rs/acp/src/config/types.rs`
- `/home/user/nori-cli/codex-rs/acp/src/config/loader.rs`

### Step 9.2: Update error messages and comments

Replace any help text, error messages, or comments mentioning forbidden terms.

### Step 9.3: Run tests

```bash
cargo test -p codex-acp
```

### Step 9.4: Commit

```bash
git add -A && git commit -m "chore(acp): Remove codex/openai/chatgpt references from config"
```

---

## Edge Cases

1. **Missing config file**: When `~/.nori/cli/config.toml` doesn't exist, use all defaults. Do not error.

2. **Invalid TOML syntax**: Provide clear error message with file path and parse error details.

3. **Unknown fields in TOML**: Ignore unknown fields (use `#[serde(deny_unknown_fields)]` only if strict mode is desired).

4. **Invalid model name**: The model validation happens in `get_agent_config()` in registry.rs, not in config loading. Config loading should accept any string.

5. **Permission denied reading config**: Provide helpful error message suggesting checking file permissions.

6. **NORI_HOME points to non-existent directory**: Create it on first write, but don't fail on read if it doesn't exist.

7. **Relative path in cwd override**: Canonicalize relative paths to absolute paths.

8. **MCP server config with missing required fields**: Provide clear error message identifying which server and which field is missing.

---

## Questions

1. **Should the nori-config feature completely replace codex-core config, or should it be a thin wrapper?**
   - Recommendation: Complete replacement for simplicity. The ACP module should be self-contained.

2. **Should MCP server config be included in the minimal config?**
   - Recommendation: Yes, but as optional. MCP servers are useful for tools but not required for basic operation.

3. **Should there be a migration path from ~/.codex/config.toml to ~/.nori/cli/config.toml?**
   - Recommendation: No automatic migration. Users should manually create a new config. This is cleaner.

4. **Should the history file also move to ~/.nori/cli/?**
   - Recommendation: Yes, `~/.nori/cli/history.jsonl` for consistency.

5. **What about log files?**
   - Recommendation: `~/.nori/cli/log/` for consistency.

---

**Testing Details:**
- Unit tests verify TOML deserialization, default value application, and env var precedence
- Integration tests verify end-to-end config loading from temp files
- Tests specifically verify *behavior* (config resolves correctly) not just types exist

**Implementation Details:**
- NoriConfig is ~50 lines vs ~900 lines for codex-core Config
- Uses existing SandboxMode from codex-protocol (no duplication)
- Feature flag allows gradual rollout and easy revert
- No changes to codex-core (minimizes upstream conflicts)
- Config loading is synchronous (no async needed for simple TOML parsing)
- Clear separation: ACP module owns config, TUI consumes it

---
