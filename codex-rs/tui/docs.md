# Noridoc: nori-tui

Path: @/codex-rs/tui

### Overview

The TUI crate provides Nori's terminal-based user interface. Built on Ratatui, it renders an interactive chat interface for communicating with AI agents. The crate produces the main `nori` binary and handles user input, markdown rendering, diff display, and session management.

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

Key dependencies: `ratatui` for rendering, `crossterm` for terminal events, `pulldown-cmark` for markdown parsing, `tree-sitter-bash` for syntax highlighting.

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

### Things to Know

- The `nori-config` feature enables Nori-specific paths (`~/.nori/cli/`) instead of legacy Codex paths
- The `login` feature (enabled by default) adds `/login` command support via `codex-login`
- Snapshot testing via `insta` is used extensively - see `snapshots/` directory
- The `vt100-tests` feature enables terminal emulator-based integration tests
- Markdown rendering handles streaming content gracefully, updating incrementally as tokens arrive
- The `chatwidget.rs` file is large (~165K) and contains most of the chat rendering logic

Created and maintained by Nori.
