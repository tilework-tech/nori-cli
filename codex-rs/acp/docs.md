# Noridoc: codex-acp

Path: @/codex-rs/acp

### Overview

The ACP crate implements the Agent Client Protocol integration for Nori. It manages spawning ACP-compliant agent subprocesses (like Claude Code, Codex, or Gemini), communicating with them over JSON-RPC, and translating between ACP protocol messages and Codex internal protocol types.

### How it fits into the larger codebase

```
nori-tui
    |
    v
codex-acp <---> ACP Agent subprocess (claude-code-acp, codex-acp, gemini-cli)
    |
    v
codex-protocol (internal event types)
```

The ACP crate serves as a bridge between:
- The TUI layer (`@/codex-rs/tui/`) which displays UI and collects user input
- External ACP agent processes installed via npm (@anthropic-ai/claude-code, @openai/codex, @google/gemini-cli)

Key files:
- `registry.rs` - Agent configuration and npm package detection
- `connection.rs` - Subprocess spawning and JSON-RPC communication
- `translator.rs` - Protocol translation between ACP and Codex types
- `backend.rs` - Implements `ConversationClient` trait from codex-core

### Core Implementation

**Model Registry** (`registry.rs`):

The registry is **model-centric** rather than provider-centric:
- `get_agent_config()` accepts model names (e.g., "claude-code", "gemini-2.5-flash") instead of provider names
- Returns `AcpAgentConfig` containing:
  - `provider_slug`: Identifies which agent subprocess to spawn
  - `command`: Executable path or command name
  - `args`: Arguments to pass to the subprocess
  - `env`: Environment variables (used by mock agents for testing)
  - `provider_info`: Retry settings, timeouts
  - `auth_hint`: Agent-specific authentication instructions for error messages

Agent display names and auth hints:

| Agent | Display Name | Auth Hint |
|-------|--------------|-----------|
| Claude Code | "Claude Code" | "Run /login for instructions, or set ANTHROPIC_API_KEY." |
| Codex | "Codex" | "Run /login to authenticate, or set OPENAI_API_KEY." |
| Gemini | "Gemini" | "Run /login for instructions, or set GOOGLE_API_KEY." |

**Nori Config Path Resolution** (`config/`):

The config module provides the **canonical source of truth** for Nori home path resolution:
- `find_nori_home()`: Returns `~/.nori/cli` or `$NORI_HOME` if set
- `NORI_HOME_ENV`: Environment variable name (`"NORI_HOME"`)
- `NORI_HOME_DIR`: Default relative path (`".nori/cli"`)
- `CONFIG_FILE`: Config filename (`"config.toml"`)
- `DEFAULT_MODEL`: Default agent model (`"claude-code"`)

**Agent vs Model Field Distinction:**

| Field | Purpose | Persistence |
|-------|---------|-------------|
| `agent` | User's persistent agent preference | Saved to config.toml |
| `model` | Active model for current session | Can be overridden by CLI flags |

**Notification Configuration** (`config/types.rs`):

Three config enums control notification behavior, all stored in the `[tui]` section of `config.toml`:

| Enum | TOML Key | Default | Controls |
|------|----------|---------|----------|
| `TerminalNotifications` | `terminal_notifications` | `Enabled` | OSC 9 escape sequences sent by the TUI (`chatwidget.rs`) |
| `OsNotifications` | `os_notifications` | `Enabled` | Native desktop notifications via `notify-rust` (wired in `backend.rs` to `UserNotifier::new()`) |
| `NotifyAfterIdle` | `notify_after_idle` | `FiveSeconds` (`"5s"`) | Duration to wait before firing an idle notification; `Disabled` suppresses the timer entirely |

`NotifyAfterIdle` accepts serde-renamed string values: `"5s"`, `"10s"`, `"30s"`, `"60s"`, `"disabled"`. Its `as_duration()` method returns `Option<Duration>` (`None` when `Disabled`). The idle timer in `backend.rs` is conditionally spawned only when `as_duration()` returns `Some` -- when `Disabled`, no timer task or abort handle is created.

