# Nori CLI Feature Flags Implementation Plan

**Goal:** Feature-gate all OpenAI/Codex-specific functionality to create a clean Nori CLI fork that defaults to ACP-only mode.

**Architecture:** Add Cargo feature flags to TUI crate for `login`, `feedback`, `backend-client`, and `upstream-updates`. Create Nori-specific update mechanism in `tui/src/nori/`. Hide or replace slash command behavior with Nori-specific implementations when features are disabled.

**Tech Stack:** Rust, Cargo features, `#[cfg(feature = "...")]` conditional compilation

---

## Key Configuration

| Item | Value |
|------|-------|
| NPM package name | `nori-ai-cli` |
| Executable name | `nori` |
| GitHub release tag format | `nori-v0.x.x` |
| GitHub repo | `tilework-tech/nori-cli` |
| Feedback URL | `https://github.com/tilework-tech/nori-cli/discussions` |
| Update check interval | 20 hours (same as upstream) |

---

## Testing Plan

I will add the following tests:

1. **Integration test**: Verify that `cargo build -p codex-tui` compiles successfully with no features (minimal Nori build)
2. **Integration test**: Verify that `cargo build -p codex-tui --features full` compiles with all features
3. **Unit test**: Verify `/feedback` shows "Report issue on GitHub Discussions" message when `feedback` feature is disabled
4. **Unit test**: Verify `/logout` is hidden from slash command list when `login` feature is disabled
5. **Unit test**: Verify Nori update prompt renders correctly with Nori branding and URLs
6. **Snapshot test**: Add Nori update_prompt snapshot with correct branding

NOTE: I will write *all* tests before I add any implementation behavior.

---

## Phase 1: Add Feature Flags to TUI Crate

### Step 1.1: Write failing test for minimal TUI build

**File:** Create test script or CI check

```bash
# Expected to fail initially because dependencies are not optional
cargo build -p codex-tui --no-default-features 2>&1 | grep -q "error"
```

### Step 1.2: Update TUI Cargo.toml with feature flags

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/Cargo.toml`

Add these features:

```toml
[features]
default = []
vt100-tests = []
debug-logs = []

# NEW: Upstream functionality flags
full = ["login", "feedback", "backend-client", "upstream-updates"]

# ChatGPT/API login functionality
login = ["dep:codex-login"]

# Feedback to Sentry
feedback = ["dep:codex-feedback"]

# Backend client for cloud tasks
backend-client = ["dep:codex-backend-client"]

# Upstream (OpenAI) update checking
upstream-updates = []
```

Change dependencies to optional:

```toml
codex-backend-client = { workspace = true, optional = true }
codex-feedback = { workspace = true, optional = true }
codex-login = { workspace = true, optional = true }
```

### Step 1.3: Run test to verify it now compiles

```bash
cargo build -p codex-tui --no-default-features
```

---

## Phase 2: Feature-Gate Login Functionality

### Step 2.1: Write failing test for hidden login slash commands

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/slash_command.rs`

Add test that verifies `/logout` is NOT in `built_in_slash_commands()` when login feature is disabled.

### Step 2.2: Gate login imports in TUI

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/lib.rs`

Wrap `codex_login` imports with `#[cfg(feature = "login")]`

### Step 2.3: Hide `/logout` from slash command list

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/slash_command.rs`

Update `is_visible()` method:

```rust
fn is_visible(self) -> bool {
    match self {
        SlashCommand::Rollout | SlashCommand::TestApproval => cfg!(debug_assertions),
        #[cfg(not(feature = "login"))]
        SlashCommand::Logout => false,
        _ => true,
    }
}
```

### Step 2.4: Gate `/logout` slash command handler

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/chatwidget.rs` (around line 1550)

```rust
SlashCommand::Logout => {
    #[cfg(feature = "login")]
    {
        if let Err(e) = codex_core::auth::logout(
            &self.config.codex_home,
            self.config.cli_auth_credentials_store_mode,
        ) {
            tracing::error!("failed to logout: {e}");
        }
        self.request_exit();
    }
    #[cfg(not(feature = "login"))]
    {
        // Should not reach here since command is hidden, but handle gracefully
        self.add_to_history(history_cell::new_error_event(
            "Login functionality not available in this build."
        ));
    }
}
```

