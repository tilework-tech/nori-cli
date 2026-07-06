# Noridoc: nori-rs

Path: @/nori-rs

### Overview

This is the Rust implementation of Nori, a terminal-based AI coding assistant. The codebase provides both a TUI (Terminal User Interface) application and supporting libraries for AI agent communication, command sandboxing, and configuration management. The primary production binary is `nori`, which uses the ACP (Agent Client Protocol) backend to communicate with AI agents like Claude Code.

### How it fits into the larger codebase

The `nori-rs` directory is the root of a Cargo workspace containing all Rust code for the project. The workspace is organized into focused crates that handle specific concerns:

- **Entry points**: `tui/` provides the main TUI application, `cli/` provides the `nori` binary dispatch and sandbox debug utilities
- **ACP integration**: `acp/` handles communication with ACP-compliant agents spawned as local subprocesses, and owns session state, transcripts, Nori config, and the moved-in leaf helpers (shell parsing, notifications, custom prompts, compact constants)
- **Shared infrastructure**: `core/` contains configuration, authentication, and model/provider metadata consumed by the frontends (not by `acp/`)
- **Protocol definitions**: `protocol/`, `app-server-protocol/`, `mcp-types/` define shared type vocabularies
- **Sandboxing**: `sandbox/` (`codex-sandbox`) owns the sandboxed exec engine and platform sandbox selection; `linux-sandbox/`, `windows-sandbox-rs/`, `execpolicy/` provide the platform-specific pieces
- **Utilities**: Various crates in `utils/` provide shared functionality

Most shared crates still follow the inherited `codex-` prefix convention (for example `codex-core` and `codex-protocol`), while Nori-owned entrypoint crates now use `nori-` names such as `nori-acp`, `nori-protocol`, `nori-installed`, and `nori-tui`.

The workspace is converging on a three-layer structure -- publishable ACP-host leaves at the bottom, a headless session harness in the middle, thin frontends on top -- per `@/docs/specs/crate-layering.md`. Milestones already landed: dead Codex-engine subsystems deleted from `codex-core`, the `nori-tui` `nori-config` cargo feature removed (Nori config is the only path), protocol types imported directly from `codex-protocol` (core's re-export detour deleted), `nori-acp`'s dependency on `codex-core` fully severed, and the sandboxed-execution engine extracted from `codex-core` into `codex-sandbox` (`@/nori-rs/sandbox/`).

### Core Implementation

The TUI drives user interaction through a Ratatui-based interface. When using ACP mode (the primary mode for Nori), user prompts flow through `nori-acp` which communicates with ACP agents over JSON-RPC 2.0 via stdin/stdout of a local subprocess. Cloud sessions (`nori cloud`) use the same path: the CLI pins the agent to an external `nori-handroll cloud-acp` child, and all broker/auth/transport concerns live in that binary (nori-sessions repo). Configuration is loaded unconditionally from `~/.nori/cli/config.toml` via `nori-acp`'s config layer.

Architecture:
- nori-tui (TUI) -> Terminal User Interface
  - nori-acp -> ACP Agent Connection -> External ACP Agents (claude, etc); depends only on codex-protocol among inherited crates
  - codex-core -> Config/Auth infrastructure for the frontends (nori-tui, nori-cli)
  - codex-sandbox -> Sandboxed exec engine and platform sandbox selection; imported directly by core, tui, cli, and linux-sandbox
  - codex-protocol -> Shared type vocabulary, imported directly by every consumer

### Things to Know

- Large modules across the workspace use a directory layout (`foo/mod.rs` + `foo/tests.rs`) instead of a single `foo.rs` file, separating test code from production code while preserving Rust module paths

- The workspace uses Rust 2024 edition with strict clippy lints (no `unwrap`, `expect`, or stdout/stderr prints in library code)
- Nori uses ACP exclusively; the legacy HTTP backend code (`codex-api`, `codex-client` crates) and all feature-gated HTTP modules in `codex-core` have been removed
- Cross-platform sandboxing uses Landlock on Linux, Seatbelt on macOS, and restricted tokens on Windows
- No cargo feature may change which crate owns a responsibility (a crate-layering ground rule); the former `nori-config` and `unstable` features that did this are gone
- Snapshot testing via `insta` is used extensively in the TUI for regression testing
- External dependencies are patched: `crossterm` and `ratatui` use custom forks for color query support
- Configuration is stored in `~/.nori/cli/config.toml` with profile support for different model providers and settings

Created and maintained by Nori.
