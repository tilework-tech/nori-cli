# ACP Client Implementation Plan

## Overview

This plan describes the implementation of an ACP (Agent Client Protocol) client in the `codex-acp` crate. The client will enable the CLI/TUI to communicate with ACP-compliant agents via JSON-RPC over stdio.

## Design Decisions

Based on scoping discussions, the following decisions have been made:

1. **Connection lifecycle**: Spawn a fresh subprocess per session (simpler implementation)
2. **Permission handling**: Bridge ACP permissions to existing codex approval system (consistent UX)
3. **Tool output**: Pass through to TUI for rendering (avoid duplicating TUI logic)
4. **Scope**: Core features (Phases 1-3) plus cancellation support (Phase 6)

## Architecture

### Current State

The `codex-acp` crate currently has:
- `registry.rs`: Agent configuration registry (`AcpAgentConfig`, `AcpAgentRegistry`)
- `lib.rs`: Re-exports registry and provides `get_agent_config()` helper

The integration point is in `codex-core/src/client.rs:173` which has:
```rust
todo!("ACP streaming not yet implemented")
```

### Target Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                       codex-core                                │
│ ┌─────────────────────────────────────────────────────────────┐ │
│ │                     ModelClient                             │ │
│ │  stream() → ResponseStream<ResponseEvent>                   │ │
│ └─────────────────────────────────────────────────────────────┘ │
│                             │                                   │
│                             ▼                                   │
│ ┌─────────────────────────────────────────────────────────────┐ │
│ │                   AcpClientAdapter                          │ │
│ │  Converts ACP SessionUpdate → ResponseEvent                 │ │
│ └─────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                       codex-acp                                 │
│ ┌─────────────────────────────────────────────────────────────┐ │
│ │                    AcpConnection                            │ │
│ │  - Manages agent subprocess lifecycle                       │ │
│ │  - Handles initialization handshake                         │ │
│ │  - Routes JSON-RPC messages                                 │ │
│ └─────────────────────────────────────────────────────────────┘ │
│                             │                                   │
│ ┌─────────────────────────────────────────────────────────────┐ │
│ │                    AcpSession                               │ │
│ │  - Per-session state (modes, models)                        │ │
│ │  - Session-scoped operations                                │ │
│ └─────────────────────────────────────────────────────────────┘ │
│                             │                                   │
│ ┌─────────────────────────────────────────────────────────────┐ │
│ │                  ClientDelegate                             │ │
│ │  - Implements acp::Client trait                             │ │
│ │  - Handles agent→client requests                            │ │
│ │  - Permission requests, file I/O, terminals                 │ │
│ └─────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
                     ┌─────────────────┐
                     │  Agent Process  │
                     │  (via stdio)    │
                     └─────────────────┘
```

## Implementation Tasks

### Phase 1: Create `AcpConnection` struct
**File:** `codex-rs/acp/src/connection.rs`

```rust
pub struct AcpConnection {
    connection: acp::ClientSideConnection,
    agent_capabilities: acp::AgentCapabilities,
    child: tokio::process::Child,
    _io_task: tokio::task::JoinHandle<Result<(), acp::Error>>,
    _stderr_task: tokio::task::JoinHandle<()>,
}

impl AcpConnection {
    pub async fn spawn(config: &AcpAgentConfig, cwd: &Path) -> Result<Self>;
    pub fn capabilities(&self) -> &acp::AgentCapabilities;
}
```

**Key responsibilities:**
- Spawn agent subprocess with proper environment
- Initialize JSON-RPC transport over stdin/stdout
- Perform ACP initialization handshake
- Version negotiation (minimum V1)
- Store agent capabilities for later use

**Reference:** `zed/crates/agent_servers/src/acp.rs:82-220`

### Phase 2: Create `AcpSession` struct
**File:** `codex-rs/acp/src/session.rs`

```rust
pub struct AcpSession {
    session_id: acp::SessionId,
    modes: Option<acp::SessionModeState>,
    update_tx: mpsc::Sender<acp::SessionUpdate>,
}
```

**Key responsibilities:**
- Track per-session state
- Store optional session modes
- Provide channel for update streaming

### Phase 3: Implement `ClientDelegate`
**File:** `codex-rs/acp/src/client_delegate.rs`

```rust
pub struct ClientDelegate {
    sessions: Arc<RwLock<HashMap<acp::SessionId, AcpSession>>>,
    permission_handler: Box<dyn PermissionHandler>,
    file_handler: Box<dyn FileHandler>,
}