### Step 2.5: Gate onboarding auth screen

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/onboarding/auth.rs`

Gate the entire file with `#[cfg(feature = "login")]` and provide a stub when disabled.

### Step 2.6: Run tests to verify login gating works

```bash
cargo test -p codex-tui --no-default-features
cargo test -p codex-tui --features login
```

---

## Phase 3: Feature-Gate Feedback Functionality

### Step 3.1: Write failing test for hidden feedback slash command

Test that `/feedback` is NOT in `built_in_slash_commands()` when feature disabled, but a Nori-specific feedback mechanism exists.

### Step 3.2: Gate feedback imports

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/bottom_pane/mod.rs`

Gate `feedback_view` module and `feedback_selection_params` with `#[cfg(feature = "feedback")]`

### Step 3.3: Create Nori feedback redirect module

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/nori/feedback.rs`

```rust
//! Nori-specific feedback handling - redirects to GitHub Discussions

pub const NORI_FEEDBACK_URL: &str = "https://github.com/tilework-tech/nori-cli/discussions";

pub fn feedback_message() -> &'static str {
    "To report issues or provide feedback, please visit:\n\
     https://github.com/tilework-tech/nori-cli/discussions"
}
```

### Step 3.4: Hide `/feedback` and replace with Nori behavior

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/slash_command.rs`

Update `is_visible()`:

```rust
fn is_visible(self) -> bool {
    match self {
        SlashCommand::Rollout | SlashCommand::TestApproval => cfg!(debug_assertions),
        #[cfg(not(feature = "login"))]
        SlashCommand::Logout => false,
        #[cfg(not(feature = "feedback"))]
        SlashCommand::Feedback => false,
        _ => true,
    }
}
```

### Step 3.5: Gate `/feedback` slash command handler

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/chatwidget.rs` (around line 1509)

```rust
SlashCommand::Feedback => {
    #[cfg(feature = "feedback")]
    {
        // Existing Sentry feedback flow
        let params = crate::bottom_pane::feedback_selection_params(self.app_event_tx.clone());
        self.bottom_pane.show_selection_view(params);
        self.request_redraw();
    }
    #[cfg(not(feature = "feedback"))]
    {
        // Should not reach here since command is hidden, but handle gracefully
        use crate::nori::feedback;
        self.add_to_history(history_cell::new_system_event(
            feedback::feedback_message()
        ));
        self.request_redraw();
    }
}
```

### Step 3.6: Update nori/mod.rs to include feedback module

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/nori/mod.rs`

```rust
#[cfg(not(feature = "feedback"))]
pub(crate) mod feedback;
```

### Step 3.7: Run tests

```bash
cargo test -p codex-tui --no-default-features
```

---

## Phase 4: Create Nori Update Mechanism

### Step 4.1: Write failing snapshot test for Nori update prompt

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/nori/updates.rs`

Test that renders the Nori update prompt with correct branding.

### Step 4.2: Create Nori update_action module

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/nori/update_action.rs`

```rust
//! Nori-specific update actions

/// Update action for Nori CLI
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    /// Update via `npm install -g nori-ai-cli@latest`
    NpmGlobalLatest,
    /// Manual update (show instructions)
    Manual,
}

impl UpdateAction {
    /// Returns the list of command-line arguments for invoking the update.
    pub fn command_args(self) -> (&'static str, &'static [&'static str]) {
        match self {
            UpdateAction::NpmGlobalLatest => ("npm", &["install", "-g", "nori-ai-cli@latest"]),
            UpdateAction::Manual => ("echo", &["Please visit https://github.com/tilework-tech/nori-cli/releases"]),
        }
    }

    /// Returns string representation of the command-line arguments for invoking the update.
    pub fn command_str(self) -> String {
        let (command, args) = self.command_args();
        shlex::try_join(std::iter::once(command).chain(args.iter().copied()))
            .unwrap_or_else(|_| format!("{command} {}", args.join(" ")))
    }
}

#[cfg(not(debug_assertions))]
pub(crate) fn get_update_action() -> Option<UpdateAction> {
    let managed_by_npm = std::env::var_os("NORI_MANAGED_BY_NPM").is_some();

    if managed_by_npm {
        Some(UpdateAction::NpmGlobalLatest)
    } else {
        // For other installations, show manual update option
        Some(UpdateAction::Manual)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn npm_update_command_is_correct() {
        let action = UpdateAction::NpmGlobalLatest;
        let (cmd, args) = action.command_args();
        assert_eq!(cmd, "npm");
        assert_eq!(args, &["install", "-g", "nori-ai-cli@latest"]);
    }
}
```

