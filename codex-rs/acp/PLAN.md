# ACP Integration Implementation Plan: Parallel acp-core

**Goal:** Integrate ACP (Agent Client Protocol) agents into the Codex TUI as an
alternative backend to HTTP-based LLM providers, with zero changes to codex-core.

**Summary:** The Agent Client Protocol (ACP) is a JSON-RPC 2.0 protocol for
communicating with AI agent subprocesses over stdin/stdout. Instead of Codex
making HTTP calls to LLM APIs (OpenAI, Anthropic, etc.), ACP spawns a local
agent subprocess that handles its own HTTP communication, tool execution, and
conversation history. The `codex_acp` crate acts as the Codex client to these
agents, translating between ACP's event model and Codex's protocol types. This
"Parallel acp-core" approach keeps the ACP integration completely separate from
codex-core, enabling trivial upstream merges while retaining approval bridging
functionality.

**Constraints:**
- **Zero changes to codex-core** - All ACP logic lives in `acp/` crate to avoid upstream merge conflicts
- **Minimal TUI changes** - Only `tui/src/chatwidget/agent.rs` is modified; all other TUI files remain unchanged
- **Agent owns execution** - Tools, sandboxing, and command execution are delegated to the agent subprocess
- **Agent owns history** - Conversation history is managed by the agent; no local persistence in Codex
- **No LLM HTTP calls** - The `ModelClient`, `ToolRouter`, `ToolOrchestrator`, and `ContextManager` are bypassed entirely
- **Separate registries** - ACP agents use `acp/src/registry.rs`, HTTP providers use `core/src/model_provider_info.rs`

**Architecture:**
- `codex_acp` crate - Subprocess management, JSON-RPC I/O, type translation, backend adapter
- `acp/src/backend.rs` - Backend adapter that mimics `CodexConversation` interface for TUI compatibility
- `agent-client-protocol` library - External crate providing ACP types and traits
- `codex_protocol` crate - Shared event types used by both HTTP and ACP flows
- Dedicated worker thread - ACP uses `LocalBoxFuture` (!Send), requiring a single-threaded runtime

---

## Testing Plan

### Integration Tests

1. **ACP Connection Lifecycle Test** (`acp/src/connection.rs`)
   - Spawn mock agent, verify capabilities returned
   - Create session, verify session ID returned
   - Send prompt, verify SessionUpdates received
   - Cancel session, verify cancellation succeeds

2. **Approval Bridging Test** (`acp/src/translator.rs`)
   - Translate ACP `RequestPermissionRequest` → Codex `ExecApprovalRequestEvent`
   - Translate Codex `ReviewDecision::Approved` → ACP `RequestPermissionOutcome::Selected` with "allow" option
   - Translate Codex `ReviewDecision::Denied` → ACP `RequestPermissionOutcome::Selected` with "reject" option

3. **Event Translation Test** (`acp/src/translator.rs`)
   - Translate `AgentMessageChunk` → `codex_protocol::Event` with `AgentMessageDelta`
   - Translate `AgentThoughtChunk` → `codex_protocol::Event` with `AgentReasoningDelta`
   - Verify `ToolCall`, `ToolCallUpdate`, `Plan` produce appropriate events

### E2E Tests

4. **TUI ACP Mode Startup** (`tui-pty-e2e/tests/acp_mode.rs`)
   - Launch TUI with `--model mock-model`
   - Verify TUI starts in ACP mode
   - Verify mock agent subprocess is spawned
   - Send input, verify response appears in chat

5. **ACP Approval Flow** (`tui-pty-e2e/tests/acp_mode.rs`)
   - Launch TUI with mock agent that requests permission
   - Verify approval popup appears
   - Send approval keystroke
   - Verify agent receives approval and continues

6. **ACP Tool Calls Display** (`tui-pty-e2e/tests/acp_tool_calls.rs`)
   - Launch TUI with agent that makes tool calls
   - Verify tool call is displayed in the TUI
   - Verify tool result is displayed

NOTE: I will write all tests before I add any implementation behavior.

---

## Part 1: ACP Crate Structure

The `acp/` crate is already structured correctly. This section documents the existing architecture.

### File Inventory

