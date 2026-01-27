# Noridoc: Nori CLI

Path: @/

### Overview

Nori CLI is a multi-provider terminal-based AI coding assistant built in Rust. It provides a unified interface for interacting with AI agents from Anthropic (Claude Code), OpenAI (Codex), and Google (Gemini). The project uses the Agent Client Protocol (ACP) for subprocess-based agent communication and features a Ratatui-based TUI.

### How it fits into the larger codebase

This is the root repository containing the Nori CLI project. The primary source code lives in `@/codex-rs/`, which is a Cargo workspace containing all Rust crates. The repository also includes:

- Build and CI configuration in `@/.github/`
- Skills and configuration for Claude-based development in `@/.claude/`
- NPM packaging configuration in `@/nori-cli/`
- Development scripts in `@/scripts/`

The project was originally forked from OpenAI Codex CLI and has been adapted to support multiple AI providers through ACP integration.

### Core Implementation

The main entry point is the `nori` binary built from `@/codex-rs/tui/`. On launch:

1. Config is loaded from `~/.nori/cli/config.toml` (when `nori-config` feature is enabled)
2. Available ACP agents are detected via package manager checks
3. The TUI initializes and presents an interactive chat interface
4. User prompts flow through ACP to the selected agent subprocess

The architecture follows a layered design:

```
+------------------+
|    nori-tui      |  <-- User-facing TUI binary
+------------------+
         |
+------------------+
|    codex-acp     |  <-- ACP agent spawning and communication
+------------------+
         |
+------------------+
|   codex-core     |  <-- Config, auth, conversation management
+------------------+
         |
+------------------+
| codex-protocol   |  <-- Wire types and protocol definitions
+------------------+
```

### Things to Know

- The crate naming uses a `codex-` prefix (legacy from the OpenAI Codex fork), except for `nori-tui` and `nori-installed`
- The `nori-config` feature flag enables Nori-specific configuration paths (`~/.nori/cli/`) instead of the legacy Codex paths (`~/.codex/`)
- The `unstable` feature flag gates experimental ACP features like model switching
- Cross-platform sandboxing is implemented using Landlock (Linux), Seatbelt (macOS), and restricted tokens (Windows)
- Snapshot testing with `insta` is used extensively for TUI regression testing

Created and maintained by Nori.
