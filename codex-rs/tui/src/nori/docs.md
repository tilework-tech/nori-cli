# Noridoc: nori

Path: @/codex-rs/tui/src/nori

### Overview

The `nori` module contains Nori-specific TUI customizations that replace or extend the default Codex UI behavior. It provides branded session headers, agent picking, feedback redirection, and a Nori-specific update checking mechanism that queries GitHub releases instead of OpenAI's update system.

### How it fits into the larger codebase

- **Called by** `history_cell.rs` via `new_session_info()` which delegates to `new_nori_session_info()`
- **Replaces** the original `SessionHeaderHistoryCell` (preserved as dead code for potential future feature flag selection)
- **Uses** `HistoryCell` trait from `@/codex-rs/tui/src/history_cell.rs` for consistent rendering
- **Reads** `~/.nori-config.json` for Nori profile information
- **Conditionally compiled** based on feature flags - modules like `feedback.rs`, `updates.rs` are only included when their corresponding upstream features are disabled
- **Re-exported** by `@/codex-rs/tui/src/lib.rs` to provide unified access to update types regardless of which update system is active

### Core Implementation

**Session Header (`session_header.rs`):**

The `NoriSessionHeaderCell` struct implements `HistoryCell` and renders:

```
╭──────────────────────────────────────╮
│   _   _  ___  ____  ___              │
│  | \ | \/ _ \|  _ \|_ _\             │
│  |  \| | | | | |_) || |              │
│  | |\  | |_| |  _ < | |              │
│  \_| \_|\___/\_| \_\___|             │
│                                      │
│ version:   v0.x.x                    │
│ directory: ~/path/to/project         │
│ agent:     claude-sonnet             │
│ profile:   senior-swe                │
╰──────────────────────────────────────╯

  Powered by Nori AI

  Run 'npx nori-ai install' to set up Nori AI enhancements
```

**Key functions:**

- `new_nori_session_info()`: Entry point called by `history_cell::new_session_info()`. Creates the composite cell with header + help text
- `read_nori_profile()`: Parses `~/.nori-config.json` to extract `profile.baseProfile`
- `format_directory()`: Relativizes paths to home directory with truncation for narrow terminals
- `new_nori_status_output()`: Creates the composite cell for `/status` command output. Displays the command echo followed by the `NoriSessionHeaderCell` showing Nori branding, version, directory, agent, and profile info. Called by `chatwidget.rs::add_status_output()`

**ASCII Banner Styling:**

The banner uses green+bold for alphabetic characters and dark gray for structural characters (pipes, slashes) to create a two-tone visual effect.

**Agent Picker (`agent_picker.rs`):**

- `agent_picker_params()` consumes `codex_acp::list_available_agents()` so `/agent` can display each `AcpAgentInfo` entry (model name, display name, description, provider slug) with a `SelectionAction` that sends `AppEvent::SetPendingAgent`.
- `acp_model_picker_params()` renders a fallback when the `unstable` feature is disabled, showing a disabled picker that directs users to use `/agent`.
- `acp_model_picker_params_with_models()` [unstable] creates a model picker with actual models fetched from the ACP agent. Displays each model as a `SelectionItem` with a `SelectionAction` that sends `AppEvent::SetAcpModel`.
- `PendingAgentSelection` holds the selected model/display name pair so the App and `ChatWidget` can store it until the next prompt triggers `AppEvent::SubmitWithAgentSwitch`, at which point the agent is persisted to config and the conversation is rebuilt with the new model.

**Feedback Redirect (`feedback.rs`):**

Compiled only when `feedback` feature is disabled (`#[cfg(not(feature = "feedback"))]`). Redirects `/feedback` command to GitHub Discussions instead of OpenAI's feedback system:
- `NORI_FEEDBACK_URL`: Points to `https://github.com/tilework-tech/nori-cli/discussions`
- `feedback_message()`: Returns user-facing message with the discussions URL

**Update System (`update_action.rs`, `updates.rs`, `update_prompt.rs`):**

Compiled only when `upstream-updates` feature is disabled. Provides Nori-specific update checking:

`update_action.rs`:
- `UpdateAction` enum with `NpmGlobalLatest` and `Manual` variants
- `command_args()` returns the shell command to execute the update
- `get_update_action()` (release builds only) checks `NORI_MANAGED_BY_NPM` env var to determine update method