#[async_trait]
impl acp::Client for ClientDelegate {
    async fn request_permission(&self, req: RequestPermissionRequest)
        -> Result<RequestPermissionResponse, acp::Error>;
    async fn write_text_file(&self, req: WriteTextFileRequest)
        -> Result<WriteTextFileResponse, acp::Error>;
    async fn read_text_file(&self, req: ReadTextFileRequest)
        -> Result<ReadTextFileResponse, acp::Error>;
    async fn session_notification(&self, notif: SessionNotification)
        -> Result<(), acp::Error>;
    // Terminal methods (initially stubbed)
    async fn create_terminal(&self, req: CreateTerminalRequest)
        -> Result<CreateTerminalResponse, acp::Error>;
    async fn terminal_output(&self, req: TerminalOutputRequest)
        -> Result<TerminalOutputResponse, acp::Error>;
    async fn kill_terminal_command(&self, req: KillTerminalCommandRequest)
        -> Result<KillTerminalCommandResponse, acp::Error>;
    async fn release_terminal(&self, req: ReleaseTerminalRequest)
        -> Result<ReleaseTerminalResponse, acp::Error>;
    async fn wait_for_terminal_exit(&self, req: WaitForTerminalExitRequest)
        -> Result<WaitForTerminalExitResponse, acp::Error>;
}
```

### Phase 4: Create `SessionUpdateTranslator`
**File:** `codex-rs/acp/src/translator.rs`

Maps ACP `SessionUpdate` variants to codex `ResponseEvent` and `EventMsg`:

| ACP SessionUpdate | ResponseEvent / EventMsg |
|-------------------|-------------------------|
| `AgentMessageChunk(ContentBlock::Text)` | `OutputTextDelta(String)` |
| `AgentMessageChunk(ContentBlock::Resource)` | `OutputItemDone(ResponseItem::Resource)` |
| `AgentThoughtChunk` | `ReasoningContentDelta` |
| `ToolCall` | `EventMsg::ExecCommandBegin` / custom tool events |
| `ToolCallUpdate` | `EventMsg::ExecCommandEnd` / tool result events |
| `Plan` | Custom plan event handling |
| `UserMessageChunk` | Echo handling (typically ignored) |
| `CurrentModeUpdate` | Mode change notification |
| `AvailableCommandsUpdate` | Slash command updates |

```rust
pub struct SessionUpdateTranslator;

impl SessionUpdateTranslator {
    pub fn translate(update: acp::SessionUpdate) -> Vec<ResponseEvent>;
    fn translate_tool_call(tc: acp::ToolCall) -> Vec<ResponseEvent>;
    fn translate_tool_call_update(tcu: acp::ToolCallUpdate) -> Vec<ResponseEvent>;
}
```

### Phase 5: Create `AcpStreamAdapter`
**File:** `codex-rs/acp/src/stream_adapter.rs`

```rust
pub struct AcpStreamAdapter {
    update_rx: mpsc::Receiver<acp::SessionUpdate>,
    translator: SessionUpdateTranslator,
}

