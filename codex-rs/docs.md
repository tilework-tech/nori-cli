# Noridoc: codex-rs

Path: @/codex-rs

### Overview

This is the root of the Rust Cargo workspace containing the Codex CLI implementation. Codex is a local coding agent that provides AI-assisted coding capabilities through terminal-based and programmatic interfaces. The workspace contains the core business logic, multiple client interfaces (TUI, exec, MCP server, app-server), and supporting utilities for authentication, sandboxing, patch application, and model communication.

### How it fits into the larger codebase

The `codex-rs` directory is the primary source code location for all Rust components. It provides:

- **CLI entry points**: The `cli` crate serves as the main multitool binary, dispatching to TUI (`tui`), headless execution (`exec`), MCP server (`mcp-server`), and app server (`app-server`) modes
- **Core library**: The `core` crate contains shared business logic used by all interfaces
- **Protocol definitions**: The `protocol` crate defines message types shared across the codebase
- **Model providers**: Support for OpenAI, Claude, Ollama, LM Studio, and Gemini ACP through various backend crates

### Core Implementation

The workspace is organized into crate categories:

| Category | Crates | Purpose |
|----------|--------|---------|
| Entry Points | `cli`, `tui`, `exec`, `app-server`, `mcp-server` | User-facing interfaces |
| Core Logic | `core`, `protocol`, `common` | Business logic and shared types |
| Authentication | `login`, `chatgpt`, `backend-client` | OAuth, API keys, token management |
| Sandbox | `linux-sandbox`, `execpolicy`, `execpolicy2`, `process-hardening` | Security enforcement |
| Patch System | `apply-patch` | Structured file modification |
| MCP | `mcp-types`, `rmcp-client` | Model Context Protocol support |
| ACP | `acp`, `mock-acp-agent` | Agent Context Protocol support |
| Analytics | `installed` | Install tracking and usage analytics |
| Testing | `tui-pty-e2e` | PTY-based black-box TUI testing |
| Utilities | `utils/*`, `async-utils`, `ansi-escape`, `feedback` | Helper libraries |

Key architectural patterns:
- **Event-driven communication**: Core uses `Event`/`Op` message passing between components
- **Configuration layering**: CLI args -> environment -> config.toml -> defaults
- **Sandbox enforcement**: Platform-specific sandboxing (Seatbelt on macOS, Landlock on Linux, restricted tokens on Windows)
- **Session persistence**: Rollout recording enables session resume

### Things to Know

The workspace uses Rust 2024 edition with strict Clippy lints (`clippy::unwrap_used = "deny"`, `clippy::expect_used = "deny"`).

Library crates (`core`, `tui` lib portion, `exec`) deny direct stdout/stderr writes to ensure output goes through proper abstractions.

The `codex-linux-sandbox` binary can be embedded into the main CLI via arg0 dispatch (`codex-arg0` crate) for single-binary distribution.

External dependencies are patched: `crossterm` and `ratatui` use custom forks for color query support.

Configuration is stored in `~/.codex/config.toml` with profile support for different model providers and settings.

Created and maintained by Nori.