### Step 4.3: Create Nori updates module

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/nori/updates.rs`

```rust
//! Nori-specific update checking
//!
//! Checks for updates from the tilework-tech/nori-cli GitHub releases.

#![cfg(not(debug_assertions))]

use crate::nori::update_action;
use crate::nori::update_action::UpdateAction;
use chrono::DateTime;
use chrono::Duration;
use chrono::Utc;
use codex_core::config::Config;
use codex_core::default_client::create_client;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

use crate::version::CODEX_CLI_VERSION;

const VERSION_FILENAME: &str = "nori-version.json";
const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/tilework-tech/nori-cli/releases/latest";

#[derive(Serialize, Deserialize, Debug, Clone)]
struct VersionInfo {
    latest_version: String,
    last_checked_at: DateTime<Utc>,
    #[serde(default)]
    dismissed_version: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct ReleaseInfo {
    tag_name: String,
}

pub fn get_upgrade_version(config: &Config) -> Option<String> {
    if !config.check_for_update_on_startup {
        return None;
    }

    let version_file = version_filepath(config);
    let info = read_version_info(&version_file).ok();

    if match &info {
        None => true,
        Some(info) => info.last_checked_at < Utc::now() - Duration::hours(20),
    } {
        // Refresh the cached latest version in the background
        tokio::spawn(async move {
            check_for_update(&version_file)
                .await
                .inspect_err(|e| tracing::error!("Failed to check for Nori update: {e}"))
        });
    }

    info.and_then(|info| {
        if is_newer(&info.latest_version, CODEX_CLI_VERSION).unwrap_or(false) {
            Some(info.latest_version)
        } else {
            None
        }
    })
}

fn version_filepath(config: &Config) -> PathBuf {
    config.codex_home.join(VERSION_FILENAME)
}

fn read_version_info(version_file: &Path) -> anyhow::Result<VersionInfo> {
    let contents = std::fs::read_to_string(version_file)?;
    Ok(serde_json::from_str(&contents)?)
}

async fn check_for_update(version_file: &Path) -> anyhow::Result<()> {
    let ReleaseInfo { tag_name } = create_client()
        .get(LATEST_RELEASE_URL)
        .send()
        .await?
        .error_for_status()?
        .json::<ReleaseInfo>()
        .await?;

    let latest_version = extract_version_from_tag(&tag_name)?;

    let prev_info = read_version_info(version_file).ok();
    let info = VersionInfo {
        latest_version,
        last_checked_at: Utc::now(),
        dismissed_version: prev_info.and_then(|p| p.dismissed_version),
    };

    let json_line = format!("{}\n", serde_json::to_string(&info)?);
    if let Some(parent) = version_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(version_file, json_line).await?;
    Ok(())
}

fn extract_version_from_tag(tag_name: &str) -> anyhow::Result<String> {
    tag_name
        .strip_prefix("nori-v")
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("Failed to parse Nori tag name '{tag_name}'"))
}

fn is_newer(latest: &str, current: &str) -> Option<bool> {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => Some(l > c),
        _ => None,
    }
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut iter = v.trim().split('.');
    let maj = iter.next()?.parse::<u64>().ok()?;
    let min = iter.next()?.parse::<u64>().ok()?;
    let pat = iter.next()?.parse::<u64>().ok()?;
    Some((maj, min, pat))
}

pub fn get_upgrade_version_for_popup(config: &Config) -> Option<String> {
    if !config.check_for_update_on_startup {
        return None;
    }

    let version_file = version_filepath(config);
    let latest = get_upgrade_version(config)?;

    if let Ok(info) = read_version_info(&version_file)
        && info.dismissed_version.as_deref() == Some(latest.as_str())
    {
        return None;
    }
    Some(latest)
}