| File | Purpose | Status |
|------|---------|--------|
| `acp/src/lib.rs` | Module exports and re-exports | ✅ Complete |
| `acp/src/registry.rs` | Agent configuration lookup by model name | ✅ Complete |
| `acp/src/connection.rs` | Subprocess spawning, JSON-RPC I/O, worker thread | ✅ Complete |
| `acp/src/translator.rs` | ACP ↔ Codex type conversion | ✅ Complete |
| `acp/src/tracing_setup.rs` | File-based logging for ACP operations | ✅ Complete |
| `acp/src/backend.rs` | Backend adapter mimicking CodexConversation interface | 🚧 New |

### Key Types

```
AcpBackend           - Backend adapter providing CodexConversation-compatible interface
AcpConnection        - Thread-safe wrapper around agent subprocess
AcpAgentConfig       - Command/args to spawn agent (from registry)
AcpProviderInfo      - Retry settings, timeouts (mirrors ModelProviderInfo)
```

---

## Part 2: TUI Integration (Minimal Changes)

The key insight is that the TUI already has a clean abstraction boundary: `spawn_agent()` returns
`UnboundedSender<Op>`, and all events flow back via `AppEvent::CodexEvent(Event)`. By creating
a backend adapter in the ACP crate that implements this same interface, we minimize TUI changes
to a single file.

### 2.1 Backend Adapter (New File: `acp/src/backend.rs`)

Create an adapter that provides the same interface pattern as `CodexConversation`:

**Interface:**
```rust
pub struct AcpBackend {
    connection: AcpConnection,
    session_id: SessionId,
    event_tx: mpsc::Sender<Event>,
}

impl AcpBackend {
    /// Spawn ACP connection, create session, and return adapter
    pub async fn spawn(
        config: &Config,
        event_tx: mpsc::Sender<Event>,
    ) -> Result<Self>;

    /// Submit an operation (mirrors CodexConversation::submit)
    pub async fn submit(&self, op: Op) -> Result<String>;

    /// Internal: translates ACP SessionUpdates to codex_protocol::Event
    /// and sends them via event_tx
}
```

**Internal Flow:**
1. Spawn `AcpConnection` using model name from config
2. Create session on startup
3. Send synthetic `SessionConfigured` event for TUI initialization
4. Spawn internal task that:
   - Receives ACP `SessionUpdate` from connection
   - Translates to `codex_protocol::protocol::Event` variants (not `TranslatedEvent`)
   - Sends via `event_tx` as `AppEvent::CodexEvent`
5. When approval request received from ACP:
   - Translate to `Event { msg: EventMsg::ExecApprovalRequest(...) }`
   - Send via `event_tx`
   - Store pending approval state
6. When `Op::ReviewExecApproval` received:
   - Translate to ACP `RequestPermissionOutcome`
   - Respond to stored pending approval

**Op Translation:**
| Codex Op | ACP Action |
|----------|------------|
| `Op::UserInput { items }` | Extract text, call `connection.prompt()` |
| `Op::Interrupt` | Call `connection.cancel()` |
| `Op::ReviewExecApproval { decision }` | Send decision to pending approval |
| `Op::Compact`, `Op::Undo`, etc. | Log warning, ignore (not supported) |

### 2.2 Agent Spawning (Single Branch Point)

**File:** `tui/src/chatwidget/agent.rs`

This is the **only TUI file modified**. Add mode detection and branch:

```rust
pub(crate) fn spawn_agent(
    config: Config,
    app_event_tx: AppEventSender,
    server: Arc<ConversationManager>,
) -> UnboundedSender<Op> {
    // Detect ACP mode based on model name
    if codex_acp::get_agent_config(&config.model).is_ok() {
        spawn_acp_agent(config, app_event_tx)
    } else {
        spawn_http_agent(config, app_event_tx, server)  // existing code, renamed
    }
}

fn spawn_acp_agent(config: Config, app_event_tx: AppEventSender) -> UnboundedSender<Op> {
    let (codex_op_tx, mut codex_op_rx) = unbounded_channel::<Op>();

    tokio::spawn(async move {
        // Create event channel for backend → TUI
        let (event_tx, mut event_rx) = mpsc::channel(32);

        let backend = match codex_acp::AcpBackend::spawn(&config, event_tx).await {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("failed to spawn ACP backend: {e}");
                return;
            }
        };

        // Forward ops to backend (same pattern as HTTP mode)
        let backend_ref = backend.clone();
        tokio::spawn(async move {
            while let Some(op) = codex_op_rx.recv().await {
                if let Err(e) = backend_ref.submit(op).await {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        // Forward events to TUI (same pattern as HTTP mode)
        while let Some(event) = event_rx.recv().await {
            app_event_tx.send(AppEvent::CodexEvent(event));
        }
    });

    codex_op_tx
}

// Existing spawn_agent code becomes spawn_http_agent (unchanged logic)
fn spawn_http_agent(
    config: Config,
    app_event_tx: AppEventSender,
    server: Arc<ConversationManager>,
) -> UnboundedSender<Op> {
    // ... existing implementation unchanged ...
}
```

