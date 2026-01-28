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

**Message History** (`message_history.rs`):

- File location: `~/.nori/cli/history.jsonl`
- Entry schema: `{"session_id":"<uuid>","ts":<unix_seconds>,"text":"<message>"}`
- Uses advisory file locking for concurrent write safety
- `HistoryPersistence` policy: `SaveAll` (default) or `None` (privacy mode)

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

The Rust struct fields `SessionMetaEntry.model` and `AssistantEntry.model` serialize to `"agent"` in JSON via `#[serde(rename = "agent")]`. This naming reflects that the field identifies which agent processed the session/message rather than a specific model variant.
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
- `ContentBlock` (Text variant only), `Attachment`, `GitInfo`
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

**Event Flow Tracing:**

```bash
RUST_LOG=acp_event_flow=debug cargo run
```

The `acp_event_flow` target logs streaming deltas, tool calls, and dispatch loop event counts. Pairs with TUI-side tracing (`tui_event_flow`, `cell_flushing`).

**LocalSet Cooperative Scheduling:**

The `io_task` and `run_command_loop` tasks run cooperatively in a LocalSet. A race condition exists when the agent sends notifications followed immediately by a PromptResponse. The fix adds a yield loop (`yield_now()` × 10) before `unregister_session()` to allow pending notifications to drain.

Created and maintained by Nori.
