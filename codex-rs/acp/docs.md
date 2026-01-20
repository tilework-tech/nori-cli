# Noridoc: ACP Module

Path: @/codex-rs/acp

## Overview

- Implements Agent Context Protocol (ACP) for Codex to communicate with external AI agent subprocesses
- Uses the official `agent-client-protocol` library with optional `unstable` feature for model switching
- Exports `init_file_tracing()` for file-based structured logging at DEBUG level

### How it fits into the larger codebase

- Designed as a parallel crate to `codex-core`, not tightly integrated
- Uses channel-based streaming pattern (mpsc) consistent with core's `ResponseStream`
- Provides structured error handling via library's typed error responses
- TUI and other clients can access captured stderr for displaying agent diagnostic output
- ACP vs HTTP mode is determined at startup via config, no mid-session switching

### Model Registry

The ACP registry in `@/codex-rs/acp/src/registry.rs` is **model-centric** rather than provider-centric:
- `get_agent_config()` accepts model names (e.g., "mock-model", "mock-model-alt", "gemini-2.5-flash", "claude-acp") instead of provider names
- Returns `AcpAgentConfig` containing:
  - `provider_slug`: Identifies which agent subprocess to spawn (e.g., "mock-acp", "mock-acp-alt", "gemini-acp", "claude-acp")
  - `command`: Executable path or command name
  - `args`: Arguments to pass to the subprocess
  - `env`: Environment variables to pass to the subprocess (used by mock agents for model-specific behavior)
  - `provider_info`: Embedded `AcpProviderInfo` with provider configuration (name, retry settings, timeouts)
  - `auth_hint`: Agent-specific authentication instructions for error messages
- Model names are normalized to lowercase for case-insensitive matching (e.g., "Gemini-2.5-Flash" → "gemini-2.5-flash")
- Uses exact matching only (no prefix matching) - each model must be explicitly registered
- The `provider_slug` field enables subprocess reuse determination when switching models (same slug can reuse, different slug spawns new process)
- `mock-model-alt` uses the same binary as `mock-model` but with provider_slug `mock-acp-alt` for E2E testing agent switching between different configurations
- Claude ACP is registered for both "claude-4.5" and "claude-acp" model names, using `npx @zed-industries/claude-code-acp` command with no arguments

**Agent Display Names:**

`get_agent_display_name()` returns a human-readable display name for any registered agent model:
- Mock agents: "Mock ACP" / "Mock ACP Alt" (debug builds only)
- Production agents: Uses `AgentKind::display_name()` (e.g., "Claude Code", "Gemini", "Codex")
- Fallback: Returns the raw model name if not recognized
- Used by the TUI for the "Connecting to [Agent]" status indicator during slow agent startup

**Agent Authentication Hints:**

Each `AgentKind` provides actionable authentication instructions via `auth_hint()`:

| Agent | Auth Hint |
|-------|-----------|
| Claude Code | "Run /login for instructions, or set ANTHROPIC_API_KEY." |
| Codex | "Run /login to authenticate, or set OPENAI_API_KEY." |
| Gemini | "Run /login for instructions, or set GOOGLE_API_KEY." |

These hints are embedded in `AcpAgentConfig.auth_hint` and displayed in enhanced error messages when authentication fails.

### Agent Picker Metadata

`list_available_agents()` (also in `acp/src/registry.rs`) returns `Vec<AcpAgentInfo>` so the TUI can render the `/agent` picker:
- `model_name`, `display_name`, and `description` describe what to present in the selection view.
- `provider_slug` mirrors the config slug so the UI can explain when different agents reuse the same backend.
- `codex_tui::nori::agent_picker` consumes these entries to build the selection popup shown by `/agent`.
- Selecting an agent raises `AppEvent::SetPendingAgent`, stores a `PendingAgentSelection`, and defers the actual switch until `AppEvent::SubmitWithAgentSwitch` rebuilds the `ChatWidget` with the new model.

### Model Switching (Unstable Feature)

When the `unstable` feature is enabled, ACP supports runtime model switching within a session:

- `AcpModelState` tracks the current model ID and available models from the agent
- State is populated from `NewSessionResponse.models` when a session is created
- `AcpConnection::set_model()` sends `session/set_model` to the ACP agent
- `AcpBackend::set_model()` exposes model switching to the TUI layer

The model state flow:

```
┌─────────────────────────┐     NewSessionResponse.models   ┌─────────────────────────┐
│   AcpConnection         │◄────────────────────────────────│   ACP Agent             │
│                         │                                 │                         │
│   model_state: Arc<     │   set_session_model             │   - session_model_state │
│     RwLock<AcpModel     │────────────────────────────────►│   - available_models    │
│     State>>             │                                 │                         │
└─────────────────────────┘                                 └─────────────────────────┘
         ▲
         │ get_model_state() / set_model()
         │
┌─────────────────────────┐
│   TUI (AcpAgentHandle)  │
│   - /model command      │
│   - OpenAcpModelPicker  │
│   - SetAcpModel event   │
└─────────────────────────┘
```

Re-exported types under `unstable`:
- `SessionModelState`, `ModelInfo`, `ModelId`: Model information from agent
- `SetSessionModelRequest`, `SetSessionModelResponse`: Protocol messages for switching

### Embedded Provider Info