### 2.3 Event Loop (NO CHANGES)

The TUI event loop in `app.rs` remains **completely unchanged**:
```rust
loop {
    select! {
        Some(event) = app_event_rx.recv() => {
            app.handle_event(tui, event).await?
        }
        Some(event) = tui_events.next() => {
            app.handle_tui_event(tui, event).await?
        }
    }
}
```

Events from both HTTP and ACP backends flow through `AppEvent::CodexEvent(Event)`.

### 2.4 Approval Bridging (NO TUI CHANGES)

Approvals work identically to HTTP mode from the TUI's perspective:

1. ACP backend receives `RequestPermissionRequest` from agent
2. Backend translates to `Event { msg: EventMsg::ExecApprovalRequest(...) }`
3. Backend sends event via `event_tx` → TUI receives as `AppEvent::CodexEvent`
4. TUI displays approval popup using **existing `approval_overlay.rs`** (no changes)
5. User approves → TUI calls `submit_op(Op::ReviewExecApproval { decision })`
6. Backend receives Op, translates to ACP `RequestPermissionOutcome`, responds to agent

**No changes needed to `approval_overlay.rs` or any other approval-related TUI code.**

### 2.5 TUI Files Summary

| File | Change |
|------|--------|
| `tui/src/chatwidget/agent.rs` | **Modified** - Add mode detection, `spawn_acp_agent()` |
| `tui/src/lib.rs` | No changes |
| `tui/src/app.rs` | No changes |
| `tui/src/chatwidget.rs` | No changes |
| `tui/src/bottom_pane/approval_overlay.rs` | No changes |

**Total TUI files modified: 1**

---

## Part 3: Model Picker

### 3.1 Separate Registries

**HTTP Registry:** `core/src/model_provider_info.rs::built_in_model_providers()`
- OpenAI, Anthropic, Azure, OSS providers
- NOT modified for ACP

**ACP Registry:** `acp/src/registry.rs::get_agent_config()`
- `mock-model` → `mock_acp_agent` binary
- `gemini-2.5-flash` / `gemini-acp` → `npx @google/gemini-cli --experimental-acp`
- `claude` / `claude-acp` → `npx @zed-industries/claude-code-acp`

### 3.2 Model Selection (MVP: CLI-Only)

For the MVP, model selection is handled via CLI argument only:

**Usage:**
```bash
# HTTP mode (existing behavior)
codex --model gpt-4o

# ACP mode (detected automatically from registry)
codex --model gemini-acp
codex --model claude-acp
codex --model mock-model
```

**Detection Logic (in `agent.rs`):**
1. Read `config.model` from CLI/config
2. Call `codex_acp::get_agent_config(&config.model)`
3. If `Ok(_)` → ACP mode
4. If `Err(_)` → HTTP mode

**No TUI picker changes required.** The existing model picker continues to work for HTTP
providers. ACP agents are selected via `--model` flag.

### 3.3 Future Enhancement: Unified Picker (Deferred)

A future iteration could add a unified picker showing both HTTP and ACP options:
- Create `tui/src/model_picker.rs` that queries both registries
- Display with clear categorization (HTTP vs ACP)
- Return either `ModelProviderInfo` or `AcpAgentConfig`

This is **out of scope for MVP** to minimize TUI changes.

---

## Part 4: Config Reuse

### 4.1 Shared Config Fields

These `Config` fields are applicable to ACP and should be read:

| Field | Usage in ACP |
|-------|--------------|
| `approval_policy` | Whether to show approval popup or auto-approve |
| `sandbox_policy` | Passed to agent (if agent supports) |
| `cwd` | Working directory for agent subprocess |
| `mcp_servers` | Passed to agent in `NewSessionRequest` |

