# Noridoc: ACP Module

Path: @/codex-rs/acp

## Overview

- Implements Agent Context Protocol (ACP) for Codex to communicate with external AI agent subprocesses
- Uses the official `agent-client-protocol` v0.7 library instead of any custom JSON-RPC implementation
- Exports `init_file_tracing()` for file-based structured logging at DEBUG level

### How it fits into the larger codebase

- Used by `@/codex-rs/core/src/client.rs` to communicate with ACP-compliant agents via `WireApi::Acp` variant
- Uses channel-based streaming pattern (mpsc) consistent with core's `ResponseStream`
- Provides structured error handling via library's typed error responses that core translates to user-facing messages
- TUI and other clients can access captured stderr for displaying agent diagnostic output

### Model Registry

The ACP registry in `@/codex-rs/acp/src/registry.rs` is **model-centric** rather than provider-centric:
- `get_agent_config()` accepts model names (e.g., "mock-model", "gemini-2.5-flash", "claude-acp") instead of provider names
- Called from `@/codex-rs/core/src/client.rs` at the start of `stream()` to check if model is an ACP agent
- Returns `AcpAgentConfig` containing:
  - `provider_slug`: Identifies which agent subprocess to spawn (e.g., "mock-acp", "gemini-acp", "claude-acp")
  - `command`: Executable path or command name
  - `args`: Arguments to pass to the subprocess
  - `provider_info`: Embedded `AcpProviderInfo` with provider configuration (name, retry settings, timeouts)
- Model names are normalized to lowercase for case-insensitive matching (e.g., "Gemini-2.5-Flash" → "gemini-2.5-flash")
- Uses exact matching only (no prefix matching) - each model must be explicitly registered
- The `provider_slug` field enables future optimization to determine when existing subprocess can be reused vs when new one must be spawned when switching models
- Claude ACP is registered for both "claude" and "claude-acp" model names, using `npx @zed-industries/claude-code-acp` command with no arguments
- Unit test `test_get_claude_model_config()` verifies Claude ACP registry configuration

### Embedded Provider Info

ACP providers embed their configuration directly in `AcpAgentConfig` via `AcpProviderInfo`:
- Avoids circular dependency between `codex-acp` and `codex-core` (core depends on acp, not vice versa)
- ACP providers are NOT in `built_in_model_providers()` in core - they're self-contained in the registry
- `AcpProviderInfo` contains:
  - `name`: Display name (e.g., "Gemini ACP")
  - `request_max_retries`: Max request retries (default: 1)
  - `stream_max_retries`: Max stream reconnection attempts (default: 1)
  - `stream_idle_timeout`: Idle timeout for streaming (default: 5 minutes)
- Core's `client.rs` checks the ACP registry first in `stream()`, using the embedded provider info for ACP models


### Stderr Capture Implementation

- Buffer lines per session for access between reader task and caller
- Bounded at 500 lines with FIFO eviction when full
- Individual lines truncated to 10KB
- Reader task runs until EOF or error, logging warnings via tracing

### File-Based Tracing

The `init_file_tracing()` function in `@/codex-rs/acp/src/tracing_setup.rs` provides structured file logging:
- Sets global tracing subscriber that writes to a user-specified file path
- Filters at DEBUG level and above (TRACE is excluded)
- Uses non-blocking file appender for async-safe writes
- Creates parent directories automatically if they don't exist
- Returns error on re-initialization since global subscriber can only be set once per process
- Guard is intentionally leaked via `std::mem::forget()` to keep non-blocking writer alive for program lifetime
- ANSI colors disabled for clean file output
- Automatically initialized by the CLI (`@/codex-rs/cli/src/main.rs`) at startup, writing to `.codex-acp.log` in the current working directory

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
│   - cancel()            │  Cancel             │                         │
│                         │  ◄────────────────  │                         │
│                         │  oneshot responses  │                         │
└─────────────────────────┘                     └─────────────────────────┘
```

- `AcpConnection::spawn()` creates dedicated thread with `LocalSet` for `!Send` futures
- Commands sent via `mpsc::Sender<AcpCommand>` to worker thread
- Responses returned via `oneshot` channels embedded in commands
- Worker thread spawns subprocess, handles JSON-RPC handshake, runs command loop

**ClientDelegate (`connection.rs`):**

- Implements `acp::Client` trait to handle agent requests
- Routes session updates to registered `mpsc::Sender<SessionUpdate>` channels
- Bridges permission requests to Codex approval system via `ApprovalRequest` channel
- Implements file read (synchronous `std::fs::read_to_string`)
- Terminal operations return `method_not_found` (not yet supported)

**Approval Bridging:**

The ACP module bridges permission requests to Codex's approval UI:

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
