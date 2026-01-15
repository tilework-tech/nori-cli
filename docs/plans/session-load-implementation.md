# Implementation Plan: ACP `session/load` Feature Support

## Overview

This document outlines the implementation plan to support the ACP (Agent Client Protocol) `session/load` feature in the `codex-acp` package. This feature enables the client to request loading a previous session from the agent, receiving a "replay" of the session history via `session/update` notifications.

## References

- **ACP Documentation**: https://agentclientprotocol.com/protocol/session-setup#loading-sessions
- **Reference Implementation**: `/tmp/agent-client-protocol` (cloned for reference)
- **Schema Version**: 0.10.7
- **SDK Version**: 0.9.3 (current project has 0.7)

---

## Phase 1: Dependency Upgrade

### 1.1 Upgrade `agent-client-protocol` Crate

**File**: `codex-rs/acp/Cargo.toml`

**Current version**: `0.7`
**Target version**: `0.9.3`

```toml
[dependencies]
agent-client-protocol = "0.9"  # Upgrade from "0.7"
```

**Rationale**: Version 0.9.3 includes:
- `LoadSessionRequest` and `LoadSessionResponse` types
- `loadSession` capability in `AgentCapabilities`
- `Agent::load_session()` method in the trait

### 1.2 Update Re-exports

**File**: `codex-rs/acp/src/lib.rs`

Add new re-exports for session load types:

```rust
// Add to existing re-exports
pub use agent_client_protocol::LoadSessionRequest;
pub use agent_client_protocol::LoadSessionResponse;
```

### 1.3 Verify API Compatibility

After upgrading, check for any breaking changes in:
- `InitializeRequest`/`InitializeResponse`
- `AgentCapabilities` struct (may have new fields)
- `SessionUpdate` enum variants (may have new variants)

---

## Phase 2: Core Implementation in `connection.rs`

### 2.1 Add New `AcpCommand` Variant

**File**: `codex-rs/acp/src/connection.rs:55-70`

Add a new command variant for loading sessions:

```rust
enum AcpCommand {
    CreateSession {
        cwd: PathBuf,
        response_tx: oneshot::Sender<Result<acp::SessionId>>,
    },
    LoadSession {
        session_id: acp::SessionId,
        cwd: PathBuf,
        update_tx: mpsc::Sender<acp::SessionUpdate>,
        response_tx: oneshot::Sender<Result<acp::LoadSessionResponse>>,
    },
    Prompt { /* existing */ },
    Cancel { /* existing */ },
}
```

### 2.2 Implement `load_session` Method on `AcpConnection`

**File**: `codex-rs/acp/src/connection.rs`

Add a new public method:

```rust
impl AcpConnection {
    /// Load an existing session from the agent.
    ///
    /// The agent will replay the entire conversation history via `session/update`
    /// notifications sent to the `update_tx` channel.
    ///
    /// # Arguments
    /// * `session_id` - The ID of the session to load
    /// * `cwd` - Working directory for the session
    /// * `update_tx` - Channel to receive session updates (history replay)
    ///
    /// # Returns
    /// The `LoadSessionResponse` after all history has been replayed.
    pub async fn load_session(
        &self,
        session_id: acp::SessionId,
        cwd: &Path,
        update_tx: mpsc::Sender<acp::SessionUpdate>,
    ) -> Result<acp::LoadSessionResponse> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(AcpCommand::LoadSession {
                session_id,
                cwd: cwd.to_path_buf(),
                update_tx,
                response_tx,
            })
            .await
            .context("ACP worker thread died")?;
        response_rx.await.context("ACP worker thread died")?
    }

    /// Check if the agent supports loading sessions.
    pub fn supports_load_session(&self) -> bool {
        self.agent_capabilities.load_session
    }
}
```

### 2.3 Handle `LoadSession` Command in Worker Loop

**File**: `codex-rs/acp/src/connection.rs:353-466`

Add handler in `run_command_loop`:

```rust
async fn run_command_loop(inner: AcpConnectionInner, mut command_rx: mpsc::Receiver<AcpCommand>) {
    use acp::Agent;

    while let Some(cmd) = command_rx.recv().await {
        match cmd {
            AcpCommand::CreateSession { /* existing */ } => { /* existing */ },

            AcpCommand::LoadSession {
                session_id,
                cwd,
                update_tx,
                response_tx,
            } => {
                // Register session for receiving updates before loading
                inner.client_delegate.register_session(session_id.clone(), update_tx);

                let result = inner
                    .connection
                    .load_session(acp::LoadSessionRequest {
                        session_id: session_id.clone(),
                        cwd,
                        mcp_servers: vec![], // TODO: Support MCP servers
                        meta: None,
                    })
                    .await
                    .context("Failed to load ACP session");

                // Keep session registered for subsequent prompts
                // (don't unregister like in Prompt command)

                let _ = response_tx.send(result);
            },

            AcpCommand::Prompt { /* existing */ } => { /* existing */ },
            AcpCommand::Cancel { /* existing */ } => { /* existing */ },
        }
    }
}
```

---

## Phase 3: Backend Integration in `backend.rs`

### 3.1 Extend `AcpBackendConfig`

**File**: `codex-rs/acp/src/backend.rs:36-49`

Add optional session loading configuration:

```rust
#[derive(Debug, Clone)]
pub struct AcpBackendConfig {
    /// Model name used to look up agent in registry
    pub model: String,
    /// Working directory for the session
    pub cwd: PathBuf,
    /// Approval policy for command execution
    pub approval_policy: AskForApproval,
    /// Sandbox policy for command execution
    pub sandbox_policy: SandboxPolicy,
    /// Optional: Session ID to load (for resuming sessions)
    pub load_session_id: Option<String>,
}
```

### 3.2 Implement Session Loading in `AcpBackend::spawn`

**File**: `codex-rs/acp/src/backend.rs:66-141`

Modify the `spawn` method to support loading sessions:

```rust
impl AcpBackend {
    pub async fn spawn(config: &AcpBackendConfig, event_tx: mpsc::Sender<Event>) -> Result<Self> {
        let agent_config = get_agent_config(&config.model)?;
        let cwd = config.cwd.clone();

        debug!("Spawning ACP backend for model: {}", config.model);

        let mut connection = AcpConnection::spawn(&agent_config, &cwd).await?;

        let session_id = match &config.load_session_id {
            Some(session_id_str) if connection.supports_load_session() => {
                // Load existing session with history replay
                Self::load_session_with_replay(
                    &connection,
                    session_id_str,
                    &cwd,
                    &event_tx,
                ).await?
            }
            Some(session_id_str) => {
                // Agent doesn't support load_session, log warning and create new
                warn!(
                    "Agent does not support session/load; creating new session \
                     (requested session_id: {})",
                    session_id_str
                );
                connection.create_session(&cwd).await?
            }
            None => {
                // Create new session (existing behavior)
                connection.create_session(&cwd).await?
            }
        };

        debug!("ACP session ready: {:?}", session_id);

        // ... rest of existing spawn implementation
    }

    /// Load a session and replay its history to the TUI.
    async fn load_session_with_replay(
        connection: &AcpConnection,
        session_id_str: &str,
        cwd: &Path,
        event_tx: &mpsc::Sender<Event>,
    ) -> Result<acp::SessionId> {
        let session_id = acp::SessionId::new(session_id_str);
        let (update_tx, mut update_rx) = mpsc::channel(64);

        // Spawn task to translate and forward replay updates
        let event_tx_clone = event_tx.clone();
        let replay_handler = tokio::spawn(async move {
            while let Some(update) = update_rx.recv().await {
                let events = translate_replay_update_to_events(&update);
                for event_msg in events {
                    let _ = event_tx_clone
                        .send(Event {
                            id: "replay".to_string(),
                            msg: event_msg,
                        })
                        .await;
                }
            }
        });

        // Load session - this triggers replay via session/update notifications
        let response = connection
            .load_session(session_id.clone(), cwd, update_tx)
            .await?;

        // Wait for replay to complete
        let _ = replay_handler.await;

        debug!("Session loaded with modes: {:?}", response.modes);

        Ok(session_id)
    }
}
```

### 3.3 Implement Replay Update Translation

