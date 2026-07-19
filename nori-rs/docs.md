# Noridoc: nori-rs

Path: @/nori-rs

### Overview

This is the Rust implementation of Nori, a terminal-based AI coding assistant. The codebase provides both a TUI (Terminal User Interface) application and supporting libraries for AI agent communication, command sandboxing, and configuration management. The primary production binary is `nori`, which uses the ACP (Agent Client Protocol) backend to communicate with AI agents like Claude Code.

### How it fits into the larger codebase

The `nori-rs` directory is the root of a Cargo workspace containing all Rust code for the project. The workspace is organized into focused crates that handle specific concerns:

- **Entry points**: `tui/` provides the main TUI application, `cli/` provides the `nori` binary dispatch and sandbox debug utilities
- **ACP integration**: `harness/` (`nori-harness`, formerly `nori-acp`) is the headless session harness -- it owns the ACP backend session runtime, the frontend-facing session launch runtime (`harness/src/runtime.rs`: frontends build a `SessionLaunchSpec`, call `launch_session`, and consume `SessionEvent`s), transcripts, hooks, goals, and moved-in leaf helpers (shell parsing, notifications, custom prompts, compact constants). The agent-agnostic hosting machinery (subprocess spawning, wire client, registry, translator) lives in `acp-host/` (`nori-acp-host`) and is re-exported through `nori-harness` so consumers have a single import surface. The Nori config layer lives in `nori-config/` and is imported directly by the frontends (`nori-harness` uses it internally but does not re-export it)
- **Shared infrastructure**: `nori-config/` owns the production CLI configuration boundary. The inherited `core/` crate retains authentication and legacy Codex support used behind `codex-login`, but is not a configuration source for Nori frontends or the harness
- **Protocol definitions**: `protocol/`, `app-server-protocol/`, `mcp-types/` define shared type vocabularies
- **Sandboxing**: `sandbox/` (`codex-sandbox`) owns the sandboxed exec engine and platform sandbox selection; `linux-sandbox/`, `windows-sandbox-rs/`, `execpolicy/` provide the platform-specific pieces
- **Utilities**: Various crates in `utils/` provide shared functionality

Most shared crates still follow the inherited `codex-` prefix convention (for example `codex-core` and `codex-protocol`), while Nori-owned entrypoint crates now use `nori-` names such as `nori-harness`, `nori-protocol`, `nori-installed`, and `nori-tui`.

The workspace is converging on a three-layer structure -- publishable ACP-host leaves at the bottom, a headless session harness in the middle, thin frontends on top -- per `@/docs/specs/crate-layering.md`. Milestones already landed: dead Codex-engine subsystems deleted from `codex-core`, the Nori config layer consolidated in `nori-config`, protocol types imported directly from `codex-protocol`, direct `codex-core` dependencies removed from the Nori frontends and harness, the sandboxed-execution engine extracted into `codex-sandbox`, the agent-agnostic ACP hosting machinery extracted into `nori-acp-host`, and session spawn/resume orchestration moved into the harness runtime. The TUI is now a thin adapter that resolves one `NoriConfig`, injects it into session launches, and maps `SessionEvent`s onto app events.

### Core Implementation

The TUI drives user interaction through a Ratatui-based interface. When using ACP mode (the primary mode for Nori), user prompts flow through `nori-harness` which communicates with ACP agents over JSON-RPC 2.0 via stdin/stdout of a local subprocess. Cloud sessions (`nori cloud`) use the same path: the CLI pins the agent to an external `nori-handroll cloud-acp` child, and all broker/auth/transport concerns live in that binary (nori-sessions repo). Cloud session lifecycle is expressed through standard ACP capabilities -- the agent advertises `sessionCapabilities.{list,resume,close}` (with `loadSession: false`). Entry is picker-first: `nori cloud` probes the agent's `session/list` without creating a session and opens a picker (existing sessions plus an explicit "Start a new session" row), so nothing claims a VM until the user picks. Quitting is a detach -- connection EOF is non-terminal and the session keeps running in the cloud for later reattach via `session/resume` -- while `/close` is the only terminal verb (`session/close`) and returns to the picker; the one-active-session contract lives agent-side (see `@/nori-rs/cli/docs.md`, `@/nori-rs/harness/docs.md`, and `@/nori-rs/tui/docs.md`). Configuration is resolved once from `$NORI_HOME/config.toml` (default `~/.nori/cli/config.toml`) through `nori-config`, then shared with the TUI and injected into each harness session launch.

Architecture:
- nori-tui (TUI) -> Terminal User Interface
  - nori-harness -> ACP session harness -> External ACP Agents (claude, etc); depends only on codex-protocol among inherited crates
    - nori-acp-host -> agent-agnostic ACP hosting leaf (subprocess spawn, wire client, registry, translator); re-exported through nori-harness
  - nori-config -> Nori config layer (~/.nori/cli/config.toml); imported directly by nori-tui and nori-cli, used internally by nori-harness and nori-acp-host
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
- Nori has no Codex config profiles or managed configuration layers. The `profile` and `[profiles]` keys are rejected with guidance to use Nori Skillsets, while `agent` and `[default_models]` cover CLI-owned agent and model selection
- The inherited OpenTelemetry crate, config, and TUI feature were deleted; observability is no longer configured through the CLI config surface

Created and maintained by Nori.
