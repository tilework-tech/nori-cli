# Noridoc: nori

Path: @/codex-rs/tui/src/nori

### Overview

The `nori` module contains Nori-specific TUI customizations that replace or extend the default Codex UI behavior. It provides branded session headers, agent picking, feedback redirection, and a Nori-specific update checking mechanism that queries GitHub releases instead of OpenAI's update system.

### How it fits into the larger codebase

- **Called by** `history_cell.rs` via `new_session_info()` which delegates to `new_nori_session_info()`
- **Replaces** the original Codex session header (preserved as dead code for potential future feature flag selection)
- **Uses** `HistoryCell` trait from `@/codex-rs/tui/src/history_cell.rs` for consistent rendering
- **Reads** `~/.nori-config.json` for Nori profile information
- **Conditionally compiled** based on feature flags - modules like `feedback.rs`, `updates.rs` are only included when their corresponding upstream features are disabled
- **Re-exported** by `@/codex-rs/tui/src/lib.rs` to provide unified access to update types regardless of which update system is active

### Core Implementation

**Session Header (`session_header.rs`):**

The `NoriSessionHeaderCell` struct implements `HistoryCell` and renders:

```
╭───────────────────────────────────────────────────╮
│ Nori CLI v0.x.x                                   │
│                                                   │
│ directory: ~/path/to/project                      │
│ agent:     claude-sonnet                          │
│ profile:   senior-swe                             │
│                                                   │
│ Instruction Files                                 │
│   ~/.claude/CLAUDE.md              (active)       │
│   ~/project/.claude/CLAUDE.md      (active)       │
│   ~/project/AGENTS.md              (dimmed)       │
╰───────────────────────────────────────────────────╯

  Run 'npx nori-ai install' to set up Nori AI enhancements
```

**Agent-Specific Instruction File Discovery:**

The `discover_all_instruction_files()` function discovers ALL instruction files in the directory hierarchy and user home, marking them as active/inactive based on the current agent's activation algorithm:

| Agent   | Active Files                                              |
|---------|----------------------------------------------------------|
| Claude  | `.claude/CLAUDE.md`, `CLAUDE.md`, `CLAUDE.local.md` (all three per directory) |
| Codex   | `AGENTS.override.md` OR `AGENTS.md` per directory (override takes precedence) |
| Gemini  | `GEMINI.md` only (no hidden variants, no overrides)       |

**Discovery Order:**
1. Home directory configs first (`~/.claude/CLAUDE.md`, `~/.codex/AGENTS.md`, `~/.gemini/GEMINI.md`)
2. Project configs from git root to cwd (or just cwd if no git root)

**Key functions:**

- `new_nori_session_info()`: Entry point called by `history_cell::new_session_info()`. Creates the composite cell with header + help text
- `detect_agent_kind(agent)`: Parses agent string to determine `AgentKindSimple` (Claude, Codex, Gemini, or None)
- `discover_all_instruction_files(cwd, agent_kind)`: Discovers all instruction files and applies agent-specific activation algorithm
- `discover_all_instruction_files_with_home(cwd, agent_kind, home_dir)`: Internal variant accepting optional custom home directory for testing
- `read_nori_profile(cwd)`: Walks from the given directory upward, returning the profile from the nearest ancestor containing a `.nori-config.json` file
- `format_directory()`: Relativizes paths to home directory with truncation for narrow terminals
- `new_nori_status_output()`: Creates the composite cell for `/status` command output

**Exit Message (`exit_message.rs`):**

The `ExitMessageCell` struct implements `HistoryCell` and displays session statistics when users quit the TUI. Called by `ChatWidget::create_exit_message_cell()` when `AppEvent::ExitRequest` is received.

Display format (60-char max inner width, bordered):
- Goodbye message: "Goodbye! Thanks for using Nori." (green bold + dim styling)
- Session ID
- Messages: User/Assistant/Total counts
- Tool Calls: Sorted alphabetically by name (e.g., "Bash: 3  Read: 5"), or "(none)"
- Skills Used: Bullet list, or "(none)"
- Subagents Used: Bullet list, or "(none)"