**File**: `codex-rs/acp/src/backend.rs`

Add a function to translate replay updates to TUI events:

```rust
/// Translate an ACP SessionUpdate during replay to codex_protocol events.
///
/// During session load, the agent replays history which includes both
/// user messages and agent responses. We translate these to appropriate
/// TUI events for display.
fn translate_replay_update_to_events(update: &acp::SessionUpdate) -> Vec<EventMsg> {
    match update {
        // User messages in history - could be displayed in history panel
        acp::SessionUpdate::UserMessageChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                // User messages during replay can be represented as history items
                // For now, emit as delta for rendering in conversation view
                vec![EventMsg::AgentMessageDelta(
                    codex_protocol::protocol::AgentMessageDeltaEvent {
                        delta: format!("[User]: {}", text.text),
                    },
                )]
            } else {
                vec![]
            }
        }
        // Agent messages in history
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                vec![EventMsg::AgentMessageDelta(
                    codex_protocol::protocol::AgentMessageDeltaEvent {
                        delta: text.text.clone(),
                    },
                )]
            } else {
                vec![]
            }
        }
        // Reasoning/thoughts in history
        acp::SessionUpdate::AgentThoughtChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                vec![EventMsg::AgentReasoningDelta(
                    codex_protocol::protocol::AgentReasoningDeltaEvent {
                        delta: text.text.clone(),
                    },
                )]
            } else {
                vec![]
            }
        }
        // Tool calls from history
        acp::SessionUpdate::ToolCall(tool_call) => {
            vec![EventMsg::ExecCommandBegin(
                codex_protocol::protocol::ExecCommandBeginEvent {
                    call_id: tool_call.id.to_string(),
                    process_id: None,
                    turn_id: String::new(),
                    command: vec![tool_call.title.clone()],
                    cwd: PathBuf::new(),
                    parsed_cmd: vec![],
                    source: codex_protocol::protocol::ExecCommandSource::Agent,
                    interaction_input: None,
                },
            )]
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            if update.fields.status == Some(acp::ToolCallStatus::Completed) {
                vec![EventMsg::ExecCommandEnd(
                    codex_protocol::protocol::ExecCommandEndEvent {
                        call_id: update.id.to_string(),
                        process_id: None,
                        turn_id: String::new(),
                        command: vec![update.fields.title.clone().unwrap_or_default()],
                        cwd: PathBuf::new(),
                        parsed_cmd: vec![],
                        source: codex_protocol::protocol::ExecCommandSource::Agent,
                        interaction_input: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        aggregated_output: String::new(),
                        exit_code: 0,
                        duration: std::time::Duration::ZERO,
                        formatted_output: String::new(),
                    },
                )]
            } else {
                vec![]
            }
        }
        // Other update types
        _ => vec![],
    }
}
```

---

## Phase 4: TUI Integration

### 4.1 Extend `spawn_acp_agent` Function

**File**: `codex-rs/tui/src/chatwidget/agent.rs:82-130`

Add support for passing session ID to load:

```rust
fn spawn_acp_agent(
    config: Config,
    app_event_tx: AppEventSender,
    load_session_id: Option<String>,  // NEW parameter
) -> UnboundedSender<Op> {
    // ...

    let acp_config = AcpBackendConfig {
        model: config.model.clone(),
        cwd: config.cwd.clone(),
        approval_policy: config.approval_policy,
        sandbox_policy: config.sandbox_policy.clone(),
        load_session_id,  // NEW field
    };

    // ... rest of implementation
}
```

### 4.2 Update `spawn_agent` Function

**File**: `codex-rs/tui/src/chatwidget/agent.rs:27-52`

Modify to accept and forward session load parameter:

```rust
pub(crate) fn spawn_agent(
    config: Config,
    app_event_tx: AppEventSender,
    server: Arc<ConversationManager>,
    load_session_id: Option<String>,  // NEW parameter
) -> UnboundedSender<Op> {
    let acp_agent_result = get_agent_config(&config.model);

    match (acp_agent_result.is_ok(), config.acp_allow_http_fallback) {
        (true, _) => spawn_acp_agent(config, app_event_tx, load_session_id),
        // ... rest unchanged
    }
}
```

