# Noridoc: codex-core

Path: @/codex-rs/core

### Overview

The core crate provides foundational functionality shared across Nori components: configuration management, authentication, conversation orchestration, command execution with sandboxing, and MCP (Model Context Protocol) server connections. This is the largest crate in the workspace and contains most business logic.

### How it fits into the larger codebase

```
nori-tui / codex-acp
         |
         v
    codex-core
    /    |    \
   v     v     v
config  auth  exec/sandboxing
         |
         v
    codex-protocol (types)
```

The core crate is depended on by:
- `@/codex-rs/tui/` - for config loading, auth management, conversation orchestration
- `@/codex-rs/acp/` - for config types and auth helpers
- `@/codex-rs/login/` - for auth primitives

Key integrations:
- Uses `codex-protocol` for wire types (`@/codex-rs/protocol/`)
- Uses `codex-execpolicy` for execution policy parsing (`@/codex-rs/execpolicy/`)
- Uses `codex-apply-patch` for file patching (`@/codex-rs/apply-patch/`)
- Uses `codex-rmcp-client` for MCP server communication (`@/codex-rs/rmcp-client/`)

### Core Implementation

**Configuration** (`config/`, `config_loader/`): Loads and merges configuration from:
1. Global config at `~/.codex/config.toml` (or `~/.nori/cli/config.toml` with nori-config feature)
2. Project-local config at `<cwd>/.codex/config.toml`
3. Command-line overrides

**Authentication** (`auth.rs`, `auth/`): Supports multiple auth modes:
- API key via `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, etc.
- ChatGPT login flow with OAuth
- Keyring storage for persistent tokens (`codex-keyring-store`)

**Conversation Management** (`conversation_manager.rs`, `codex.rs`): Orchestrates conversations with AI backends. The `ConversationManager` wraps a `ConversationClient` (implemented by `AcpBackend` or the legacy HTTP backend) and handles:
- Session creation and resumption
- Message history tracking
- Token usage accumulation

**Command Execution** (`exec.rs`, `sandboxing/`): Executes shell commands with optional sandboxing:
- Linux: Landlock LSM (`landlock.rs`) + seccomp
- macOS: Seatbelt sandbox profiles (`seatbelt.rs`)
- Windows: Restricted process tokens (`codex-windows-sandbox`)

**Execution Policy** (`exec_policy.rs`, `command_safety/`): Evaluates whether commands should be auto-approved or require user confirmation based on policy rules.

**MCP Integration** (`mcp/`, `mcp_connection_manager.rs`): Connects to MCP servers (defined in config) to provide additional tools to the AI model.

### Things to Know

- The `deterministic_process_ids` feature is for testing only - produces predictable IDs instead of UUIDs
- Sandbox policies are defined in `.sbpl` files for macOS Seatbelt
- Config uses TOML with optional environment variable expansion
- Auth tokens are stored in the system keyring with fallback to file storage
- The conversation history is stored in `~/.codex/conversations/` (or `~/.nori/cli/conversations/`)
- Error types are defined in `error.rs` and use `thiserror`

Created and maintained by Nori.
