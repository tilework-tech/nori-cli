# Noridoc: codex-rs

Path: @/codex-rs

### Overview

This is the Rust implementation of Nori, a terminal-based AI coding assistant. The codebase provides both a TUI (Terminal User Interface) application and supporting libraries for AI agent communication, command sandboxing, and configuration management. The primary production binary is `nori`, which uses the ACP (Agent Client Protocol) backend to communicate with AI agents like Claude Code.

### How it fits into the larger codebase

The `codex-rs` directory is the root of a Cargo workspace containing all Rust code for the project. The workspace is organized into focused crates that handle specific concerns:

- **Entry points**: `tui/` provides the main TUI application, `cli/` provides sandbox testing utilities
- **ACP integration**: `acp/` handles communication with ACP-compliant agent subprocesses
- **Core business logic**: `core/` contains configuration, authentication, and conversation management
- **Protocol definitions**: `protocol/`, `app-server-protocol/`, `mcp-types/` define wire formats
- **Sandboxing**: `linux-sandbox/`, `execpolicy/` provide command execution security
- **Utilities**: Various crates in `utils/` provide shared functionality

The crate names follow a `codex-` prefix convention (e.g., `codex-core`, `codex-acp`) except for the TUI which is `nori-tui`.

### Core Implementation

The TUI drives user interaction through a Ratatui-based interface. When using ACP mode (the primary mode for Nori), user prompts flow through `codex-acp` which spawns and communicates with agent subprocesses over stdin/stdout using JSON-RPC 2.0. Configuration is loaded from `~/.nori/cli/config.toml` when the `nori-config` feature is enabled.

Architecture:
- nori-tui (TUI) -> Terminal User Interface
  - codex-acp -> ACP Agent Connection -> External ACP Agents (claude, etc)
  - codex-core -> Config/Auth Management
  - codex-protocol -> Wire Types

### Things to Know

- Large modules across the workspace use a directory layout (`foo/mod.rs` + `foo/tests.rs`) instead of a single `foo.rs` file, separating test code from production code while preserving Rust module paths

- The workspace uses Rust 2024 edition with strict clippy lints (no `unwrap`, `expect`, or stdout/stderr prints in library code)
- Nori uses ACP exclusively; the legacy HTTP backend code (`codex-api`, `codex-client` crates) and all feature-gated HTTP modules in `codex-core` have been removed
- Cross-platform sandboxing uses Landlock on Linux, Seatbelt on macOS, and restricted tokens on Windows
- The `unstable` feature flag guards experimental ACP features like model switching
- Snapshot testing via `insta` is used extensively in the TUI for regression testing
- External dependencies are patched: `crossterm` and `ratatui` use custom forks for color query support
- Configuration is stored in `~/.nori/cli/config.toml` with profile support for different model providers and settings

Created and maintained by Nori.