`updates.rs` (release builds only):
- Queries `https://api.github.com/repos/tilework-tech/nori-cli/releases/latest` for version info
- Caches version data in `~/.codex/nori-version.json` with 20-hour refresh interval
- `get_upgrade_version()`: Background-refreshes cache and returns newer version if available
- `get_upgrade_version_for_popup()`: Returns version only if not previously dismissed
- `dismiss_version()`: Persists user's dismissal to avoid repeated prompts
- Tag format: expects `nori-v<semver>` (e.g., `nori-v1.2.3`)

`update_prompt.rs` (release builds only):
- `run_update_prompt_if_needed()`: Displays update prompt UI when new version available
- Returns `UpdatePromptOutcome::Continue` or `UpdatePromptOutcome::RunUpdate(action)`

### Things to Know

**Profile Display:**

- When `~/.nori-config.json` contains a `profile.baseProfile`, that value is displayed
- When the file is missing or has no profile, displays "(none)"
- Config parsing is permissive - missing fields or invalid JSON result in `None` profile

**Integration Point:**

The original Codex session header (`SessionHeaderHistoryCell`) is preserved with `#[allow(dead_code)]` annotations. The `new_session_info()` function in `history_cell.rs` unconditionally calls the Nori version. Future work could add a feature flag or config option to toggle between them.

**Width Handling:**

The session header uses a max inner width of 60 characters. Directory paths are center-truncated when they exceed available space (e.g., `~/a/b/…/y/z`).

**Conditional Compilation:**

Module availability in `mod.rs` follows this pattern:

```
session_header.rs, agent_picker.rs  -> Always included
feedback.rs                         -> #[cfg(not(feature = "feedback"))]
update_action.rs                    -> #[cfg(not(feature = "upstream-updates"))]
update_prompt.rs, updates.rs        -> #[cfg(all(not(feature = "upstream-updates"), not(debug_assertions)))]
```

The `lib.rs` re-export logic ensures `UpdateAction` type is always available via `codex_tui::update_action::UpdateAction` regardless of which update system is compiled.

**Config Adapter (`config_adapter.rs`):**

Provides integration between the Nori config system (from `@/codex-rs/acp/src/config/`) and the TUI:
- `get_nori_home()`: Returns the canonicalized Nori home path (`~/.nori/cli`). Canonicalization handles systems with symlinks (e.g., macOS `/var` -> `/private/var`) to ensure consistency between where config is saved and loaded.
- `setup_nori_config_environment()`: Sets `CODEX_HOME` env var (canonicalized) to redirect codex-core's config loading to the Nori location
- `load_nori_config()`: Loads `NoriConfig` directly from `~/.nori/cli/config.toml`

Path canonicalization is important because `find_codex_home()` returns a canonicalized path. If NORI_HOME is not canonicalized, trust settings saved under one path may not be found when config is loaded under the canonical path.

**Onboarding Module (`onboarding/`):**

Provides Nori-branded first-launch onboarding flow:
- `first_launch.rs`: First-launch detection via `~/.nori/cli/config.toml` existence
  - `is_first_launch(nori_home)`: Returns true if `config.toml` doesn't exist
  - `mark_first_launch_complete(nori_home)`: Sets `cli.first_launch_complete = true` using `ConfigEditsBuilder`
  - Note: `nori_home` parameter expects the full path (`~/.nori/cli`), not `~/.nori`
- `welcome.rs`: ASCII banner welcome screen with Nori branding
- `trust_directory.rs`: Directory trust prompt (persists to codex-core's trust system)
- `onboarding_screen.rs`: Orchestrates the multi-step onboarding flow

**Config Persistence Pattern:**

The onboarding flow writes config changes via `ConfigEditsBuilder` (from `@/codex-rs/core/src/config/edit.rs`) which merges edits with existing content. This is critical because:
1. `trust_directory.rs` calls `set_project_trust_level()` to save trust settings
2. `first_launch.rs` calls `mark_first_launch_complete()` to set `cli.first_launch_complete`
3. Both must merge rather than overwrite to preserve each other's changes

The onboarding screen uses `get_nori_home()` from `config_adapter` to get the canonical path, ensuring consistency with the ACP config module.

Created and maintained by Nori.