The `AcpBackendConfig` struct carries both `os_notifications` and `notify_after_idle` so the backend can configure the `UserNotifier` and the idle timer respectively. Terminal notifications flow separately through `codex-core`'s `Config::tui_notifications` bool to the TUI's `ChatWidget::notify()` method.


**Hotkey Configuration** (`config/types.rs`):

Hotkeys are user-configurable keyboard shortcuts stored under `[tui.hotkeys]` in `config.toml`. The config layer defines four types:

| Type | Purpose |
|------|---------|
| `HotkeyAction` | Enum of bindable actions with display names, descriptions, TOML keys, and default bindings. Covers both app-level actions (OpenTranscript, OpenEditor) and emacs-style editing actions (cursor movement, deletion, kill/yank) used by the textarea |
| `HotkeyBinding` | String-based key representation (e.g. `"ctrl+t"`, `"alt+g"`, `"none"` for unbound). Serializes/deserializes via serde for TOML roundtripping |
| `HotkeyConfigToml` | TOML deserialization struct with `Option<HotkeyBinding>` fields for each action |
| `HotkeyConfig` | Resolved config with defaults applied via `from_toml()`. Provides `binding_for()`, `set_binding()`, and `all_bindings()` accessors |

The binding string format is kept terminal-agnostic (no crossterm dependency in the config crate). The TUI layer in `@/codex-rs/tui/src/nori/hotkey_match.rs` handles conversion between binding strings and crossterm `KeyEvent` types. `HotkeyConfig` is carried on `NoriConfig` and resolved during config loading in `loader.rs`.

**Message History** (`message_history.rs`):

- File location: `~/.nori/cli/history.jsonl`
- Entry schema: `{"session_id":"<uuid>","ts":<unix_seconds>,"text":"<message>"}`
- Uses advisory file locking for concurrent write safety
- `HistoryPersistence` policy: `SaveAll` (default) or `None` (privacy mode)

**Connection Management** (`connection.rs`):

Thread-safe wrapper pattern for `!Send` ACP futures:

```
┌─────────────────────────┐   mpsc channels     ┌─────────────────────────┐
│   Main Tokio Runtime    │◄───────────────────►│  ACP Worker Thread      │
│                         │  AcpCommand enum    │  (single-threaded RT)   │
│   AcpConnection         │                     │                         │
│   - spawn()             │  ────────────────►  │  AcpConnectionInner     │
│   - create_session()    │  CreateSession      │  - ClientDelegate       │
│   - prompt()            │  Prompt             │  - run_command_loop()   │
│   - cancel()            │  Cancel             │  - model_state Arc      │
│   - set_model() [unst]  │  SetModel [unstable]│                         │
└─────────────────────────┘                     └─────────────────────────┘
```

**Subprocess Lifecycle Management:**

Multi-layer cleanup strategy for robust process termination:

1. **Process Group Isolation (Unix)**: Agent spawns in own process group via `setpgid(0, 0)`. Enables killing entire process tree with `killpg()`.

2. **Kernel-Level Parent Death Signal (Linux)**: `PR_SET_PDEATHSIG` set to `SIGTERM`. Guarantees agent receives signal if parent crashes.

3. **IO Task Abort**: Explicit abort before killing child prevents hanging on orphaned file descriptors.

4. **Process Group Kill**: `SIGKILL` to entire process group ensures grandchildren are terminated.

5. **Synchronous Drop Cleanup**: `Drop` waits for completion signal (2-second timeout) before returning.

**File Write Security Boundaries** (`ClientDelegate`):

- Workspace writes: Any path within or under the workspace directory
- Temporary writes: Any path under `/tmp` directory
- System paths: All other paths are rejected
- Path canonicalization prevents symlink-based directory traversal attacks

**Session Transcript Parsing** (`session_parser.rs`):

Parses token usage from agent session files:

| Agent | Path Format |
|-------|-------------|
| Codex | `~/.codex/sessions/<YEAR>/<MM>/<DD>/rollout-<ISODATE>T<HH-MM-SS>-<SESSION_GUID>.jsonl` |
| Gemini | `~/.gemini/tmp/<HASHED_PATHS>/chats/session-<ISODATE>T<HH-MM>-<SESSIONID>.json` |
| Claude | `~/.claude/projects/<PROJECT_PATH>/<SESSIONID>.jsonl` |

