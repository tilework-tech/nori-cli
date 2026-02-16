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


**Configuration Editing** (`config/edit.rs`): Provides a builder API for programmatic config updates via `toml_edit`:

The `ConfigEditsBuilder` allows code to modify `config.toml` atomically without losing comments or formatting:

```rust
ConfigEditsBuilder::new(codex_home)
    .set_default_model("claude-code", "haiku")
    .apply()
    .await?;
```

Key methods:
- `set_default_model(agent, model)`: Persists a model preference to the `[default_models]` table for a specific agent
- `set_path(path, value)`: Sets arbitrary TOML paths for advanced config mutations
- `apply()`: Writes changes asynchronously; locks config file during write
- `apply_blocking()`: Synchronous variant for non-async contexts

The builder is used by the TUI layer (`@/codex-rs/tui/`) to persist user preferences like model selections when `/model` is invoked (see `@/codex-rs/tui/docs.md`).

**Authentication** (`auth.rs`, `auth/`): Supports multiple auth modes:
- API key via `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, etc.
- ChatGPT login flow with OAuth
- Keyring storage for persistent tokens (`codex-keyring-store`)

**Conversation Management** (`conversation_manager.rs`, `codex/mod.rs`): Orchestrates conversations with AI backends. The `ConversationManager` wraps a `ConversationClient` (implemented by `AcpBackend` or the legacy HTTP backend) and handles:
- Session creation and resumption
- Message history tracking
- Token usage accumulation

**Command Execution** (`exec.rs`, `sandboxing/`): Executes shell commands with optional sandboxing:
- Linux: Landlock LSM (`landlock.rs`) + seccomp
- macOS: Seatbelt sandbox profiles (`seatbelt.rs`)
- Windows: Restricted process tokens (`codex-windows-sandbox`)

**Execution Policy** (`exec_policy.rs`, `command_safety/`): Evaluates whether commands should be auto-approved or require user confirmation based on policy rules.

**Custom Prompts** (`custom_prompts.rs`): Discovers and executes user-authored custom prompts from a directory. Two kinds of prompts are supported:

| Kind | Extensions | Behavior |
|------|-----------|----------|
| Markdown | `.md` | Content is read, frontmatter parsed for `description` and `argument_hint`, body becomes the prompt template |
| Script | `.sh`, `.py`, `.js` | File is discovered with an assigned interpreter; content is empty at discovery time; execution happens later via `execute_script()` |

`discover_prompts_in()` scans a directory for supported file extensions, assigns a `CustomPromptKind` (from `@/codex-rs/protocol/src/custom_prompts.rs`), and returns sorted `CustomPrompt` structs. Scripts are assigned interpreters: `.sh` -> `bash`, `.py` -> `python3`, `.js` -> `node`.

`execute_script()` runs a `Script`-kind prompt via its interpreter (e.g. `bash script.sh arg1 arg2`), captures stdout, and enforces a configurable timeout. Returns `Ok(stdout)` on zero exit or `Err(message)` on non-zero exit, I/O error, or timeout.

**MCP Integration** (`mcp/`, `mcp_connection_manager.rs`): Connects to MCP servers (defined in config) to provide additional tools to the AI model.

**Data Flow:**

```
User Input -> Op (UserTurn) -> ConversationManager -> ModelClient -> ResponseStream
    |
    v
Event (TurnStart/Delta/Complete) <- Response Processing <- Tool Execution
```

**Model Client Architecture:**

`client.rs` provides `ModelClient` for communicating with HTTP-based model providers. The `WireApi` enum defines two HTTP-based protocols:
- `WireApi::Responses`: OpenAI Responses API (used by some internal models)
- `WireApi::Chat`: OpenAI Chat Completions API (the default)

ACP (Agent Context Protocol) integration is handled separately in `@/codex-rs/acp`, not embedded in core's model client. This decoupled architecture means codex-core only handles HTTP-based providers.

**User Notifications:**

The `user_notification.rs` module provides OS-level notification support:

| Notification Type | Title | Body Content |
|-------------------|-------|--------------|
| `AgentTurnComplete` | "Nori: Task Complete" | Last assistant message, or "Completed: {input}" fallback |
| `AwaitingApproval` | "Nori: Approval Required" | Truncated command and cwd |
| `Idle` | "Nori: Session Idle" | Idle duration in seconds |

Notification modes:
1. **Native notifications** (`use_native: true`): Uses `notify-rust` for desktop notifications. All calls to `send_native()` are non-blocking -- they spawn a background thread to call `notif.show()`, because some platforms (notably macOS) block synchronously on that call. On X11 Linux, the spawned thread also handles click-to-focus via `wmctrl` or `xdotool`. The `use_native` flag is controlled by `OsNotifications` in the ACP config layer (`@/codex-rs/acp/src/config/types.rs`).
2. **External script** (`notify_command` configured): Invokes user-specified command with JSON payload.

Core's `Config::tui_notifications` is a simple `bool` that controls whether the TUI sends OSC 9 terminal escape sequence notifications. It derives its value from the ACP config's `TerminalNotifications` enum during config loading.

### Things to Know

**Module Structure Convention:**

Large modules use a directory layout (`foo/mod.rs` + `foo/tests.rs`) instead of a single `foo.rs` file. This separates test code from production code while keeping the Rust module path unchanged. Modules using this pattern include `codex/`, `tools/spec/`, and `config/` (which also has a `notifications_tests.rs` alongside `tests.rs`).

- The `deterministic_process_ids` feature is for testing only - produces predictable IDs instead of UUIDs
- Sandbox policies are defined in `.sbpl` files for macOS Seatbelt
- Config uses TOML with optional environment variable expansion
- Auth tokens are stored in the system keyring with fallback to file storage
- The conversation history is stored in `~/.codex/conversations/` (or `~/.nori/cli/conversations/`)
- Error types are defined in `error.rs` and use `thiserror`

**Test Suite Configuration:**

The integration test suite in `@/codex-rs/core/tests/suite` includes timing-sensitive tests that are excluded from normal CI runs:

- `tool_parallelism.rs`: Tests parallel tool execution with strict timing requirements (<750ms threshold). The `read_file_tools_run_in_parallel` test is marked `#[ignore]`.
- `rmcp_client.rs`: Tests remote MCP server communication. Several tests are marked `#[ignore]` as they take >60 seconds due to cargo builds and HTTP server startup.

These tests remain available via `cargo test -- --ignored` but are skipped during routine runs to prevent false failures.

Created and maintained by Nori.
