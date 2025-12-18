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
  - `provider_info`: Embedded `AcpProviderInfo` with provider configuration (name, retry settings, timeouts)
- Model names are normalized to lowercase for case-insensitive matching (e.g., "Gemini-2.5-Flash" → "gemini-2.5-flash")
- Uses exact matching only (no prefix matching) - each model must be explicitly registered
- The `provider_slug` field enables subprocess reuse determination when switching models (same slug can reuse, different slug spawns new process)
- `mock-model-alt` uses the same binary as `mock-model` but with provider_slug `mock-acp-alt` for E2E testing agent switching between different configurations
- Claude ACP is registered for both "claude-4.5" and "claude-acp" model names, using `npx @zed-industries/claude-code-acp` command with no arguments

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

Consumers of these paths:
- `@/codex-rs/tui/src/nori/config_adapter.rs`: `get_nori_home()` delegates to `find_nori_home()`
- `@/codex-rs/tui/src/nori/onboarding/`: Uses `get_nori_home()` for first-launch detection and config file creation

Path semantics:
- `nori_home` always refers to `~/.nori/cli` (the full path)
- Config file lives at `{nori_home}/config.toml` (i.e., `~/.nori/cli/config.toml`)


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

The `run_command_loop()` function manages agent subprocess cleanup:
- Runs until the command channel is closed (when `AcpConnection` is dropped)
- On exit, calls `child.kill()` to terminate the subprocess
- This prevents orphaned/zombie processes when sessions are switched (e.g., via `/new` command)
- Logs subprocess PID at spawn via `debug!("ACP agent spawned (pid: {:?})")` for E2E test verification

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


**Approval Bridging:**

The ACP module bridges permission requests to Codex's approval UI. Approval requests are handled **immediately** (not deferred) to avoid deadlocks:

```
┌─────────────────────────┐   ApprovalRequest     ┌─────────────────────────┐
│   ACP Worker Thread     │──────────────────────►│   Main Thread (TUI)     │
│                         │                       │                         │
│   ClientDelegate        │                       │   - Display approval UI │
│   - request_permission()│◄──────────────────────│   - Get user decision   │
│                         │  ReviewDecision       │   - Send via oneshot    │
└─────────────────────────┘  (via oneshot)        └─────────────────────────┘
```

- `ApprovalRequest` bundles the translated `ExecApprovalRequestEvent`, original ACP options, and response channel
- `AcpConnection::take_approval_receiver()` exposes the receiver for TUI consumption
- Falls back to auto-approve if approval channel is closed (no UI listening)
- Falls back to deny if response channel is dropped (UI didn't respond)
- **Critical timing**: The agent subprocess blocks waiting for approval. Deferring approval display would deadlock (agent waits for approval, but TaskComplete never arrives until agent finishes)

**TUI Backend Adapter (`backend.rs`):**

The `AcpBackend` provides a TUI-compatible interface that wraps `AcpConnection`:

```
┌─────────────────────────┐                      ┌─────────────────────────┐
│   TUI Event Loop        │  Event channel       │   AcpBackend            │
│                         │◄─────────────────────│                         │
│   - spawn_acp_agent()   │  codex_protocol::    │   - spawn()             │
│   - forwards events     │  Event               │   - submit(Op)          │
│                         │                      │   - approval handling   │
│                         │  ─────────────────►  │                         │
│                         │  Op channel          │                         │
└─────────────────────────┘                      └─────────────────────────┘
```

- `AcpBackendConfig`: Configuration for spawning (model, cwd, approval_policy, sandbox_policy)
- `AcpBackend::spawn()`: Creates AcpConnection, session, and starts approval handler task
- `AcpBackend::submit(Op)`: Translates Codex Ops to ACP actions:
  - `Op::UserInput` → ACP `prompt()`
  - `Op::Interrupt` → ACP `cancel()`
  - `Op::ExecApproval`/`PatchApproval` → Resolves pending approval
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

**Approval Translation Details:**

The approval translation maps between Codex's binary approve/deny model and ACP's option-based model:

- `Approved`/`ApprovedForSession` → Finds option with `AllowOnce` or `AllowAlways` kind
- `Denied`/`Abort` → Finds option with `RejectOnce` or `RejectAlways` kind
- Falls back to text matching ("allow", "approve", "yes" vs "deny", "reject", "no") if kind-based matching fails
- Last resort: first option for approve, last option for deny

### Things to Know

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