**Approval Bridging:**

| Policy | Behavior |
|--------|----------|
| `AskForApproval::UnlessTrusted` | Auto-approve known-safe read-only commands, prompt for all else |
| `AskForApproval::OnFailure` | Auto-approve in sandbox, prompt on failure to escalate |
| `AskForApproval::OnRequest` | (Default) Model decides when to request approval |
| `AskForApproval::Never` | Auto-approve all requests (yolo mode) |

Dynamic policy updates via `tokio::sync::watch` channel enable `/approvals` command to take effect immediately.

**Patch Event Translation:**

For Edit/Write/Delete operations, ACP emits native patch events:

| Operation | Approval Event | Result Event |
|-----------|----------------|--------------|
| Edit (old_string + new_string) | `ApplyPatchApprovalRequest` | `PatchApplyBegin` with `FileChange::Update` |
| Write (content only) | `ApplyPatchApprovalRequest` | `PatchApplyBegin` with `FileChange::Add` |
| Delete | `ApplyPatchApprovalRequest` | `PatchApplyBegin` with `FileChange::Delete` |
| Execute, Read, etc. | `ExecApprovalRequest` | `ExecCommandBegin/End` |

**Tool Call Event Filtering:**

Two-layer filtering prevents duplicate `ExecCommandBegin` events:

1. **Skip Generic Events**: Filter ToolCall events lacking useful display info (empty `raw_input`, generic titles)
2. **Dispatch-Loop Deduplication**: Track `emitted_begin_call_ids` HashSet to skip duplicates

**Tool Classification System:**

| ACP ToolKind | ParsedCommand | TUI Rendering |
|--------------|---------------|---------------|
| `Read` | `ParsedCommand::Read` | Exploring (compact, grouped) |
| `Search` | `ParsedCommand::Search` | Exploring (compact, grouped) |
| `Execute`, `Edit`, `Delete`, etc. | `ParsedCommand::Unknown` | Command (full display) |

**Conversation Compaction:**

Unlike core's direct history manipulation, ACP uses a **prompt-based approach**:
1. `/compact` sends summarization prompt to agent
2. Agent's summary response is captured
3. Summary is prepended to next user message
4. Emits `ContextCompacted` event to TUI

**ACP Error Categorization:**

| Category | Detection Patterns | User Message |
|----------|-------------------|--------------|
| `Authentication` | "auth", "-32000", "api key", "unauthorized" | "Authentication required for {provider}. {auth_hint}" |
| `QuotaExceeded` | "quota", "rate limit", "429", "usage limit" | "Rate limit or quota exceeded for {provider}" |
| `ExecutableNotFound` | "not found", "command not found" | "Could not find the {agent} CLI. Install with: npm install -g {package}" |
| `Initialization` | "initialization", "handshake", "protocol" | "Failed to initialize {provider}" |

### Things to Know

- Agent subprocess communication uses stdin/stdout with JSON-RPC 2.0 framing
- The minimum supported ACP protocol version is V1
- The `unstable` feature gates model switching functionality
- Approval requests are translated to use appropriate UI (exec approval for shell commands, patch approval for file edits)
- A `DRAIN_YIELD_COUNT` of 10 yields allows pending notifications to drain before session cleanup
- Config loading uses Nori-specific paths (`~/.nori/cli/config.toml`) when the `nori-config` feature is enabled in the TUI

**Event Flow Tracing:**

```bash
RUST_LOG=acp_event_flow=debug cargo run
```

The `acp_event_flow` target logs streaming deltas, tool calls, and dispatch loop event counts. Pairs with TUI-side tracing (`tui_event_flow`, `cell_flushing`).

**LocalSet Cooperative Scheduling:**

The `io_task` and `run_command_loop` tasks run cooperatively in a LocalSet. A race condition exists when the agent sends notifications followed immediately by a PromptResponse. The fix adds a yield loop (`yield_now()` × 10) before `unregister_session()` to allow pending notifications to drain.

Created and maintained by Nori.
