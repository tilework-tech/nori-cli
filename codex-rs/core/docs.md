# Noridoc: core

Path: @/codex-rs/core

### Overview

The `codex-core` crate is the central business logic library for Codex. It provides the AI conversation management, tool execution, configuration handling, authentication, and sandboxing capabilities that all Codex interfaces depend upon. This is designed as a reusable library crate for building Rust applications that use Codex.

### How it fits into the larger codebase

Core serves as the foundation consumed by all entry points:

- **TUI** (`@/codex-rs/tui`): Uses `ConversationManager`, `Config`, `AuthManager` for interactive sessions
- **Exec** (`@/codex-rs/exec`): Uses same core types for headless automation
- **App Server** (`@/codex-rs/app-server`): Wraps core for JSON-RPC communication
- **MCP Server** (`@/codex-rs/mcp-server`): Exposes Codex tools to MCP clients

Core depends on:
- `codex-protocol` for message types and protocol definitions
- `codex-apply-patch` for structured file modifications
- `codex-linux-sandbox` for Linux sandboxing
- Various utility crates for specific functionality

### Core Implementation

**Entry Points:**

- `ConversationManager` - Creates and resumes conversations, manages session lifecycle
- `CodexConversation` - Active conversation handle for submitting operations and receiving events
- `Config` - Loaded configuration with model, sandbox, and approval settings

**Key Data Flow:**

```
User Input -> Op (UserTurn) -> ConversationManager -> ModelClient -> ResponseStream
    |
    v
Event (TurnStart/Delta/Complete) <- Response Processing <- Tool Execution
```

**State Management:**

The `state/` module manages conversation state through:
- `session.rs`: Per-session state including MCP connections and tool registry
- `service.rs`: Long-running services (history, delegate)
- `turn.rs`: Per-turn state tracking

**Tool System:**

Located in `tools/`:
- `registry.rs`: Registers available tools (shell, apply_patch, read_file, list_dir, grep_files, etc.)
- `orchestrator.rs`: Manages tool execution flow
- `router.rs`: Routes tool calls to appropriate handlers
- `handlers/`: Implementation of each tool

**Configuration:**

The `config/` module handles:
- `mod.rs`: Core `Config` struct with all settings
- `types.rs`: Configuration type definitions
- `profile.rs`: Config profile support
- `edit.rs`: Config file modification utilities

### Things to Know

**Sandbox Enforcement:**

Sandboxing is enforced through `safety.rs` and `sandboxing/`:
- macOS: Seatbelt profiles via `/usr/bin/sandbox-exec`
- Linux: Landlock + seccomp via `codex-linux-sandbox`
- Windows: Restricted process tokens

The `SandboxMode` enum controls the policy: `ReadOnly`, `WorkspaceWrite`, `DangerFullAccess`.

**Authentication:**

The `auth/` module manages:
- OAuth tokens from ChatGPT login
- API keys (environment variable or stored)
- Token refresh logic
- `AuthManager` provides shared access across components

**Model Client Architecture:**

The `client.rs` defines `ModelClient` trait implemented by:
- Default client for OpenAI-compatible APIs
- ACP client for Agent Context Protocol agents

Response streaming uses `ResponseStream` of `ResponseEvent` items.

For ACP providers (`wire_api: WireApi::Acp`), the client looks up subprocess configuration via `codex_acp::get_agent_config(self.config.model)` from `@/codex-rs/acp/src/registry.rs`. The registry is **model-centric**: it maps model names (e.g., "mock-model", "gemini-2.5-flash") to `AcpAgentConfig` structs containing provider identifier, command, and args. This differs from the provider-based approach used for HTTP APIs. ACP providers should not define `env_key` or `env_key_instructions` in their `ModelProviderInfo` entries, as they communicate via subprocess rather than HTTP APIs.

**Session Recording:**

The `rollout/` module handles session persistence:
- `recorder.rs`: Writes session events to disk
- `list.rs`: Lists and queries saved sessions
- Sessions stored in `~/.codex/sessions/` with JSON-lines format

**MCP Integration:**

The `mcp/` and `mcp_connection_manager.rs` modules manage MCP server connections defined in config.

**Context Management:**

The `context_manager/` maintains conversation history with:
- Message history tracking
- Context window management
- History normalization for model input

Created and maintained by Nori.