impl Stream for AcpStreamAdapter {
    type Item = Result<ResponseEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>>;
}
```

### Phase 6: Integration with codex-core

#### Implement `stream_acp()` function
**File:** `codex-rs/core/src/client.rs`

Replace the `todo!()` at line 173 with:

```rust
async fn stream_acp(
    config: &AcpAgentConfig,
    messages: Vec<ChatMessage>,
    cwd: &Path,
    permission_handler: impl PermissionHandler,
) -> Result<ResponseStream> {
    let (tx, rx) = mpsc::channel(32);

    // Spawn fresh connection for this session
    let connection = AcpConnection::spawn(config, cwd).await?;

    // Create session
    let session_id = connection.create_session(cwd).await?;

    // Convert messages to ACP prompt format
    let prompt = convert_to_acp_prompt(&messages)?;

    // Spawn prompt task
    tokio::spawn(async move {
        let result = connection.prompt(session_id, prompt, tx).await;
        // Handle completion - connection dropped when task ends
    });

    Ok(ResponseStream { rx_event: rx })
}
```

### Phase 7: Cancellation Support

#### Implement cancellation
**File:** `codex-rs/acp/src/connection.rs`

```rust
impl AcpConnection {
    pub async fn cancel(&self, session_id: &acp::SessionId) -> Result<()> {
        self.connection.cancel(acp::CancelNotification::new(session_id.clone())).await
    }
}
```

Integrate with codex's existing cancellation mechanism (Ctrl+C handling).

## File Structure

```
codex-rs/acp/src/
├── lib.rs                 # Module exports
├── registry.rs            # Existing: agent config registry
├── connection.rs          # NEW: AcpConnection (subprocess management)
├── session.rs             # NEW: AcpSession (session state)
├── client_delegate.rs     # NEW: acp::Client implementation
├── translator.rs          # NEW: SessionUpdate → ResponseEvent
├── stream_adapter.rs      # NEW: Stream wrapper for ResponseStream
└── handlers.rs            # NEW: Permission/File handler traits
```

## Dependencies

Add to `codex-rs/acp/Cargo.toml`:
```toml
[dependencies]
agent-client-protocol = "0.7"  # Already present
tokio = { workspace = true, features = ["process", "sync"] }
futures = { workspace = true }
async-trait = { workspace = true }
```

## Testing Strategy

### Unit Tests
1. `SessionUpdateTranslator` - Test all mapping cases
2. `ClientDelegate` - Mock permission/file handlers
3. Connection initialization handshake

### Integration Tests
1. Use `mock-acp-agent` for full protocol tests
2. Test session lifecycle (new → prompt → cancel)
3. Test permission request flow

### E2E Tests
The reference tests are in `codex-rs/tui-pty-e2e/tests/prompt_flow.rs`. These tests spawn the full TUI and verify that prompts flow through to the mock agent and responses are displayed.

## Out of Scope (Deferred)

1. **Authentication** - Agents requiring auth can authenticate out-of-band initially
2. **Session Loading** - `session/load` for resuming sessions
3. **Session Listing** - `session/list` capability (unstable feature)
4. **Model Selection** - `session/set_model` (unstable feature)
5. **Terminal rendering** - Terminal UI handled by TUI, not ACP client
6. **MCP server configuration** - Pass empty MCP servers initially
7. **Connection pooling** - Spawn per session instead
8. **Advanced permission bridge** - Basic bridge only
9. **Advanced tool call mapping** - Basic mapping only

## Implementation Order

1. **Phase 1**: Create `AcpConnection` struct (connection.rs)
2. **Phase 2**: Create `AcpSession` struct (session.rs)
3. **Phase 3**: Implement `ClientDelegate` (client_delegate.rs)
4. **Phase 4**: Create `SessionUpdateTranslator` (translator.rs)
5. **Phase 5**: Create `AcpStreamAdapter` (stream_adapter.rs)
6. **Phase 6**: Integration with codex-core client.rs (replace todo!())
7. **Phase 7**: Cancellation support

## Verification Criteria

1. E2E tests in `codex-rs/tui-pty-e2e/tests/prompt_flow.rs` pass
2. Can connect to mock-acp-agent and complete a prompt turn
3. SessionUpdate events properly translate to ResponseEvent stream
4. Permission requests properly flow through to TUI/CLI
5. Cancellation (Ctrl+C) properly terminates agent operations

## Key Decision Points (Resolved)

1. **Connection lifecycle**: ✅ Spawn per session (simpler, fresh state)

2. **Permission handler interface**: ✅ Bridge to existing codex approval types (consistent UX)

3. **Tool call content**: ✅ Pass through to TUI for rendering (avoid duplication)

4. **Error handling**: Map to `anyhow::Error` with context for consistency