---

## Phase 5: Session ID Persistence & Discovery

### 5.1 Store ACP Session IDs in Rollout Metadata

**File**: `codex-rs/core/src/rollout/recorder.rs`

Extend rollout format to include ACP session ID:

```rust
// Add to RolloutLine or create new line type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpSessionMeta {
    pub acp_session_id: String,
    pub agent_name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}
```

### 5.2 Create Session Mapping

Create a mechanism to map local rollout paths to ACP session IDs:

```rust
// New file: codex-rs/acp/src/session_store.rs

use std::collections::HashMap;
use std::path::PathBuf;

/// Maps local rollout files to ACP session IDs.
pub struct AcpSessionStore {
    /// Map from rollout path to ACP session ID
    sessions: HashMap<PathBuf, String>,
}

impl AcpSessionStore {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    pub fn register(&mut self, rollout_path: PathBuf, acp_session_id: String) {
        self.sessions.insert(rollout_path, acp_session_id);
    }

    pub fn get_session_id(&self, rollout_path: &PathBuf) -> Option<&String> {
        self.sessions.get(rollout_path)
    }
}
```

---

## Phase 6: Update Mock Agent

### 6.1 Implement Proper `load_session` in Mock Agent

**File**: `codex-rs/mock-acp-agent/src/main.rs:192-200`

Update the mock agent to replay test history:

```rust
async fn load_session(
    &self,
    arguments: acp::LoadSessionRequest,
) -> Result<acp::LoadSessionResponse, acp::Error> {
    eprintln!("Mock agent: load_session id={}", arguments.session_id);

    // Replay mock history via session/update notifications
    let session_id = arguments.session_id;

    // Replay a user message
    self.send_update(
        session_id.clone(),
        acp::SessionUpdate::UserMessageChunk(acp::ContentChunk {
            content: acp::ContentBlock::Text(acp::TextContent {
                text: "Previous user message".to_string(),
                annotations: None,
                meta: None,
            }),
            meta: None,
        }),
    ).await?;

    // Replay an agent response
    self.send_update(
        session_id.clone(),
        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk {
            content: acp::ContentBlock::Text(acp::TextContent {
                text: "Previous agent response".to_string(),
                annotations: None,
                meta: None,
            }),
            meta: None,
        }),
    ).await?;

    Ok(acp::LoadSessionResponse {
        modes: None,
        meta: None,
    })
}
```

### 6.2 Update Mock Agent Capabilities

Ensure the mock agent reports `load_session: true`:

```rust
async fn initialize(
    &self,
    _arguments: acp::InitializeRequest,
) -> Result<acp::InitializeResponse, acp::Error> {
    Ok(acp::InitializeResponse {
        protocol_version: acp::V1,
        agent_capabilities: acp::AgentCapabilities {
            load_session: true,  // Enable session loading
            ..Default::default()
        },
        // ...
    })
}
```

---

## Phase 7: Testing

### 7.1 Unit Tests

**File**: `codex-rs/acp/src/connection.rs` (test module)

```rust
#[tokio::test]
async fn test_load_session_command() {
    // Test that LoadSession command is properly sent and handled
}

#[tokio::test]
async fn test_supports_load_session() {
    // Test capability checking
}
```

### 7.2 Integration Tests

**File**: `codex-rs/acp/tests/load_session.rs`

```rust
#[tokio::test]
async fn test_load_session_with_mock_agent() {
    // Spawn mock agent with MOCK_AGENT_LOAD_SESSION enabled
    // Call load_session
    // Verify history replay events received
}

#[tokio::test]
async fn test_load_session_unsupported_agent() {
    // Test fallback when agent doesn't support load_session
}
```

### 7.3 TUI Snapshot Tests

Update existing snapshot tests to verify session loading UI.

---

## Implementation Sequence

1. **Phase 1**: Upgrade `agent-client-protocol` to 0.9.x
2. **Phase 2**: Add `load_session` to `AcpConnection`
3. **Phase 3**: Integrate into `AcpBackend`
4. **Phase 6**: Update mock agent for testing
5. **Phase 7**: Write and run tests
6. **Phase 4**: TUI integration
7. **Phase 5**: Session persistence (optional, can be deferred)

