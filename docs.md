# Noridoc: Nori CLI

Path: @/

### Overview

Nori CLI is a multi-provider terminal-based AI coding assistant built in Rust. It provides a unified interface for interacting with AI agents from Anthropic (Claude Code), OpenAI (Codex), and Google (Gemini). The project uses the Agent Client Protocol (ACP) for subprocess-based agent communication and features a Ratatui-based TUI. The implementation is in Rust (`codex-rs`), with a Node.js launcher for npm distribution (`nori-cli`).

### How it fits into the larger codebase

This is the root repository containing the Nori CLI project:

- **`codex-rs/`**: Main Rust implementation (Cargo workspace with all core functionality)
- **`nori-cli/`**: Node.js launcher for npm distribution (thin wrapper that invokes the Rust binary)
- **`.github/`**: Build and CI configuration
- **`.claude/`**: Skills and configuration for Claude-based development
- **`scripts/`**: Development scripts

The project was originally forked from OpenAI Codex CLI and has been adapted to support multiple AI providers through ACP integration. The `nori-cli` package provides the `nori` command via npm.

### Core Implementation

**Architecture:**

```
┌─────────────────────────────────────────────────┐
│                   nori CLI                      │
│         (codex-rs/tui - main binary)            │
├─────────────────────────────────────────────────┤
│                  nori-tui                       │
│        Interactive Terminal Interface           │
├────────────────────────┬────────────────────────┤
│     codex-acp (acp/)   │   codex-core (core/)   │
│  ACP Agent Connection  │  Config, Auth, Tools   │
│  Subprocess Spawning   │  Sandbox, Utilities    │
├────────────────────────┴────────────────────────┤
│           codex-protocol (protocol/)            │
│         Events, Operations, Types               │
└─────────────────────────────────────────────────┘
                    │
                    ▼
        ┌───────────────────────┐
        │   ACP Agent Process   │
        │  (claude-code, etc.)  │
        └───────────────────────┘
```

**Entry Points:**

| Command           | Description        | Implementation        |
| ----------------- | ------------------ | --------------------- |
| `nori`            | Interactive TUI    | `codex-rs/tui`        |
| `nori exec`       | Headless execution | `codex-rs/exec`       |
| `nori mcp-server` | MCP tool provider  | `codex-rs/mcp-server` |
| `nori login`      | Authentication     | `codex-rs/login`      |
| `nori apply`      | Apply cloud diffs  | `codex-rs/chatgpt`    |

**Model Providers (via ACP):**

- Claude Code (primary)
- Codex
- Gemini

**Installation:**

```bash
npm i -g nori-ai-cli
```

**Configuration:**

Stored in `~/.nori/cli/`:

- `config.toml`: Main configuration
- `sessions/`: Saved conversations
- `history.jsonl`: Message history

**Session Management:**

Conversations are recorded to `~/.nori/cli/sessions/` and can be resumed:

```bash
nori resume              # Show picker
nori resume --last       # Most recent
nori resume <SESSION_ID> # Specific session
```

**MCP Support:**

Nori acts as both MCP client and server:

- **Client**: Connects to MCP servers defined in config
- **Server**: Exposes Nori tools via `nori mcp-server`

### Things to Know

- The crate naming uses a `codex-` prefix (legacy from the OpenAI Codex fork), except for `nori-tui` and `nori-installed`
- The `nori-config` feature flag enables Nori-specific configuration paths (`~/.nori/cli/`) instead of the legacy Codex paths (`~/.codex/`)
- The `unstable` feature flag gates experimental ACP features like model switching
- Cross-platform sandboxing is implemented using Landlock (Linux), Seatbelt (macOS), and restricted tokens (Windows)
- Snapshot testing with `insta` is used extensively for TUI regression testing
- The project uses `just` for build automation in `codex-rs` and `pnpm` for Node.js workspace management

Created and maintained by Nori.
