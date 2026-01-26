# Noridoc: tui

Path: @/codex-rs/tui

### Overview

The `nori-tui` crate provides the interactive terminal user interface for Nori, built with the Ratatui framework. It handles the fullscreen TUI experience including chat display, input composition, onboarding flows, and real-time streaming of model responses with markdown rendering.

### How it fits into the larger codebase

TUI is the primary entry point, invoked when running `nori` without a subcommand:

- **Depends on** `codex-core` for conversation management, configuration, and authentication
- **Depends on** `codex-acp` for ACP agent backend (Claude integration via Agent Context Protocol)
- **Depends on** `codex-common` for CLI argument parsing and shared utilities
- **Uses** `codex-protocol` types for events and messages
- **Optionally integrates** `codex-login` via feature flags

The `cli/` crate's `main.rs` dispatches to `nori_tui::run_main()` for interactive mode. Feature flags propagate from CLI to TUI for coordinated modular builds.

### Core Implementation

**Entry Point:**

`run_main()` in `lib.rs`:
1. Parses CLI arguments and loads configuration
2. Initializes tracing (file + OpenTelemetry)
3. Runs onboarding if needed (login, trust screen)
4. Launches the main `App::run()` loop

**Application Core:**

- `app.rs`: Main `App` struct managing application state and event loop
- `app_event.rs`: Application-level events (key input, model responses, etc.)
- `tui.rs`: Terminal initialization and restoration

**Agent Spawning (`chatwidget/agent.rs`):**

The TUI uses ACP (Agent Context Protocol) for Claude integration:

- `spawn_agent()`: Entry point that spawns the ACP agent via `codex_acp::spawn_acp_agent()`
- Returns `AgentHandle` and `impl Stream<Item = Event>` for async event consumption

The TUI shows "Starting agent..." feedback during slow agent startup (e.g., when npx/bunx needs to resolve and download dependencies for the first time).

**Chat Widget (`chatwidget/`):**

The `ChatWidget` is the main component containing:
- Message history display with markdown rendering
- Input composition area
- Tool call visualization
- Approval prompts

**Configuration Flow:**

1. CLI flags (`--model` and `--yolo` always available)
2. Environment variables (`NORI_MODEL`, `ANTHROPIC_API_KEY`, etc.)
3. Config file (`~/.nori/cli/config.toml`)
4. Defaults

**Slash Commands:**

Available commands via `/` prefix:

| Command | Description |
|---------|-------------|
| `/agent` | Switch between available ACP agents |
| `/model` | Choose model and reasoning effort |
| `/approvals` | Choose what Nori can do without approval |
| `/review` | Review current changes and find issues |
| `/new` | Start a new chat during a conversation |
| `/init` | Create an AGENTS.md file with instructions |
| `/compact` | Summarize conversation to prevent context limit |
| `/undo` | Ask Nori to undo a turn |
| `/diff` | Show git diff (including untracked files) |
| `/mention` | Mention a file |
| `/status` | Show session configuration and token usage |
| `/mcp` | List configured MCP tools |
| `/login` | Log in to the current agent |
| `/logout` | Show logout instructions |
| `/quit` | Exit Nori |

The `/login` and `/logout` commands require the `login` feature to be enabled.

**Status Line Footer:**

The footer displays:
- Current git branch (refreshes on transcript activity)
- Approval mode label (e.g., "Agent", "Full Access", "Read Only")
- Model name
- Key bindings (Ctrl+C, Esc, Enter)

The approval mode is determined by `approval_mode_label()` from `@/codex-rs/common/src/approval_presets.rs`, which maps current approval and sandbox policies to a preset name.

### Things to Know

**Cargo Feature Flags:**

| Feature | Dependencies | Default | Purpose |
|---------|--------------|---------|---------|
| `unstable` | `codex-acp/unstable` | Yes | Unstable ACP features like model switching |
| `nori-config` | - | Yes | Use Nori's simplified ACP-only config |
| `login` | `codex-login`, `codex-utils-pty` | Yes | ChatGPT/API login functionality |
| `otel` | `opentelemetry-appender-tracing` | No | OpenTelemetry tracing export |
| `vt100-tests` | - | No | vt100-based emulator tests |
| `debug-logs` | - | No | Verbose debug logging |

**Update Checking:**

The TUI uses Nori-specific update checking via files in `@/codex-rs/tui/src/nori/`:
- `update_action.rs`: Update action handling
- `updates.rs`: Version checking against GitHub releases
- `update_prompt.rs`: User prompting for updates

**Error Reporting:**

When errors occur, users are directed to report bugs at `https://github.com/tilework-tech/nori-cli/issues`.

**--yolo Flag:**

The `--dangerously-bypass-approvals-and-sandbox` flag (alias: `--yolo`) works in all builds. When enabled, it overrides any configured sandbox or approval policies to auto-approve all tool operations without prompting the user.

**Terminal Restoration:**

The TUI uses `color-eyre` for panic handling and ensures terminal state is restored on exit or crash via the `tui.rs` module.

**Markdown Rendering:**

The `markdown/` module provides streaming markdown rendering using `pulldown-cmark` with syntax highlighting via `tree-sitter-highlight`.

**Clipboard Integration:**

Clipboard support is provided via `arboard` crate, except on Android/Termux where it's disabled.

Created and maintained by Nori.