---

## Message Flow Diagram

```
┌─────────────────────────────────────────────────────────────┐
│                         TUI Layer                           │
├─────────────────────────────────────────────────────────────┤
│  spawn_agent(config, load_session_id)                       │
│       ↓                                                     │
│  AcpBackend::spawn(config with load_session_id)             │
└─────────────────────────┬───────────────────────────────────┘
                          │
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                    AcpBackend                               │
├─────────────────────────────────────────────────────────────┤
│  1. Check connection.supports_load_session()                │
│  2. If true: connection.load_session(...)                   │
│  3. Spawn replay_handler to receive updates                 │
│  4. Translate updates to EventMsg                           │
│  5. Send events to TUI via event_tx                         │
└─────────────────────────┬───────────────────────────────────┘
                          │ AcpCommand::LoadSession
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                  AcpConnection (Worker Thread)              │
├─────────────────────────────────────────────────────────────┤
│  1. Register session for updates                            │
│  2. Call connection.load_session(LoadSessionRequest)        │
│  3. Receive session/update notifications via ClientDelegate │
│  4. Forward updates via update_tx channel                   │
│  5. Return LoadSessionResponse when replay complete         │
└─────────────────────────┬───────────────────────────────────┘
                          │ JSON-RPC (session/load)
                          ↓
┌─────────────────────────────────────────────────────────────┐
│                    ACP Agent Subprocess                     │
├─────────────────────────────────────────────────────────────┤
│  1. Receive session/load request                            │
│  2. Restore session context                                 │
│  3. Stream history via session/update notifications         │
│     - UserMessageChunk for user messages                    │
│     - AgentMessageChunk for agent responses                 │
│     - AgentThoughtChunk for reasoning                       │
│     - ToolCall/ToolCallUpdate for tool history              │
│  4. Return session/load response                            │
└─────────────────────────────────────────────────────────────┘
```

---

## API Changes Summary

### New Public API

```rust
// codex-acp/src/connection.rs
impl AcpConnection {
    pub async fn load_session(
        &self,
        session_id: acp::SessionId,
        cwd: &Path,
        update_tx: mpsc::Sender<acp::SessionUpdate>,
    ) -> Result<acp::LoadSessionResponse>;

    pub fn supports_load_session(&self) -> bool;
}

// codex-acp/src/backend.rs
pub struct AcpBackendConfig {
    // ... existing fields ...
    pub load_session_id: Option<String>,
}
```

### New Types Re-exported

```rust
// codex-acp/src/lib.rs
pub use agent_client_protocol::LoadSessionRequest;
pub use agent_client_protocol::LoadSessionResponse;
```

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Breaking changes in ACP SDK 0.9 | Build failures | Review changelog; create migration guide |
| Large history causing UI lag | Poor UX during replay | Batch events; show loading indicator |
| Agent doesn't support load_session | Feature unavailable | Graceful fallback to new session |
| Session ID mismatch | Failed load | Validate session exists; handle errors |

---

## Future Enhancements (Out of Scope)

1. **Session list UI**: Display available sessions to load
2. **Session forking**: Fork at specific point using `session/fork` (unstable)
3. **Session resume**: Resume without replay using `session/resume` (unstable)
4. **MCP server persistence**: Store and restore MCP server connections

---

## Checklist

- [ ] Upgrade `agent-client-protocol` to 0.9.x
- [ ] Add `LoadSession` command to `AcpCommand` enum
- [ ] Implement `load_session()` method on `AcpConnection`
- [ ] Implement `supports_load_session()` capability check
- [ ] Handle `LoadSession` in worker command loop
- [ ] Add `load_session_id` to `AcpBackendConfig`
- [ ] Modify `AcpBackend::spawn` to handle session loading
- [ ] Implement `translate_replay_update_to_events()`
- [ ] Update TUI `spawn_agent` to accept session ID
- [ ] Update mock agent to properly implement `load_session`
- [ ] Update mock agent capabilities to report `load_session: true`
- [ ] Add unit tests for connection layer
- [ ] Add integration tests with mock agent
- [ ] Update documentation