ACP providers embed their configuration directly in `AcpAgentConfig` via `AcpProviderInfo`:
- `codex-core` does not depend on `codex-acp` - they are decoupled crates
- ACP providers are NOT in `built_in_model_providers()` in core - they're self-contained in the registry
- `AcpProviderInfo` contains:
  - `name`: Display name (e.g., "Gemini ACP")
  - `request_max_retries`: Max request retries (default: 1)
  - `stream_max_retries`: Max stream reconnection attempts (default: 1)
  - `stream_idle_timeout`: Idle timeout for streaming (default: 5 minutes)

### Nori Config Path Resolution

The `config` module (`@/codex-rs/acp/src/config/`) provides the **canonical source of truth** for Nori home path resolution:

```
┌─────────────────────────────┐
│  codex_acp::config module   │  <-- Single source of truth
│  - find_nori_home()         │
│  - NORI_HOME_ENV            │
│  - NORI_HOME_DIR            │
└─────────────────────────────┘
            │
            ▼
┌─────────────────────────────┐     ┌─────────────────────────────┐
│  TUI config_adapter.rs      │     │  TUI onboarding module      │
│  - get_nori_home()          │     │  - first_launch.rs          │
│  - setup_nori_config_env()  │     │  - onboarding_screen.rs     │
└─────────────────────────────┘     └─────────────────────────────┘
```

Key exports from `@/codex-rs/acp/src/config/loader.rs`:
- `find_nori_home()`: Returns `~/.nori/cli` or `$NORI_HOME` if set
- `NORI_HOME_ENV`: Environment variable name (`"NORI_HOME"`)
- `NORI_HOME_DIR`: Default relative path (`".nori/cli"`)
- `CONFIG_FILE`: Config filename (`"config.toml"`)

Key exports from `@/codex-rs/acp/src/config/types.rs`:
- `DEFAULT_MODEL`: The default agent model (`"claude-code"`), used when no agent is specified in config
- `NoriConfigToml`: TOML-deserializable config structure with optional fields
- `NoriConfig`: Resolved configuration with defaults applied

**Agent Persistence:**

The `NoriConfig` and `NoriConfigToml` types include an `agent` field that tracks the user's preferred agent separately from the `model` field:

| Field | Purpose | Persistence |
|-------|---------|-------------|
| `agent` | User's agent preference (e.g., "claude-code", "gemini") | Persisted to config.toml via `set_agent()` |
| `model` | Active model for current session | Can be overridden by CLI flags |

The distinction exists because:
- `agent` represents the user's persistent preference across TUI sessions
- `model` may be temporarily overridden by CLI flags without changing the stored preference
- When loading config, `agent` defaults to `DEFAULT_MODEL` ("claude-code") if not specified


Consumers of these paths:
- `@/codex-rs/tui/src/nori/config_adapter.rs`: `get_nori_home()` delegates to `find_nori_home()`
- `@/codex-rs/tui/src/nori/onboarding/`: Uses `get_nori_home()` for first-launch detection and config file creation

Path semantics:
- `nori_home` always refers to `~/.nori/cli` (the full path)
- Config file lives at `{nori_home}/config.toml` (i.e., `~/.nori/cli/config.toml`)

### Message History Support

The ACP module provides cross-session message history persistence, matching the functionality in `codex-core`:

**History File Location:**
- Stored at `{nori_home}/history.jsonl` (i.e., `~/.nori/cli/history.jsonl`)
- Uses JSON-Lines format with one entry per line

**History Entry Schema:**
```json
{"session_id":"<uuid>","ts":<unix_seconds>,"text":"<message>"}
```

**Key exports from `@/codex-rs/acp/src/message_history.rs`:**
- `append_entry()`: Async function to add a history entry with file locking
- `history_metadata()`: Returns (log_id, entry_count) for the history file
- `lookup()`: Retrieves a specific history entry by offset and log_id
- `HistoryEntry`: Struct representing a single history entry

**History Persistence Policy:**

The `HistoryPersistence` enum in `@/codex-rs/acp/src/config/types.rs` controls history behavior:

| Policy | Behavior |
|--------|----------|
| `SaveAll` (default) | All user messages are persisted to history.jsonl |
| `None` | No history is written (privacy mode) |

Configured via `history_persistence` in `~/.nori/cli/config.toml`:
```toml
history_persistence = "save-all"  # or "none"
```

**Implementation Details:**
- Uses advisory file locking for concurrent write safety
- File permissions set to `0o600` on Unix for security
- Appends in background task to avoid blocking the main event loop
- Maximum 10 retries with 100ms backoff for lock acquisition


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

