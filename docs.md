# Noridoc: nori-cli

Path: @/

### Overview

This repository contains the Nori AI CLI, a local coding agent that runs on your computer. It provides AI-assisted coding capabilities through a terminal-based interface, with support for multiple model providers including ACP (Agent Context Protocol), sandboxed command execution, and IDE integration. The implementation is in Rust (`codex-rs`), with a Node.js launcher for npm distribution (`codex-cli`).

### How it fits into the larger codebase

This is a monorepo containing:

- **`codex-rs/`**: Main Rust implementation (Cargo workspace with all core functionality)
- **`codex-cli/`**: Node.js launcher for npm distribution (thin wrapper that invokes the Rust binary)

The Rust codebase in `codex-rs` contains the entire implementation. The `codex-cli` package provides the `nori` command via npm.

### Core Implementation

**Architecture:**

```
┌─────────────────────────────────────────────────┐
│                   nori CLI                      │
│  (codex-rs/cli - main binary dispatcher)        │
├─────────┬─────────┬────────────┬────────────────┤
│   TUI   │  Exec   │ App Server │   MCP Server   │
│ (tui/)  │ (exec/) │(app-server)│ (mcp-server/)  │
├─────────┴─────────┴────────────┴────────────────┤
│              codex-core (core/)                 │
│   Config, Auth, Tools, Sandbox, Conversation    │
├─────────────────────────────────────────────────┤
│           codex-protocol (protocol/)            │
│         Events, Operations, Types               │
└─────────────────────────────────────────────────┘
```

**Entry Points:**

| Command           | Description        | Implementation        |
| ----------------- | ------------------ | --------------------- |
| `nori`            | Interactive TUI    | `codex-rs/tui`        |
| `nori exec`       | Headless execution | `codex-rs/exec`       |
| `nori app-server` | IDE integration    | `codex-rs/app-server` |
| `nori mcp-server` | MCP tool provider  | `codex-rs/mcp-server` |
| `nori login`      | Authentication     | `codex-rs/login`      |
| `nori apply`      | Apply cloud diffs  | `codex-rs/chatgpt`    |

**Model Providers:**

- OpenAI (default)
- Gemini ACP (via Agent Context Protocol)
- Ollama (local, --oss, requires `oss-providers` feature)
- LM Studio (local, --oss, requires `oss-providers` feature)

### Things to Know

**Installation:**

```bash
npm i -g nori-ai-cli   # npm
```

**Configuration:**

Stored in `~/.nori/cli/`:

- `config.toml`: Main configuration
- `sessions/`: Saved conversations

**Sandbox Enforcement:**

Commands run in a security sandbox:

- macOS: Seatbelt (`/usr/bin/sandbox-exec`)
- Linux: Landlock + seccomp
- Windows: Restricted process tokens

Modes: `ReadOnly`, `WorkspaceWrite`, `DangerFullAccess`

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

**Development:**

The project uses:

- Rust 2024 edition with strict Clippy lints
- pnpm for Node.js workspace management
- `just` for build automation in `codex-rs`

See `AGENTS.md` for detailed development guidelines.

Created and maintained by Nori.
