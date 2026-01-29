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
- `transcript_discovery.rs` - Discovers transcript files for external agents

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

**Transcript Discovery** (`transcript_discovery.rs`):

Detects the current running transcript file when Nori runs within an external agent environment. Used by the TUI's `SystemInfo` module (see `@/codex-rs/tui/src/system_info.rs`) to display token usage in the footer.

Two discovery entry points are provided:
- `discover_transcript_for_agent()` - Basic discovery using directory/CWD matching (legacy)
- `discover_transcript_for_agent_with_message()` - Preferred entry point that uses first-message matching for Claude Code

Agent detection via environment variables:

| Env Var | Agent |
|---------|-------|
| `CLAUDECODE=1` | Claude Code |
| `CODEX_CLI=1` | Codex |
| `GEMINI_CLI=1` | Gemini |

Transcript file locations and matching strategy:

| Agent | Path Pattern | Matching Strategy |
|-------|--------------|-------------------|
| Claude Code | `~/.claude/projects/<transformed-path>/<uuid>.jsonl` | First-message matching (requires `first_message` parameter) |
| Codex | `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl` | Parse first JSON line for `payload.cwd` field, match against CWD |
| Gemini | `~/.gemini/tmp/<sha256-hash>/chats/<session>.json` | Hash is SHA256 of canonical working directory path |

**Claude Code First-Message Matching:**

Claude Code transcript discovery uses the first user message to accurately identify the correct transcript file. This is necessary because multiple sessions may exist in the same project directory, and picking the most-recently-modified file could return the wrong transcript.

The matching process:
1. Normalize both the search message and file messages by stripping whitespace and truncating to 20 characters
2. Only consider files modified in the last 2 days (`MAX_TRANSCRIPT_AGE_SECS = 172800`)
3. Read up to 10 lines (`MAX_LINES_TO_SEARCH`) or until the first user text entry is found
4. Skip `tool_result` entries (which also have `"type":"user"`)
5. If multiple files match, pick the most recently modified one
6. If no first_message is provided or no match is found, return an error (fail closed rather than return wrong transcript)

The `first_message` flows from the TUI's `ChatWidget::first_prompt_text()` through the system info refresh mechanism to the discovery layer.

**Token Usage Parsing** (`transcript_discovery.rs`):

The `parse_transcript_tokens()` function extracts token usage breakdown from transcript files. Returns a `TranscriptTokenUsage` struct:

```rust
pub struct TranscriptTokenUsage {
    pub input_tokens: i64,    // Total input tokens
    pub output_tokens: i64,   // Total output tokens
    pub cached_tokens: i64,   // Cached input tokens (subset of input_tokens)
}
```

Each agent format requires different parsing:

| Agent | Format | Token Fields |
|-------|--------|--------------|
| Claude Code | JSONL | `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens` in `message.usage` |
| Codex | JSONL | `input_tokens`, `output_tokens`, `cached_input_tokens` from last `token_count` event |
| Gemini | JSON | `input`, `output`, `thoughts`, `cached` from each message's `tokens` object |

**Claude Code Streaming Deduplication:**

Claude Code logs multiple JSONL entries per API request due to streaming (each streaming delta contains the same usage data). The parser deduplicates by tracking seen `requestId` values in a `HashSet<String>`. Entries without a `requestId` are still counted for backward compatibility with older transcript formats.

**Claude Token Field Semantics:**

| Field | Meaning | Counted As |
|-------|---------|------------|
| `input_tokens` | Non-cached input tokens sent | Added to `input_tokens` |
| `cache_creation_input_tokens` | Tokens sent and cached for future use | Added to `input_tokens` |
| `cache_read_input_tokens` | Tokens read from cache (discounted) | Reported as `cached_tokens` |
| `output_tokens` | Output tokens generated | Added to `output_tokens` |

The `TranscriptLocation` struct returned by discovery functions includes:
- `token_breakdown: Option<TranscriptTokenUsage>` - Detailed breakdown for input, output, and cached tokens

Token parsing is synchronous because `SystemInfo::collect_fresh` runs in a background thread.