The ACP library uses `LocalBoxFuture` which is `!Send`, preventing direct use in codex-core's multi-threaded tokio runtime. The solution is a thread-safe wrapper pattern:

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
│                         │  ◄────────────────  │                         │
│                         │  oneshot responses  │                         │
└─────────────────────────┘                     └─────────────────────────┘
```

Model state is stored in `Arc<RwLock<AcpModelState>>` shared between the main thread and worker thread for thread-safe access.

- `AcpConnection::spawn()` creates dedicated thread with `LocalSet` for `!Send` futures
- Commands sent via `mpsc::Sender<AcpCommand>` to worker thread
- Responses returned via `oneshot` channels embedded in commands
- Worker thread spawns subprocess, handles JSON-RPC handshake, runs command loop

**Subprocess Lifecycle Management:**

The agent subprocess cleanup follows a deterministic multi-layer shutdown pattern with robust guarantees against orphaned processes:

```
┌─────────────────────────┐   Drop triggered   ┌─────────────────────────────────────────┐
│   AcpConnection::Drop   │───────────────────►│  Worker Thread (run_command_loop)       │
│                         │   (command_tx      │                                          │
│  1. Drop command_tx     │    dropped)        │  - Detects channel closed                │
│  2. Wait on             │                    │  - Abort IO tasks (prevents pipe hangs) │
│     shutdown_complete_rx│                    │  - Kill process group (handles           │
│  3. Join worker thread  │                    │    grandchildren)                        │
│                         │◄───────────────────│  - Kill direct child                     │
│                         │   signal complete  │  - Wait for termination (500ms timeout)  │
│                         │                    │  - Send () on shutdown_complete_tx       │
└─────────────────────────┘                    └──────────────────────────────────────────┘
```

**Multi-Layer Cleanup Strategy:**

The implementation uses several defense layers to ensure robust cleanup:

1. **Process Group Isolation (Unix):**
   - Agent spawns in its own process group via `setpgid(0, 0)` in `pre_exec`
   - Enables killing entire process tree with `killpg(pgid, SIGKILL)`
   - Handles grandchildren spawned by agent (e.g., Python agent using subprocess)

2. **Kernel-Level Parent Death Signal (Linux):**
   - `PR_SET_PDEATHSIG` set to `SIGTERM` during spawn
   - Kernel guarantees agent receives `SIGTERM` if parent crashes (even on SIGKILL)
   - Race condition handling: checks parent PID and self-terminates if parent already died
   - Provides cleanup even when Drop doesn't run (crashes, forced kills)

3. **IO Task Abort:**
   - `io_task` and `stderr_task` explicitly aborted before killing child
   - Prevents hanging on orphaned file descriptors from grandchildren
   - 50ms grace period for tasks to abort cleanly
   - Similar to 2-second IO drain timeout pattern in `exec.rs`

4. **Process Group Kill:**
   - `kill_child_process_group()` sends `SIGKILL` to entire process group
   - Gracefully handles "process not found" errors (ESRCH)
   - Ensures grandchildren are terminated along with direct child
   - Pattern reused from `codex-rs/core/src/exec.rs:720-749`

5. **Synchronous Drop Cleanup:**
   - `Drop` waits for `shutdown_complete_rx` signal (2-second timeout)
   - Worker thread joins before Drop returns
   - Ensures child process is fully terminated before continuing

Key implementation details:
- `run_command_loop()` runs until the command channel is closed (when `AcpConnection` is dropped)
- Cleanup sequence: abort IO tasks → kill process group → kill direct child → wait for exit → signal completion
- `Drop` implementation waits (with `SHUTDOWN_TIMEOUT` of 2 seconds) for cleanup completion before returning
- Uses `Mutex<Option<>>` pattern for `worker_thread` and `shutdown_complete_rx` to allow taking in Drop while satisfying `Sync` requirement for `Arc<AcpConnection>`
- Prevents orphaned/zombie processes when TUI exits (via `/exit`, `/quit`, or Ctrl+C)
- Also handles session switches (e.g., via `/new` command)
- Logs subprocess PID at spawn via `debug!("ACP agent spawned (pid: {:?})")` for E2E test verification

**Platform Support:**
- Process group and PR_SET_PDEATHSIG: Linux only
- Process group isolation: All Unix platforms (Linux, macOS, FreeBSD)
- Basic kill: All platforms (Windows uses tokio's kill implementation)

**Dependencies:**
- `libc` crate required for Unix-specific process control (added as `[target.'cfg(unix)'.dependencies]`)

**Subprocess Environment Variables:**

The `spawn_connection_internal()` function passes environment variables to the subprocess via `.envs(&config.env)`:
- Enables model-specific behavior for mock agents (e.g., `MOCK_AGENT_MODEL_NAME` identifies which mock model variant is running)
- Used by E2E tests to configure model-specific startup delays (`MOCK_AGENT_STARTUP_DELAY_MS_{MODEL}`)
- Production agents typically have an empty `env` map

**ClientDelegate (`connection.rs`):**

- Implements `acp::Client` trait to handle agent requests
- Routes session updates to registered `mpsc::Sender<SessionUpdate>` channels
- Bridges permission requests to Codex approval system via `ApprovalRequest` channel
- Implements file read (synchronous `std::fs::read_to_string`)
- Terminal operations return `method_not_found` (not yet supported)
- Implements file write (`write_text_file`) with relative path resolution and security boundaries

**File Write Implementation:**

The `write_text_file` method implements file creation and modification for ACP agents with security boundaries:

1. **Relative Path Resolution**: Paths like `file.txt` are resolved against the workspace directory (`cwd`) before validation, so agents can use simple relative paths for workspace files

2. **Security Boundaries**: Application-level path restrictions (temporary until OS-level sandboxing is deployed):
   - Workspace writes: Any path within or under the workspace directory
   - Temporary writes: Any path under `/tmp` directory  
   - System paths: All other paths are rejected with an error

3. **Auto-create Directories**: Parent directories are created automatically using `std::fs::create_dir_all` when needed

4. **Atomicity**: Not currently atomic - partial writes can occur if interrupted

The path validation canonicalizes paths to prevent symlink-based directory traversal attacks.

**Session Transcript Parsing (`session_parser.rs`):**

The `session_parser` module provides parsers to extract token usage and metadata from agent session transcript files. Each agent (Claude, Codex, Gemini) runs as an opaque subprocess and stores session data in different formats:

- **Codex**: `~/.codex/sessions/<YEAR>/<MM>/<DD>/rollout-<ISODATE>T<HH-MM-SS>-<SESSION_GUID>.jsonl`
- **Gemini**: `~/.gemini/tmp/<HASHED_PATHS>/chats/session-<ISODATE>T<HH-MM>-<SESSIONID>.json`
- **Claude**: `~/.claude/projects/<PROJECT_PATH>/<SESSIONID>.jsonl`

Key types:
- `TokenUsageReport`: Unified report wrapping `TokenUsage` (from codex-protocol) with agent type, session ID, and transcript path
- `AgentKind`: Enum identifying the agent (Claude, Codex, Gemini)
- `ParseError`: Error variants with semantic distinctions:
  - `IoError`: File cannot be read (e.g., missing file, permission denied). Automatically converted via `#[from]` attribute.
  - `JsonError`: Root-level JSON parse failure (entire file is malformed). Automatically converted via `#[from]` attribute.
  - `EmptyFile`: Either file is empty, OR all JSONL lines failed to parse, OR valid structure exists but no token data found
  - `MissingSessionId`: Valid JSON structure but session ID field not present
  - `TokenOverflow`: Arithmetic overflow during token aggregation