pub async fn dismiss_version(config: &Config, version: &str) -> anyhow::Result<()> {
    let version_file = version_filepath(config);
    let mut info = match read_version_info(&version_file) {
        Ok(info) => info,
        Err(_) => return Ok(()),
    };
    info.dismissed_version = Some(version.to_string());
    let json_line = format!("{}\n", serde_json::to_string(&info)?);
    if let Some(parent) = version_file.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(version_file, json_line).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_version_from_nori_tag() {
        assert_eq!(
            extract_version_from_tag("nori-v1.2.3").expect("failed to parse"),
            "1.2.3"
        );
    }

    #[test]
    fn rejects_non_nori_tags() {
        assert!(extract_version_from_tag("rust-v1.2.3").is_err());
        assert!(extract_version_from_tag("v1.2.3").is_err());
    }

    #[test]
    fn version_comparison_works() {
        assert_eq!(is_newer("1.0.1", "1.0.0"), Some(true));
        assert_eq!(is_newer("1.0.0", "1.0.1"), Some(false));
        assert_eq!(is_newer("2.0.0", "1.9.9"), Some(true));
    }
}
```

### Step 4.4: Create Nori update_prompt module

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/nori/update_prompt.rs`

Copy from `tui/src/update_prompt.rs` and modify:
- Import from `crate::nori::update_action` and `crate::nori::updates`
- Change release notes URL to `https://github.com/tilework-tech/nori-cli/releases/latest`
- Update snapshot test name to `nori_update_prompt_modal`

### Step 4.5: Gate upstream update modules

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/lib.rs`

```rust
#[cfg(all(not(debug_assertions), feature = "upstream-updates"))]
mod update_action;
#[cfg(all(not(debug_assertions), feature = "upstream-updates"))]
mod update_prompt;
#[cfg(all(not(debug_assertions), feature = "upstream-updates"))]
mod updates;

// Re-export the appropriate module based on feature
#[cfg(all(not(debug_assertions), feature = "upstream-updates"))]
pub(crate) use update_action::UpdateAction;
#[cfg(all(not(debug_assertions), feature = "upstream-updates"))]
pub(crate) use update_prompt::{run_update_prompt_if_needed, UpdatePromptOutcome};

#[cfg(all(not(debug_assertions), not(feature = "upstream-updates")))]
pub(crate) use nori::update_action::UpdateAction;
#[cfg(all(not(debug_assertions), not(feature = "upstream-updates")))]
pub(crate) use nori::update_prompt::{run_update_prompt_if_needed, UpdatePromptOutcome};
```

### Step 4.6: Update nori/mod.rs

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/nori/mod.rs`

```rust
//! Nori-specific customizations for the TUI.
//!
//! This module contains Nori-branded components that replace or extend
//! the default Codex TUI behavior.

pub(crate) mod agent_picker;
pub(crate) mod session_header;

#[cfg(not(feature = "feedback"))]
pub(crate) mod feedback;

#[cfg(all(not(feature = "upstream-updates"), not(debug_assertions)))]
pub(crate) mod update_action;
#[cfg(all(not(feature = "upstream-updates"), not(debug_assertions)))]
pub(crate) mod update_prompt;
#[cfg(all(not(feature = "upstream-updates"), not(debug_assertions)))]
pub(crate) mod updates;
```

### Step 4.7: Run snapshot test and update

```bash
cargo test -p codex-tui --no-default-features -- nori_update_prompt
cargo insta review
```

---

## Phase 5: Gate Backend Client

### Step 5.1: Find all backend-client usages

Search for `codex_backend_client` imports in TUI crate and wrap with `#[cfg(feature = "backend-client")]`

### Step 5.2: Gate backend-client imports

**Files to modify:**
- `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/tui/src/lib.rs`
- Any other files importing `codex_backend_client`

### Step 5.3: Verify build without backend-client

```bash
cargo build -p codex-tui --no-default-features
```

---

## Phase 6: Update CLI Crate Feature Propagation

### Step 6.1: Update CLI Cargo.toml to propagate TUI features