The data flow is:
```
SystemInfo::collect_for_directory_with_message() (background thread)
    |
    v
discover_transcript_for_agent_with_message(cwd, agent_kind, first_message)
    |
    v
parse_transcript_tokens(path, agent_kind)
    |
    v
TranscriptLocation { ..., token_breakdown }
    |
    v
FooterProps { input_tokens, output_tokens, cached_tokens, context_tokens }
    |
    v
Footer renders "Tokens: 45K in / 78K out (32K cached)"
```
**Connection Management** (`connection.rs`):

### Transcript Persistence

The ACP module provides client-side transcript persistence that captures a full view of conversations (user input + assistant responses) without relying on agent-side storage. This enables viewing previous sessions without replaying agent mechanics.

**Storage Structure:**

Transcripts are stored at `{nori_home}/transcripts/by-project/{project-id}/sessions/{session-id}.jsonl`:

```
~/.nori/cli/
└── transcripts/
    └── by-project/
        └── {project-id}/           # 16-hex-char hash
            ├── project.json        # Project metadata
            └── sessions/
                └── {session-id}.jsonl  # JSONL transcript file
```

**Project Identification:**

Project IDs are derived from the workspace to group sessions by project:
- Git repositories: SHA-256 hash of normalized git remote URL (SSH and HTTPS normalize to same hash)
- Non-git directories: SHA-256 hash of canonicalized path
- Hash is truncated to 16 hex characters for compact directory names

Key exports from `@/codex-rs/acp/src/transcript/project.rs`:
- `compute_project_id()`: Computes project ID for a working directory
- `ProjectId`: Contains id, name, git_remote, git_root, and cwd

**Transcript Schema (JSONL):**

Each line in the transcript file is a JSON object with:
- `ts`: ISO 8601 timestamp
- `v`: Schema version (currently 1)
- `type`: Entry type discriminator

Entry types (from `@/codex-rs/acp/src/transcript/types.rs`):

| Type | Description | Key Fields (JSON) |
|------|-------------|-------------------|
| `session_meta` | First line, session metadata | session_id, project_id, started_at, cwd, agent, cli_version, git |
| `user` | User message | id, content, attachments |
| `assistant` | Complete assistant turn | id, content (blocks), agent |
| `tool_call` | Tool execution start | call_id, name, input |
| `tool_result` | Tool execution result | call_id, output, truncated, exit_code |
| `patch_apply` | File modification result | call_id, operation (edit/write/delete), path, success, error |

**Schema Field Naming:**

The `SessionMetaEntry.agent` and `AssistantEntry.agent` fields identify which ACP agent (e.g., "claude-code", "codex", "gemini") processed the session or message. The field is named `agent` rather than `model` to emphasize that it identifies the agent software, not a specific model variant.
**TranscriptRecorder:**

The `TranscriptRecorder` (in `@/codex-rs/acp/src/transcript/recorder.rs`) handles async, non-blocking writes:

```
┌─────────────────────────┐   mpsc channel   ┌─────────────────────────┐
│   AcpBackend            │─────────────────►│   Writer Task           │
│                         │  TranscriptCmd   │   (background)          │
│   record_user_message() │                  │                         │
│   record_assistant_msg()│                  │   - Writes to JSONL     │
│   flush() / shutdown()  │                  │   - Creates directories │
└─────────────────────────┘                  └─────────────────────────┘
```

Key methods:
- `new()`: Creates recorder, writes session_meta and project.json
- `record_user_message()`: Records user input with optional attachments
- `record_assistant_message()`: Records complete assistant turn with content blocks
- `record_tool_call()` / `record_tool_result()`: Records tool execution
- `record_patch_apply()`: Records file modification operations (edit/write/delete)
- `flush()`: Ensures pending writes are persisted
- `shutdown()`: Flushes and terminates writer task

**TranscriptLoader:**

The `TranscriptLoader` (in `@/codex-rs/acp/src/transcript/loader.rs`) reads transcripts for viewing:

Key methods:
- `list_projects()`: List all projects with transcripts
- `list_sessions()`: List sessions for a specific project
- `find_sessions_for_cwd()`: Find sessions for current working directory
- `load_transcript()`: Load complete transcript with all entries
- `load_session_meta()`: Load just session metadata (for quick listing)