### 4.2 Ignored Config Fields

These `Config` fields are NOT applicable to ACP:

| Field | Reason |
|-------|--------|
| `model`, `model_family` | Replaced by ACP agent selection |
| `model_provider`, `model_provider_id` | Replaced by ACP registry |
| `reasoning_effort` | Agent handles internally |
| `base_instructions`, `user_instructions` | Agent has own prompts |
| `tools`, `features` | Agent provides own tools |

---

## Part 5: Edge Cases

### 5.1 Agent Subprocess Crashes

**Scenario:** Agent process exits unexpectedly during prompt.

**Detection:** `command_rx.recv()` returns `None` in worker thread.

**Handling:**
1. Worker thread exits `run_command_loop()`
2. All pending oneshot channels are dropped
3. `AcpBackend` sends `Event { msg: EventMsg::Error(...) }` via `event_tx`
4. TUI displays error message to user (existing error handling)
5. User can restart with new agent

### 5.2 Agent Hangs (No Response)

**Scenario:** Agent stops sending SessionUpdates but doesn't exit.

**Detection:** Stream idle timeout from `AcpProviderInfo::stream_idle_timeout` (default 5 minutes).

**Handling:**
1. Prompt method returns timeout error
2. Backend sends error event
3. TUI displays error, user can cancel via existing Ctrl+C handling

### 5.3 Approval Channel Closed

**Scenario:** TUI closes before responding to approval.

**Handling:** `AcpBackend` detects dropped receiver, falls back to auto-approve (first option).

### 5.4 User Closes TUI During Approval

**Scenario:** User closes TUI while approval popup is displayed.

**Handling:** Backend's pending approval response channel is dropped, agent receives deny (last option).

### 5.5 MCP Server Configuration

**Scenario:** User has MCP servers configured in `config.toml`.

**Handling:**
1. Read `config.mcp_servers` during `AcpBackend::spawn()`
2. Pass to `AcpConnection::create_session()` in `NewSessionRequest::mcp_servers`
3. Agent handles MCP server lifecycle

### 5.6 Unknown Model Name

**Scenario:** User specifies `--model unknown-xyz`.

**Handling:**
1. `get_agent_config()` returns `Err`
2. Fall through to HTTP mode in `spawn_agent()`
3. If HTTP mode also fails, codex-core handles error display

---

## Testing Details

Tests verify **behavior**, not implementation:

1. **Connection Lifecycle** - Verifies that spawning an agent, creating a session, and sending prompts produces expected responses (not just that channels work)

2. **Approval Flow** - Verifies that when an agent requests permission, the user's approval/denial decision is correctly translated and sent back (tests the full round-trip, not just translation functions)

3. **Event Translation** - Verifies that agent messages appear as text in the TUI (tests observable output, not internal enums)

4. **Error Recovery** - Verifies that agent crashes produce user-visible error messages (tests UX, not exception handling)

---

## Implementation Details

- Worker thread uses `tokio::task::LocalSet` for !Send futures from `agent-client-protocol`
- Approval bridging translates between ACP's option-based model (multiple choices) and Codex's binary model (approve/deny)
- `AcpBackend` produces `codex_protocol::Event` directly (no intermediate `TranslatedEvent` visible to TUI)
- Config loading reuses `codex_core::config::Config` but only reads applicable fields
- E2E tests use `mock_acp_agent` binary that sends deterministic responses
- MCP servers from config are passed to agent; agent manages server lifecycle
- Stderr from agent subprocess is captured and logged via tracing
- All ACP operations are logged to `.codex-acp.log` for debugging

---

## Summary: Changes by Crate

| Crate | Files Changed | Description |
|-------|---------------|-------------|
| `codex_acp` | `backend.rs` (new) | Backend adapter providing TUI-compatible interface |
| `codex_acp` | `translator.rs` (extended) | Add functions to produce `codex_protocol::Event` |
| `codex_acp` | `lib.rs` | Export new types |
| `codex_tui` | `chatwidget/agent.rs` | Single branch point for ACP vs HTTP |
| `codex_core` | (none) | Zero changes |
| `codex_protocol` | (none) | Zero changes |

**Total files modified: 4** (3 in acp crate, 1 in tui crate)
