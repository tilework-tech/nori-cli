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
- `edit.rs`: Config file modification utilities via `ConfigEditsBuilder`

**Config Persistence via `edit.rs`:**

All config mutations should go through `ConfigEditsBuilder` which provides atomic read-modify-write operations using temp file + rename. Key capabilities:
- `set_model()`, `set_agent()`, `set_project_trust_level()`, `replace_mcp_servers()` - Domain-specific setters
- `set_path(&["cli", "first_launch_complete"], toml_value(true))` - Generic path setter for arbitrary TOML paths
- Re-exports `toml_edit::Item` and `toml_value` so callers can build TOML values without adding `toml_edit` as a direct dependency

This pattern ensures config changes merge with existing content rather than overwriting the entire file.

### Things to Know

**Test Suite Configuration:**

The integration test suite in `@/codex-rs/core/tests/suite` includes timing-sensitive tests that are excluded from normal CI runs to improve reliability:

- `tool_parallelism.rs`: Tests parallel tool execution with strict timing requirements (<750ms threshold). The `read_file_tools_run_in_parallel` test is marked `#[ignore]` to prevent CI timeouts.
- `rmcp_client.rs`: Tests remote MCP server communication. Both `streamable_http_tool_call_round_trip` and `streamable_http_with_oauth_round_trip` are marked `#[ignore]` as they take >60 seconds due to cargo builds and HTTP server startup.

These tests remain available for explicit execution via `cargo test -- --ignored` but are skipped during routine test runs to prevent false failures from system load or timing variance.

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

`client.rs` provides `ModelClient` for communicating with HTTP-based model providers. Response streaming uses `ResponseStream` of `ResponseEvent` items.

The `WireApi` enum defines two HTTP-based protocols:
- `WireApi::Responses`: OpenAI Responses API (used by some internal models)
- `WireApi::Chat`: OpenAI Chat Completions API (the default)

ACP (Agent Context Protocol) integration is handled separately in `@/codex-rs/acp`, not embedded in core's model client. This decoupled architecture means codex-core only handles HTTP-based providers.

**User Notifications:**

The `user_notification.rs` module provides OS-level notification support. Key exports:

- `UserNotifier`: Manages notification delivery via native or external command
- `UserNotification`: Enum of notification event types with human-readable content

| Notification Type | Title | Body Content |
|-------------------|-------|--------------|
| `AgentTurnComplete` | "Nori: Task Complete" | Last assistant message, or "Completed: {input}" fallback |
| `AwaitingApproval` | "Nori: Approval Required" | Truncated command and cwd |
| `Idle` | "Nori: Session Idle" | Idle duration in seconds |

**Notification Modes:**

The `UserNotifier` supports two delivery modes:

1. **Native notifications** (`use_native: true`): Uses `notify-rust` to send desktop notifications directly. On X11 Linux, supports click-to-focus via `wmctrl` or `xdotool`.
2. **External script** (`notify_command` configured): Invokes user-specified command with JSON payload as argument.

```
┌─────────────────────┐   use_native=true    ┌─────────────────────┐
│  UserNotifier       │─────────────────────►│  notify-rust        │
│  .notify()          │   (no command)       │  (desktop notif)    │
│                     │                      │                     │
│                     │   command set        ┌─────────────────────┐
│                     │─────────────────────►│  External script    │
│                     │   (legacy mode)      │  (JSON arg)         │
└─────────────────────┘                      └─────────────────────┘
```

The `use_native` flag controls whether native notifications are sent when no external command is configured. Production code passes `true`, test code passes `false` to avoid notification spam during tests.

**Window Focus (X11 Linux):**

When a native notification is clicked on X11 Linux, the `focus_window_by_pid()` function attempts to focus the terminal window:
- Tries `wmctrl -l -p` to find window by PID, then `wmctrl -i -a` to activate
- Falls back to `xdotool search --pid` and `windowactivate`
- Only works on X11 (not Wayland), detected via `XDG_SESSION_TYPE` or `DISPLAY`/`WAYLAND_DISPLAY` environment variables

**JSON Serialization:**

Notifications serialize to JSON (kebab-case keys) for external scripts. The `title()` and `body()` methods provide human-readable content for native notifications, with command strings truncated to 100 characters.

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
