# Noridoc: nori-tui

Path: @/codex-rs/tui

### Overview

The `nori-tui` crate provides the interactive terminal user interface for Nori, built with the Ratatui framework. It handles the fullscreen TUI experience including chat display, input composition, onboarding flows, and real-time streaming of model responses with markdown rendering.

### How it fits into the larger codebase

```
User Input --> nori-tui --> codex-acp (ACP backend)
                       \--> codex-core (config, auth)
                       \--> codex-protocol (types)
```

The TUI acts as the frontend layer. It:
- Uses `codex-acp` for ACP agent communication (see `@/codex-rs/acp/`)
- Uses `codex-core` for configuration loading and authentication (see `@/codex-rs/core/`)
- Displays approval requests from the ACP layer and forwards user decisions back
- Renders streaming AI responses with markdown and syntax highlighting

The `cli/` crate's `main.rs` dispatches to `nori_tui::run_main()` for interactive mode. Feature flags propagate from CLI to TUI for coordinated modular builds.

Key dependencies: `ratatui` for rendering, `crossterm` for terminal events, `pulldown-cmark` for markdown parsing, `tree-sitter-highlight` for syntax highlighting.

### Core Implementation

Entry point is `main.rs` which delegates to `run_app()` in `lib.rs`. The main event loop in `app.rs` processes:

1. **Terminal events** (keyboard input, resize) via `tui.rs`
2. **ACP events** from the backend (streaming content, approval requests, completion)
3. **App events** for state changes (model selection, config updates)

The chat interface is managed by `chatwidget.rs`, which handles:
- User input composition with multi-line editing
- Message history display with markdown rendering
- File search integration (`file_search.rs`)
- Pager overlay for reviewing long content (`pager_overlay.rs`)

Approval requests from ACP agents are handled through `bottom_pane/approval.rs`, which displays command/patch details and collects user decisions (approve, deny, skip).

The Nori-specific agent picker UI lives in `nori/agent_picker.rs`, allowing users to select between available ACP agents.

**Slash Commands:**

| Command | Description |
|---------|-------------|
| `/agent` | Switch between available ACP agents |
| `/model` | Choose model and reasoning effort |
| `/approvals` | Choose what Nori can do without approval |
| `/config` | Toggle TUI settings (vertical footer) |
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
| `/exit` | Exit Nori (alias for /quit) |

Debug-only commands (not shown in help): `/rollout`, `/test-approval`

The `/logout` command is only available when the `login` feature is enabled. The `/config` command requires the `nori-config` feature.

**Status Line Footer:**

The footer displays:
- Current git branch (refreshes on transcript activity)
- Approval mode label (e.g., "Agent", "Full Access", "Read Only")
- Model name
- Key bindings (Ctrl+C, Esc, Enter)

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

**--yolo Flag:**

The `--dangerously-bypass-approvals-and-sandbox` flag (alias: `--yolo`) works in all builds. When enabled, it overrides any configured sandbox or approval policies to auto-approve all tool operations without prompting.

**Update Checking:**

The TUI uses Nori-specific update checking via files in `@/codex-rs/tui/src/nori/`:
- `update_action.rs`: Update action handling
- `updates.rs`: Version checking against GitHub releases
- `update_prompt.rs`: User prompting for updates

**Error Reporting:**

When errors occur, users are directed to report bugs at `https://github.com/tilework-tech/nori-cli/issues`.

- Snapshot testing via `insta` is used extensively - see `snapshots/` directory
- Markdown rendering uses `pulldown-cmark` for parsing with `tree-sitter-highlight` for syntax highlighting
- Clipboard integration provided via `arboard` crate (disabled on Android/Termux)
- Terminal state is restored on exit or crash via the `tui.rs` module using `color-eyre` for panic handling
- The `chatwidget.rs` file is large (~165K) and contains most of the chat rendering logic

Created and maintained by Nori.