The cell is inserted into the chat history and displayed before terminal restoration, allowing the exit summary to remain in scrollback after the TUI exits.

**Agent Picker (`agent_picker.rs`):**

- `agent_picker_params()` consumes `codex_acp::list_available_agents()` so `/agent` can display each `AcpAgentInfo` entry with a `SelectionAction` that sends `AppEvent::SetPendingAgent`
- `acp_model_picker_params()` renders a fallback when the `unstable` feature is disabled
- `PendingAgentSelection` holds the selected model/display name pair until the next prompt triggers `AppEvent::SubmitWithAgentSwitch`
- `get_agent_info(model_name)` looks up agent metadata (display name, description) from the available agents list by model name (case-insensitive). Used by `chatwidget.rs` to resolve human-readable display names for approval dialogs.

**Feedback Redirect (`feedback.rs`):**

Compiled only when `feedback` feature is disabled. Redirects `/feedback` command to GitHub Discussions instead of OpenAI's feedback system.

**Update System (`update_action.rs`, `updates.rs`, `update_prompt.rs`):**

Compiled only when `upstream-updates` feature is disabled. Provides Nori-specific update checking:
- `UpdateAction` enum with `NpmGlobalLatest`, `BunGlobalLatest`, and `Manual` variants
- `get_update_action()` checks `NORI_MANAGED_BY_BUN` then `NORI_MANAGED_BY_NPM` env vars
- Queries GitHub releases API with caching in `~/.codex/nori-version.json` (20-hour refresh)

### Things to Know

**Profile Display:**

- Profile is resolved by walking from cwd upward through parent directories, using the nearest ancestor containing `.nori-config.json`
- Supports both new format (`agents.claude-code.profile.baseProfile`) and old format (`profile.baseProfile`)
- When no config file is found in any ancestor, displays "(none)"

**Instruction Files Display:**

- Active files are shown in normal text; inactive files are dimmed
- Home directory configs appear first in the list, followed by project configs
- The `InstructionFile` struct tracks both path and activation status
- Tests use `discover_all_instruction_files_with_home()` with `None` home directory to avoid picking up real home configs

**Config Adapter (`config_adapter.rs`):**

Provides integration between the Nori config system (from `@/codex-rs/acp/src/config/`) and the TUI:
- `get_nori_home()`: Returns the canonicalized Nori home path (`~/.nori/cli`)
- `setup_nori_config_environment()`: Sets `CODEX_HOME` env var to redirect codex-core's config loading to the Nori location
- `get_persisted_agent_model()`: Returns the user's persisted agent preference from `NoriConfig`

**Model Resolution Priority:**

When the TUI starts without a `--model` CLI argument:
1. `model` field in config.toml (if explicitly set)
2. `agent` field in config.toml (persisted user preference from `/agent` command)
3. `DEFAULT_MODEL` constant ("claude-code")

**Width Handling:**

The session header uses a max inner width of 60 characters. Directory paths are center-truncated when they exceed available space.

**Conditional Compilation:**

```
session_header.rs, agent_picker.rs  -> Always included
feedback.rs                         -> #[cfg(not(feature = "feedback"))]
update_action.rs                    -> #[cfg(not(feature = "upstream-updates"))]
update_prompt.rs, updates.rs        -> #[cfg(all(not(feature = "upstream-updates"), not(debug_assertions)))]
```

**Onboarding Module (`onboarding/`):**

Provides Nori-branded first-launch onboarding flow:
- `first_launch.rs`: First-launch detection via `~/.nori/cli/config.toml` existence
- `welcome.rs`: ASCII banner welcome screen with Nori branding
- `trust_directory.rs`: Directory trust prompt
- `onboarding_screen.rs`: Orchestrates the multi-step onboarding flow

Created and maintained by Nori.