**ACP Integration:**

The `AcpBackend` automatically:
1. Creates a `TranscriptRecorder` on spawn (with graceful fallback if creation fails)
2. Records user messages when `Op::UserInput` is processed
3. Accumulates assistant text during the turn and records when turn completes
4. Records tool events via `record_tool_events_to_transcript()` in the update handler
5. Shuts down recorder on `Op::Shutdown`

**Tool Event Recording Flow:**

Tool calls and patch operations are recorded by `record_tool_events_to_transcript()` in `backend.rs`:

```
ACP SessionUpdate          Transcript Entry
─────────────────────      ──────────────────
ToolCall (non-patch)   →   tool_call entry
ToolCallUpdate         →   tool_result entry (on completion)
  (Completed, non-patch)
ToolCallUpdate         →   patch_apply entry (on completion)
  (Completed, patch)
```

Patch operations (Edit/Write/Delete via `ToolKind`) are recorded separately from generic tool calls because they represent file modifications. The operation type is determined by `ToolKind`:
- `ToolKind::Edit` → `PatchOperationType::Edit`
- `ToolKind::Delete` → `PatchOperationType::Delete`
- Other (including Write) → `PatchOperationType::Write`

Configuration:
- `AcpBackendConfig.cli_version`: CLI version included in session metadata

**Re-exported Types:**

Public exports from `@/codex-rs/acp/src/transcript/mod.rs`:
- `TranscriptRecorder`, `TranscriptLoader`
- `ProjectId`, `ProjectInfo`, `SessionInfo`, `Transcript`
- Entry types: `SessionMetaEntry`, `UserEntry`, `AssistantEntry`, `ToolCallEntry`, `ToolResultEntry`, `PatchApplyEntry`
- `PatchOperationType`: Enum for patch operations (Edit, Write, Delete)
- `ContentBlock` (Text and Thinking variants), `Attachment`, `GitInfo`
- `now_iso8601()`: Utility function returning current time as ISO 8601 string

### Stderr Capture Implementation

- Buffer lines per session for access between reader task and caller
- Bounded at 500 lines with FIFO eviction when full
- Individual lines truncated to 10KB
- Reader task runs until EOF or error, logging warnings via tracing

### File-Based Tracing

The `init_rolling_file_tracing()` function in `@/codex-rs/acp/src/tracing_setup.rs` provides structured file logging:
- Sets global tracing subscriber that writes to rolling daily log files
- Log files are named `nori-acp.YYYY-MM-DD` in the configured log directory
- Filters at DEBUG level (debug builds) or WARN with INFO for codex_tui/acp (release builds)
- RUST_LOG environment variable overrides default log level
- Uses non-blocking file appender for async-safe writes
- Creates log directory automatically if it doesn't exist
- Returns error on re-initialization since global subscriber can only be set once per process
- Guard is intentionally leaked via `std::mem::forget()` to keep non-blocking writer alive for program lifetime
- ANSI colors disabled for clean file output
- Automatically initialized by the CLI (`@/codex-rs/cli/src/main.rs`) at startup, writing to `$NORI_HOME/log/nori-acp.YYYY-MM-DD`

### Core Implementation

**Thread-Safe Connection Wrapper (`connection.rs`):**

The ACP library uses `LocalBoxFuture` which is `!Send`.
The solution is a thread-safe wrapper pattern:

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
- Transcript discovery is synchronous and intended for use in background threads (e.g., the TUI's `SystemInfo` collection thread)
- Claude Code transcript discovery requires the first user message to function correctly; without it, the discovery returns an error

**Event Flow Tracing:**

```bash
RUST_LOG=acp_event_flow=debug cargo run
```

The `acp_event_flow` target logs streaming deltas, tool calls, and dispatch loop event counts. Pairs with TUI-side tracing (`tui_event_flow`, `cell_flushing`).

**LocalSet Cooperative Scheduling:**

The `io_task` and `run_command_loop` tasks run cooperatively in a LocalSet. A race condition exists when the agent sends notifications followed immediately by a PromptResponse. The fix adds a yield loop (`yield_now()` × 10) before `unregister_session()` to allow pending notifications to drain.

Created and maintained by Nori.