Parser functions:
- `parse_codex_session()`: Parses Codex JSONL with cumulative `token_count` events. Session ID derived from filename since not embedded in content. Extracts `model_context_window` when available.
- `parse_gemini_session()`: Parses Gemini JSON with messages array. Aggregates tokens from each message. Maps `tokens.thoughts` to `reasoning_output_tokens`, `tokens.cached` to `cached_input_tokens`.
- `parse_claude_session()`: Parses Claude JSONL with per-message usage objects (nested in `.message.usage`). Maps `cache_read_input_tokens` to `cached_input_tokens`. No separate reasoning tokens.

Implementation details:
- **Line-by-line JSONL parsing**: Resilient error handling logs warnings and continues on malformed lines. Individual line parse failures do NOT cause the entire parse to fail.
- **Valid line tracking**: Both `parse_codex_session()` and `parse_claude_session()` track `valid_lines` counter (incremented after successful JSON parse). If `valid_lines == 0` after processing all lines, returns `ParseError::EmptyFile` to distinguish "all lines malformed" from "missing token data".
- **Error semantics**: The implementation distinguishes between three failure modes:
  1. **Structural failure** (IoError, JsonError): File is inaccessible or fundamentally malformed
  2. **All-malformed JSONL** (EmptyFile): Every line in JSONL failed to parse (valid_lines == 0)
  3. **Missing data** (EmptyFile, MissingSessionId): Valid JSON but required fields absent
- **Zero tokens vs. no token information**: Zero token count is semantically valid (e.g., session just started). The error case is when token information is completely inaccessible or missing from the transcript structure.
- **Checked arithmetic**: Uses `.checked_add()` for token aggregation to prevent overflow
- **Agent-specific token mapping**: Each parser maps agent-specific token fields to the unified `TokenUsage` struct
- **Codex as external agent**: Treats Codex sessions as external data (like Gemini/Claude), not relying on internal Codex state

Error handling pattern:
```rust
// In parse_codex_session and parse_claude_session:
let mut valid_lines = 0;
for line in text.lines() {
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("failed to parse line: {e}");
            continue; // Skip this line, continue processing
        }
    };
    valid_lines += 1; // Only count successfully parsed lines
    // ... process valid JSON ...
}
if valid_lines == 0 {
    return Err(ParseError::EmptyFile); // All lines failed to parse
}
```

This ensures:
- **Lenient per-line**: One bad line in a JSONL file doesn't invalidate the entire session
- **Strict overall**: If EVERY line is malformed, that's a fundamental problem that should error
- **Semantic accuracy**: `EmptyFile` when all-malformed prevents misleading `MissingSessionId` errors

Session discovery logic (finding files in ~/.codex, ~/.gemini, ~/.claude) is deferred for future TUI integration.


**Approval Bridging:**

The ACP module bridges permission requests to Codex's approval UI via `run_approval_handler()`. The handler respects the configured `approval_policy` from `AcpBackendConfig`:

| Policy | Behavior |
|--------|----------|
| `AskForApproval::Never` | Auto-approve all requests immediately (yolo mode) |
| `AskForApproval::OnFailure` | Prompt only when operations fail |
| `AskForApproval::UnlessAllowListed` | Prompt except for allowed operations |

When `approval_policy == AskForApproval::Never` (set via `--yolo` or `--dangerously-bypass-approvals-and-sandbox` CLI flags), the approval handler sends `ReviewDecision::Approved` without forwarding requests to the TUI. This completes the data flow:

```
CLI --yolo flag → AskForApproval::Never → AcpBackendConfig → run_approval_handler() → auto-approve
```

**Dynamic Approval Policy Updates:**

The approval policy can be changed mid-session via the `/approvals` command (which sends `Op::OverrideTurnContext`). The ACP backend uses a `tokio::sync::watch` channel to broadcast policy changes to the long-running approval handler:

