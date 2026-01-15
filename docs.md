# Noridoc: nori-cli (Codex CLI)

Path: @/

### Overview

This repository contains the Codex CLI, a local coding agent from OpenAI that runs on your computer. It provides AI-assisted coding capabilities through a terminal-based interface, with support for multiple model providers, sandboxed command execution, and IDE integration. The primary implementation is in Rust (`codex-rs`), with supporting TypeScript components for Node.js distribution.

### How it fits into the larger codebase

This is the root of a monorepo containing:

- **`codex-rs/`**: Main Rust implementation (Cargo workspace)
- **`codex-cli/`**: Node.js wrapper for npm distribution
- **`sdk/typescript/`**: TypeScript SDK for programmatic Codex usage
- **`docs/`**: User documentation

The Rust codebase in `codex-rs` is the core implementation, with Node.js and TypeScript components providing distribution and integration interfaces.

### Core Implementation

**Architecture:**

```
┌─────────────────────────────────────────────────┐
│                   codex CLI                     │
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

| Command            | Description        | Implementation        |
| ------------------ | ------------------ | --------------------- |
| `codex`            | Interactive TUI    | `codex-rs/tui`        |
| `codex exec`       | Headless execution | `codex-rs/exec`       |
| `codex app-server` | IDE integration    | `codex-rs/app-server` |
| `codex mcp-server` | MCP tool provider  | `codex-rs/mcp-server` |
| `codex login`      | Authentication     | `codex-rs/login`      |
| `codex apply`      | Apply cloud diffs  | `codex-rs/chatgpt`    |

**Model Providers:**

- OpenAI (default)
- Ollama (local, --oss)
- LM Studio (local, --oss)
- Gemini ACP (via Agent Context Protocol)

### Things to Know

**Installation:**

```bash
npm i -g @openai/codex   # npm
brew install --cask codex # Homebrew
```

**Configuration:**

Stored in `~/.codex/`:

- `config.toml`: Main configuration
- `auth.json`: Authentication tokens
- `sessions/`: Saved conversations
- `projects.toml`: Per-project trust settings

**Sandbox Enforcement:**

Commands run in a security sandbox:

- macOS: Seatbelt (`/usr/bin/sandbox-exec`)
- Linux: Landlock + seccomp
- Windows: Restricted process tokens

Modes: `ReadOnly`, `WorkspaceWrite`, `DangerFullAccess`

**Session Management:**

Conversations are recorded to `~/.codex/sessions/` and can be resumed:

```bash
codex resume              # Show picker
codex resume --last       # Most recent
codex resume <SESSION_ID> # Specific session
```

**MCP Support:**

Codex acts as both MCP client and server:

- **Client**: Connects to MCP servers defined in config
- **Server**: Exposes Codex tools via `codex mcp-server`

**Development:**

The project uses:

- Rust 2024 edition with strict Clippy lints
- pnpm for Node.js workspace management
- `just` for build automation in `codex-rs`

See `AGENTS.md` for detailed development guidelines.

Created and maintained by Nori.
