# Noridoc: nori

Path: @/codex-rs/tui/src/nori

### Overview

The `nori` module contains Nori-specific TUI customizations that replace or extend the default Codex UI behavior. It provides the branded session header, agent picker UI, and (in release builds without `openai-branding`) a complete update checking and prompt system for Nori CLI.

### How it fits into the larger codebase

- **Called by** `history_cell.rs` via `new_session_info()` which delegates to `new_nori_session_info()`
- **Called by** `lib.rs` for update prompts when `openai-branding` is disabled
- **Replaces** the original `SessionHeaderHistoryCell` (preserved as dead code for potential future feature flag selection)
- **Uses** `HistoryCell` trait from `@/codex-rs/tui/src/history_cell.rs` for consistent rendering
- **Reads** `~/.nori-config.json` for Nori profile information
- **Reads/writes** `~/.codex/nori-version.json` for update version caching

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

**ASCII Banner Styling:**

The banner uses green+bold for alphabetic characters and dark gray for structural characters (pipes, slashes) to create a two-tone visual effect.

**Agent Picker (`agent_picker.rs`):**

- `agent_picker_params()` consumes `codex_acp::list_available_agents()` so `/agent` can display each `AcpAgentInfo` entry (model name, display name, description, provider slug) with a `SelectionAction` that sends `AppEvent::SetPendingAgent`.
- `acp_model_picker_params()` renders the `/model` fallback page that disables selection when ACP mode is active and points the user back to `/agent`.
- `PendingAgentSelection` holds the selected model/display name pair so the App and `ChatWidget` can store it until the next prompt triggers `AppEvent::SubmitWithAgentSwitch`, at which point the conversation is rebuilt with the new model and the picker view is dismissed.

**Update System (release builds only, `#[cfg(not(debug_assertions))]`):**

The update system is only compiled in release builds and only active when `openai-branding` is disabled:

- `updates.rs`: Version checking against GitHub releases
  - `get_upgrade_version()`: Returns latest version if newer than current
  - `get_upgrade_version_for_popup()`: Same but respects dismissal preferences
  - `dismiss_version()`: Persists user's "don't remind" choice
  - Checks `https://api.github.com/repos/tilework-tech/nori-cli/releases/latest`
  - Caches results in `~/.codex/nori-version.json`
  - Background refresh every 20 hours to avoid blocking startup

- `update_action.rs`: Update command definitions
  - `UpdateAction` enum: `NpmGlobalLatest`, `BunGlobalLatest`, `CargoInstall`
  - `get_update_action()`: Detects installation method via env vars (`NORI_MANAGED_BY_NPM`, `NORI_MANAGED_BY_BUN`, `NORI_MANAGED_BY_CARGO`)
  - Defaults to `CargoInstall` when no manager detected

- `update_prompt.rs`: Interactive update UI
  - `run_update_prompt_if_needed()`: Entry point called from `lib.rs`
  - `UpdatePromptScreen`: Ratatui widget with three options:
    1. Update now (runs the update command)
    2. Skip (continue to TUI)
    3. Skip until next version (persists dismissal)
  - Returns `UpdatePromptOutcome::RunUpdate(action)` to trigger CLI update after TUI exits

### Things to Know

**Profile Display:**

- When `~/.nori-config.json` contains a `profile.baseProfile`, that value is displayed
- When the file is missing or has no profile, displays "(none)"
- Config parsing is permissive - missing fields or invalid JSON result in `None` profile

**Integration Point:**

The original Codex session header (`SessionHeaderHistoryCell`) is preserved with `#[allow(dead_code)]` annotations. The `new_session_info()` function in `history_cell.rs` unconditionally calls the Nori version. Future work could add a feature flag or config option to toggle between them.

**Width Handling:**

The session header uses a max inner width of 60 characters. Directory paths are center-truncated when they exceed available space (e.g., `~/a/b/…/y/z`).

**Update Flow Integration:**

The Nori update system integrates with the parent TUI via feature gates:

```
lib.rs (startup)
    │
    ├─► #[cfg(not(feature = "openai-branding"))]
    │       └─► nori::update_prompt::run_update_prompt_if_needed()
    │               └─► Returns UpdatePromptOutcome::RunUpdate(NoriUpdateAction)
    │                       └─► Converted to tui::UpdateAction via From impl
    │
    └─► #[cfg(feature = "openai-branding")]
            └─► update_prompt::run_update_prompt_if_needed() (OpenAI version)
```

The `From<nori::update_action::UpdateAction>` impl in `tui/src/update_action.rs` converts between the Nori-internal and public update action types, but only when `openai-branding` is disabled.

Created and maintained by Nori.