**File:** `/home/clifford/Documents/source/nori/cli/.worktrees/feature-release-feature-flags/codex-rs/cli/Cargo.toml`

Update the `full` feature to include TUI features:

```toml
[features]
default = []

full = [
    "app-server",
    "cloud-tasks",
    "login",
    "mcp-server",
    "chatgpt",
    "responses-api-proxy",
    "codex-tui/full",
]

# Existing features with TUI propagation
login = ["dep:codex-login", "codex-tui/login"]
```

### Step 6.2: Verify full build

```bash
cargo build -p codex-cli --features full
cargo build -p codex-cli  # minimal build
```

---

## Phase 7: Final Integration Testing

### Step 7.1: Run full test suite with no features

```bash
cargo test -p codex-tui --no-default-features
cargo test -p codex-cli --no-default-features
```

### Step 7.2: Run full test suite with all features

```bash
cargo test -p codex-tui --features full
cargo test -p codex-cli --features full
```

### Step 7.3: Manual smoke test

```bash
# Build minimal Nori binary
cargo build -p codex-cli --release

# Run and verify:
# - No ChatGPT login prompt appears
# - /logout is NOT visible in slash command list
# - /feedback is NOT visible in slash command list
# - Update prompt (if shown) uses Nori URLs and branding
./target/release/nori
```

---

## Summary of Changes

### New Files

| File | Purpose |
|------|---------|
| `tui/src/nori/feedback.rs` | GitHub Discussions redirect for feedback |
| `tui/src/nori/update_action.rs` | Nori-specific update actions (npm) |
| `tui/src/nori/update_prompt.rs` | Nori-branded update prompt UI |
| `tui/src/nori/updates.rs` | Nori GitHub release version checking |

### Modified Files

| File | Changes |
|------|---------|
| `tui/Cargo.toml` | Add feature flags, make deps optional |
| `tui/src/nori/mod.rs` | Export new modules conditionally |
| `tui/src/slash_command.rs` | Hide `/logout` and `/feedback` when features disabled |
| `tui/src/chatwidget.rs` | Gate slash command handlers |
| `tui/src/lib.rs` | Gate imports and re-exports |
| `tui/src/bottom_pane/mod.rs` | Gate feedback_view module |
| `cli/Cargo.toml` | Propagate TUI features |

### Feature Flag Matrix

| Feature | Default | Enables |
|---------|---------|---------|
| `login` | OFF | ChatGPT/API login, `/logout` command |
| `feedback` | OFF | Sentry feedback, `/feedback` command |
| `backend-client` | OFF | Cloud tasks backend client |
| `upstream-updates` | OFF | OpenAI GitHub release checking |
| `full` | OFF | All of the above |

---

## Testing Details

- **Minimal build test**: Verifies `cargo build` succeeds with no features enabled
- **Full build test**: Verifies `cargo build --features full` succeeds
- **Slash command visibility tests**: Unit tests verify `/logout` and `/feedback` are hidden from command list when features disabled
- **Update prompt snapshot**: Verifies Nori branding with `tilework-tech/nori-cli` URLs
- **Version parsing test**: Verifies `nori-v0.x.x` tag format is parsed correctly
- All tests verify BEHAVIOR (user-visible output) not just implementation details

## Implementation Details

- TUI crate gains 4 new features: `login`, `feedback`, `backend-client`, `upstream-updates`
- Dependencies `codex-login`, `codex-feedback`, `codex-backend-client` become optional
- Slash commands `/logout` and `/feedback` are hidden (not just erroring) when features disabled
- New `nori/` modules provide Nori-branded update mechanism checking `tilework-tech/nori-cli` releases
- Update command uses `npm install -g nori-ai-cli@latest`
- Version tags parsed as `nori-v0.x.x` format
- CLI `full` feature propagates to TUI `full` feature
- Minimal build excludes all OpenAI-specific functionality

## Questions (Resolved)

1. **NPM package name**: `nori-ai-cli`
2. **Executable name**: `nori`
3. **GitHub release tag format**: `nori-v0.x.x`
4. **Slash command behavior when disabled**: Hidden entirely from list
5. **Update check interval**: 20 hours (same as upstream)