```
┌─────────────────────────┐                        ┌─────────────────────────┐
│   AcpBackend::submit()  │   watch::Sender        │   run_approval_handler  │
│                         │  ─────────────────►    │                         │
│   Op::OverrideTurnContext                        │   watch::Receiver       │
│   { approval_policy }   │                        │   *rx.borrow() on each  │
│                         │                        │   request               │
└─────────────────────────┘                        └─────────────────────────┘
```

- `approval_policy_tx: watch::Sender<AskForApproval>` field in `AcpBackend` broadcasts updates
- `approval_policy_rx: watch::Receiver<AskForApproval>` passed to `run_approval_handler()` at spawn
- Handler reads `*approval_policy_rx.borrow()` on each incoming request to get the current policy
- Pattern enables immediate policy changes without restarting the approval handler task

For all other policies, approval requests are handled **immediately** (not deferred) to avoid deadlocks:

```
┌─────────────────────────┐   ApprovalRequest     ┌─────────────────────────┐
│   ACP Worker Thread     │──────────────────────►│   Main Thread (TUI)     │
│                         │                       │                         │
│   ClientDelegate        │                       │   - Display approval UI │
│   - request_permission()│◄──────────────────────│   - Get user decision   │
│                         │  ReviewDecision       │   - Send via oneshot    │
└─────────────────────────┘  (via oneshot)        └─────────────────────────┘
```

- `ApprovalRequest` bundles the `ApprovalEventType`, original ACP options, and response channel
- `ApprovalEventType` enum selects the appropriate approval UI:
  - `Exec(ExecApprovalRequestEvent)` - for shell commands and generic operations
  - `Patch(ApplyPatchApprovalRequestEvent)` - for file Edit/Write/Delete with diff rendering
- `AcpConnection::take_approval_receiver()` exposes the receiver for TUI consumption
- Falls back to auto-approve if approval channel is closed (no UI listening)
- Falls back to deny if response channel is dropped (UI didn't respond)
- **Critical timing**: The agent subprocess blocks waiting for approval. Deferring approval display would deadlock (agent waits for approval, but TaskComplete never arrives until agent finishes)

**Patch Event Translation:**

For Edit/Write/Delete operations, the ACP backend emits native patch events for better TUI rendering:

| Operation | Approval Event | Result Event |
|-----------|----------------|--------------|
| Edit (old_string + new_string) | `ApplyPatchApprovalRequest` | `PatchApplyBegin` with `FileChange::Update` |
| Write (content only) | `ApplyPatchApprovalRequest` | `PatchApplyBegin` with `FileChange::Add` |
| Delete | `ApplyPatchApprovalRequest` | `PatchApplyBegin` with `FileChange::Delete` |
| Execute, Read, etc. | `ExecApprovalRequest` | `ExecCommandBegin/End` |

The patch event flow requires state tracking since ToolCallUpdate may not have the same fields as ToolCall:

```
┌───────────────────┐      ┌───────────────────────────────┐      ┌───────────────────┐
│  ToolCall         │      │  RequestPermission            │      │  ToolCallUpdate   │
│  (Edit detected)  │      │                               │      │  (Completed)      │
│                   │      │                               │      │                   │
│  Store FileChange │─────►│  ApplyPatchApprovalRequest    │─────►│  Retrieve stored  │
│  in pending map   │      │  (approval overlay shown)     │      │  FileChange, emit │
│  (no event)       │      │                               │      │  PatchApplyBegin  │
└───────────────────┘      └───────────────────────────────┘      └───────────────────┘
```

Key translator functions:
- `is_patch_operation()` - detects Edit/Write/Delete based on ToolKind or raw_input fields
- `tool_call_to_file_change()` - converts raw_input to `FileChange` using `diffy` for unified diffs
- `permission_request_to_patch_approval_event()` - creates `ApplyPatchApprovalRequestEvent` for patch ops

**TUI Backend Adapter (`backend.rs`):**

The `AcpBackend` provides a TUI-compatible interface that wraps `AcpConnection`:

```
┌─────────────────────────┐                      ┌─────────────────────────┐
│   TUI Event Loop        │  Event channel       │   AcpBackend            │
│                         │◄─────────────────────│                         │
│   - spawn_acp_agent()   │  codex_protocol::    │   - spawn()             │
│   - forwards events     │  Event               │   - submit(Op)          │
│                         │                      │   - approval handling   │
│                         │  ─────────────────►  │   - OS notifications    │
│                         │  Op channel          │                         │
└─────────────────────────┘                      └─────────────────────────┘
```

- `AcpBackendConfig`: Configuration for spawning (model, cwd, approval_policy, sandbox_policy, notify, nori_home, history_persistence)
- `AcpBackend::spawn()`: Creates AcpConnection, session, and starts approval handler task. Uses enhanced error handling to provide actionable error messages on spawn or session creation failure.
- `AcpBackend::submit(Op)`: Translates Codex Ops to ACP actions:
  - `Op::UserInput` → ACP `prompt()`
  - `Op::Interrupt` → ACP `cancel()`
  - `Op::ExecApproval`/`PatchApproval` → Resolves pending approval
  - `Op::AddToHistory` → Appends to history file (async background task)
  - `Op::GetHistoryEntryRequest` → Looks up history entry and sends response event
  - `Op::OverrideTurnContext` → Updates approval policy via watch channel (enables `/approvals` command)
  - `Op::Compact` → Sends summarization prompt, stores response for next user input
  - Unsupported ops → Error event sent to TUI
- `AcpBackend::model_state()`: Returns current model state (available models and current selection)
- `AcpBackend::set_model()` [unstable]: Delegates to `AcpConnection::set_model()` for model switching
- `translate_session_update_to_events()`: Converts ACP `SessionUpdate` to `codex_protocol::EventMsg`:
  - `AgentMessageChunk` → `AgentMessageDelta`
  - `AgentThoughtChunk` → `AgentReasoningDelta`
  - `ToolCall` → `ExecCommandBegin` (with filtering, see below)
  - `ToolCallUpdate(Completed)` → `ExecCommandEnd`

### ACP Tool Call Event Filtering

The ACP protocol emits **multiple ToolCall events** for the same `call_id` as details become available during LLM streaming:

```
Event 1 (early): ToolCall { call_id="toolu_123", title="Read File", raw_input={} }
Event 2 (later): ToolCall { call_id="toolu_123", title="Read /home/.../file.rs", raw_input={path: "..."} }
```

Without filtering, duplicate events would cause ExecCells to disappear briefly and reappear at the end of agent turns. The fix uses two layers of filtering:

**Layer 1 - Skip Generic Events (`translate_session_update_to_events`):**
- Skip ToolCall events that lack useful display information
- Check both `raw_input` (for path/command/pattern fields) and title (for embedded paths or commands)
- `title_contains_useful_info()` detects paths in titles (`" /"`, backticks, long non-generic titles)
- `extract_display_args()` extracts display-friendly arguments based on tool type

**Layer 2 - Dispatch-Loop Deduplication:**
- The update handler tracks `emitted_begin_call_ids: HashSet<String>`
- Skips any `ExecCommandBegin` with a call_id that was already emitted
- Safety net for edge cases that slip through Layer 1

### Tool Classification System

The `classify_tool_to_parsed_command()` function maps ACP `ToolKind` to TUI rendering modes:

| ACP ToolKind | ParsedCommand | TUI Rendering |
|--------------|---------------|---------------|
| `Read` | `ParsedCommand::Read` | Exploring (compact, grouped) |
| `Search` | `ParsedCommand::Search` | Exploring (compact, grouped) |
| `Other` + title heuristics | `ListFiles`, `Search`, `Read` | Exploring (title-based fallback) |
| `Execute`, `Edit`, `Delete`, `Move`, `Fetch`, `Think` | `ParsedCommand::Unknown` | Command (full display) |

Title-based fallback uses `classify_tool_by_title()` when `ToolKind::Other` or `None`:
- Titles containing "list", "glob", "ls", "find files" → `ListFiles`
- Titles containing "search", "grep" → `Search`
- Titles containing "read" or exactly "file" → `Read`
- Everything else → `Unknown` (command mode)

This enables the TUI to group and collapse read-only operations ("Explored 3 files") while showing mutating operations prominently

**Event Translation (`translator.rs`):**

Bridges between ACP types and codex-protocol types:

| Function | Purpose |
|----------|---------|
| `response_items_to_content_blocks()` | Converts codex `ResponseItem` to ACP `ContentBlock` for prompts |
| `text_to_content_block()` | Simple text-to-ContentBlock conversion |
| `translate_session_update()` | Translates ACP `SessionUpdate` to `TranslatedEvent` enum |
| `permission_request_to_approval_event()` | Converts ACP `RequestPermissionRequest` to Codex `ExecApprovalRequestEvent` |
| `review_decision_to_permission_outcome()` | Converts Codex `ReviewDecision` back to ACP `RequestPermissionOutcome` |

`TranslatedEvent` variants:
- `TextDelta(String)` - Text content from `AgentMessageChunk` or `AgentThoughtChunk`
- `Completed(StopReason)` - Session completion signal

Non-text content (images, audio, resources) and tool calls are currently dropped with empty vec.

**Approval Request Formatting (`translator.rs`):**

When ACP agents request permission for tool operations, the translator converts raw JSON tool call data into human-readable approval requests using git-style formatting:

| Tool Kind | Format Example |
|-----------|----------------|
| Edit | `Edit main.rs (+6 -5)` |
| Write (new file) | `Write config.toml (23 lines)` |
| Execute | `Execute: cargo build --release` |
| Delete | `Delete temp.txt` |
| Move | `Move old.rs → new.rs` |
| Generic | `ToolName(argument)` |

The formatting pipeline:
1. `extract_command_from_tool_call()` dispatches to format functions based on `ToolKind`
2. `extract_reason_from_tool_call()` generates the descriptive reason shown in the approval prompt
3. Helper functions extract and format data from the `raw_input` JSON:
   - `extract_file_path()` - finds path from `file_path`, `path`, or `file` fields
   - `shorten_path()` - extracts just the filename for compact display
   - `calculate_diff_stats()` - computes +added/-removed using set difference on line splits
   - `truncate_str()` - truncates long strings with "..."

Write vs Edit detection uses field presence since ACP lacks a distinct Write variant:
- `old_string` field present → Edit operation (string replacement)
- `content` field present → Write operation (new file creation)

**Approval Translation Details:**

The approval translation maps between Codex's binary approve/deny model and ACP's option-based model:

- `Approved`/`ApprovedForSession` → Finds option with `AllowOnce` or `AllowAlways` kind
- `Denied`/`Abort` → Finds option with `RejectOnce` or `RejectAlways` kind
- Falls back to text matching ("allow", "approve", "yes" vs "deny", "reject", "no") if kind-based matching fails
- Last resort: first option for approve, last option for deny

### OS-Level Notifications

The ACP backend supports OS-level notifications using `codex_core::UserNotifier`. This enables alerting users when the terminal is not focused. Notifications are delivered via native desktop notifications (using `notify-rust`) or an external script if configured.

**Configuration:**
- `AcpBackendConfig.notify`: Optional `Vec<String>` specifying an external notifier command and args
- If no external command is configured, native desktop notifications are used
- The TUI passes `config.notify` from the main Config to `AcpBackendConfig`

**Notification Types:**

| Event | Title | Body Content |
|-------|-------|--------------|
| `AwaitingApproval` | "Nori: Approval Required" | Command (truncated) and cwd |
| `Idle` | "Nori: Session Idle" | Idle duration in seconds |

**Implementation Details:**

- Native notifications display human-readable titles and bodies (see `UserNotification::title()` and `body()` in `@/codex-rs/core`)
- Notifications are fire-and-forget (does not block on delivery)
- Idle timer uses `tokio::task::AbortHandle` for cancellation
- Timer is cancelled when `submit()` is called (new user activity)
- Approval handler sends notification before queuing the approval request
- On X11 Linux, clicking a native notification focuses the terminal window

```
┌─────────────────────┐   AwaitingApproval    ┌─────────────────────┐
│  ApprovalRequest    │──────────────────────►│  UserNotifier       │
│  arrives            │                       │  (desktop notif or  │
└─────────────────────┘                       │   external script)  │
                                              └─────────────────────┘

┌─────────────────────┐   5 sec timer         ┌─────────────────────┐
│  TaskComplete       │──────────────────────►│  Idle notification  │
│  event              │  (if no new input)    │  (if not cancelled) │
└─────────────────────┘                       └─────────────────────┘
```

### Conversation Compaction

The ACP backend supports the `/compact` command to summarize conversation history and reduce token usage. Unlike the core backend which has direct access to conversation history, ACP implements compaction using a **prompt-based approach**:

```
┌─────────────────────┐   SUMMARIZATION_PROMPT   ┌─────────────────────┐
│   /compact command  │─────────────────────────►│   ACP Agent         │
│   (Op::Compact)     │                          │   (subprocess)      │
│                     │◄─────────────────────────│                     │
│   Store summary in  │   Agent's summary        │   Generates summary │
│   pending_compact   │   response               │   from its context  │
└─────────────────────┘                          └─────────────────────┘
            │
            │ Next Op::UserInput
            ▼
┌─────────────────────┐
│   "{SUMMARY_PREFIX} │
│    {summary}        │
│                     │
│    {user_prompt}"   │
└─────────────────────┘
```

**Implementation Details:**

- `handle_compact()` sends `codex_core::compact::SUMMARIZATION_PROMPT` to the agent
- Agent's text response is captured and stored in `pending_compact_summary: Arc<Mutex<Option<String>>>`
- On the next `Op::UserInput`, `handle_user_input()` checks for a pending summary
- If present, prepends `SUMMARY_PREFIX` + summary to the user's prompt
- Emits `ContextCompacted` event to notify the TUI of successful compaction
- Emits `Warning` event to alert users about accuracy degradation in long conversations

**Event Sequence:**

| Step | Event | Purpose |
|------|-------|---------|
| 1 | `TaskStarted` | Indicates compact operation has begun |
| 2 | `AgentMessageDelta` | Streams agent's summary response (displayed in TUI) |
| 3 | `ContextCompacted` | Signals successful compaction |
| 4 | `Warning` | Advises starting new conversations when possible |
| 5 | `TaskComplete` | Ends the compact turn |

**Key Difference from Core Backend:**

The core backend (`@/codex-rs/core/src/compact.rs`) directly accesses and manipulates conversation history. The ACP backend cannot access the agent's internal conversation state, so it:
1. Asks the agent to summarize via a prompt
2. Captures the response
3. Injects the summary into the next user message

Both backends use the same `SUMMARIZATION_PROMPT` and `SUMMARY_PREFIX` constants from `@/codex-rs/core/src/compact.rs` for consistency.


### Things to Know

**ACP Error Categorization:**

The `AcpBackend::spawn()` method provides actionable error messages when agent initialization fails. Error categorization uses pattern matching on the full error chain (via `format!("{e:?}")` debug format) to catch nested error messages:

| Category | Detection Patterns | User Message |
|----------|-------------------|--------------|
| `Authentication` | "auth", "-32000" (JSON-RPC code), "api key", "unauthorized", "not logged in" | "Authentication required for {provider}. {auth_hint}" |
| `QuotaExceeded` | "quota", "rate limit", "too many requests", "429" | "Rate limit or quota exceeded. Please wait and try again." |
| `ExecutableNotFound` | "not found", "no such file", "command not found" | "Could not find the {agent} CLI. Please install with: npm install -g {package}" |
| `Initialization` | "initialization", "handshake", "protocol" | "Failed to initialize {provider}. Original error: {err}" |
| `Unknown` | (fallback) | Original error message passed through |

Key implementation details:
- Uses `format!("{e:?}")` (debug format) to inspect the full anyhow error chain, not just top-level message
- Uses `format!("{e}")` (display format) for user-facing error text
- Agent-specific auth hints come from `AgentKind::auth_hint()` via `AcpAgentConfig.auth_hint`
- Installation instructions use `AgentKind::npm_package()` and `AgentKind::display_name()`

**ACP Prompt Failure Error Propagation:**

When `connection.prompt()` fails at runtime (after successful spawn), the error is propagated to the TUI via `ErrorEvent`:

```
┌────────────────────┐   prompt() fails    ┌────────────────────┐
│  AcpBackend        │─────────────────────│  ACP Connection    │
│  (on_submit task)  │                     │                    │
└────────────────────┘                     └────────────────────┘
         │
         │ categorize_acp_error()
         ▼
┌────────────────────┐   ErrorEvent        ┌────────────────────┐
│  Generate user     │────────────────────►│  TUI               │
│  message           │                     │  (displays error)  │
└────────────────────┘   TaskComplete      └────────────────────┘
```

The error handling flow in `AcpBackend::on_submit()`:
1. Prompt fails with error (e.g., auth failure, rate limit)
2. Error categorized using `categorize_acp_error()` (same as spawn-time errors)
3. User-friendly message generated based on category
4. `ErrorEvent` sent to TUI **before** `TaskComplete`
5. `TaskComplete` always sent to end the turn

This ensures prompt failures are visible to users rather than appearing as silent failures where the "Working" indicator disappears with no response.

**Event Flow Tracing:**

The ACP backend provides detailed tracing for debugging tool event flow issues:

```bash
RUST_LOG=acp_event_flow=debug cargo run
```

The `acp_event_flow` target logs:
- Streaming text and reasoning deltas with content previews
- ToolCall events (skipped generic events, emitted events with parsed_cmd info)
- ToolCallUpdate completion events with output extraction
- Dispatch loop event counts and duplicate detection

This pairs with TUI-side tracing targets (`tui_event_flow`, `cell_flushing`, `pending_exec_cells`) for full event lifecycle debugging.

**Protocol Version Check:**

- Minimum supported version is `acp::V1`
- Checked during initialization handshake
- Connection fails if agent reports older version

**Stderr Handling:**

- Agent stderr is captured via `spawn_local` task in `spawn_connection_internal()`
- Lines are logged via `tracing::warn!` with "ACP agent stderr:" prefix
- Task runs until EOF or error

**Session Update Routing:**

- `ClientDelegate` maintains `HashMap<SessionId, Sender<SessionUpdate>>`
- Updates for unregistered sessions are silently dropped
- Uses `try_send()` (non-blocking) - full/closed channels cause update loss

**LocalSet Cooperative Scheduling and Message Draining:**

The ACP connection uses a single-threaded `LocalSet` runtime because `agent-client-protocol` types are `!Send`. Within this LocalSet, two tasks run cooperatively:
- `io_task`: Reads from agent subprocess stdout and dispatches notifications via `session_notification()`
- `run_command_loop`: Processes commands from the main thread (Prompt, Cancel, etc.)

Both tasks only yield control at `.await` points. A race condition exists when the agent sends notifications followed immediately by a PromptResponse:

```
┌─────────────────────┐   notifications   ┌─────────────────────┐
│   Agent subprocess  │──────────────────►│   io_task           │
│                     │   PromptResponse  │   (buffered)        │
│                     │──────────────────►│                     │
└─────────────────────┘                   └─────────────────────┘
                                                    │
                                                    │ try_send() to session channel
                                                    ▼
┌─────────────────────┐   prompt_future   ┌─────────────────────┐
│   run_command_loop  │◄──────────────────│   resolves          │
│                     │   (no yield)      │                     │
│   unregister_session│                   │   notifications     │
│   (too early!)      │                   │   still pending     │
└─────────────────────┘                   └─────────────────────┘
```

Without explicit yields, `run_command_loop` would call `unregister_session()` immediately after `prompt_future` resolves, removing the session from the map before `io_task` can forward late-arriving notifications. This causes message loss where text appears to "drain" only when the user types the next prompt.

The fix adds a yield loop before `unregister_session()`:
```rust
for _ in 0..10 {
    tokio::task::yield_now().await;
}
```

Each `yield_now()` gives the `io_task` exactly one opportunity to process a buffered message. Ten yields provides sufficient headroom for any realistic burst of trailing notifications.

**Agent Initialization:**

Client advertises these capabilities to agents:
- `fs.read_text_file: true`
- `fs.write_text_file: true`
- `terminal: false`

### Future Work

The following features are marked with TODO comments in the codebase:

**Resume/Fork Integration (connection.rs:343-350):**
- Accept optional session_id parameter to resume existing sessions
- Load persisted history from Codex rollout format
- Send history to agent via session initialization

**Codex-format History Persistence (connection.rs:385-394):**
- Collect all SessionUpdates during prompts
- Convert to Codex ResponseItem format using translator functions
- Write to rollout storage for session resume and history browsing

**History Export for Handoff (connection.rs:220-234):**
- Export session history in Codex format
- Enable switching from ACP mode to HTTP mode mid-session
- Support replaying history through different backends

Created and maintained by Nori.
