# Design: Session Resume for ACP Agents in Nori CLI

## Overview

This document proposes a design for implementing session resume functionality within the Nori CLI when using Agent Client Protocol (ACP) agents. Currently, the Nori CLI supports session resume for HTTP-mode agents through the `RolloutRecorder` and `ConversationManager` infrastructure, but ACP agents lack this capability.

## Background

### Current State

**HTTP Mode (codex-core):**
- Sessions are persisted to `~/.codex/rollouts/<provider>/<year>/<month>/<day>/<id>.jsonl`
- Resume picker UI (`resume_picker.rs`) lists available sessions
- `ConversationManager::resume_conversation_from_rollout()` reconstructs history
- History is sent to the LLM API directly

**ACP Mode:**
- `AcpConnection::create_session()` always creates a new session via `session/new`
- No mechanism to load existing sessions
- Agent capabilities include `sessions.load` but it's not utilized
- History persistence during prompts is marked as TODO in `connection.rs:559-568`

### ACP Specification

According to the ACP specification, agents can advertise session capabilities:

```json
{
  "agentCapabilities": {
    "sessions": {
      "new": true,
      "load": true
    }
  }
}
```

The `session/load` method allows clients to resume existing sessions by sending session history to the agent:

```typescript
interface LoadSessionRequest {
  session_id: SessionId;      // The session ID to load
  cwd: PathBuf;               // Working directory
  history?: ContentBlock[];   // Optional conversation history
}

interface LoadSessionResponse {
  // Empty on success, indicates session is ready
}
```

## Proposed Design

### Architecture Overview

```
┌──────────────────────────────────────────────────────────────────────────┐
│                           TUI Layer                                       │
├──────────────────────────────────────────────────────────────────────────┤
│  resume_picker.rs                                                         │
│  ├─ Lists sessions from ~/.codex/rollouts/ (existing)                    │
│  └─ Lists sessions from ~/.nori/sessions/ (new for ACP)                  │
│                                                                           │
│  lib.rs                                                                   │
│  └─ Passes ResumeSelection to App::run()                                 │
│                                                                           │
│  chatwidget/agent.rs                                                      │
│  └─ spawn_agent() decides: create_session() vs load_session()            │
└──────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌──────────────────────────────────────────────────────────────────────────┐
│                           ACP Layer                                       │
├──────────────────────────────────────────────────────────────────────────┤
│  connection.rs                                                            │
│  ├─ AcpConnection::create_session() - existing                           │
│  ├─ AcpConnection::load_session() - NEW                                  │
│  └─ History persistence in prompt() - NEW                                │
│                                                                           │
│  session_storage.rs (NEW)                                                 │
│  ├─ AcpSessionStorage - manages session persistence                      │
│  ├─ save_session_update() - persists SessionUpdates                      │
│  └─ load_session_history() - reconstructs history for resume             │
└──────────────────────────────────────────────────────────────────────────┘
                                    │
                                    ▼
┌──────────────────────────────────────────────────────────────────────────┐
│                        ACP Agent Subprocess                               │
├──────────────────────────────────────────────────────────────────────────┤
│  session/new  - Creates fresh session                                    │
│  session/load - Loads existing session with history                      │
│  prompt       - Continues conversation                                   │
└──────────────────────────────────────────────────────────────────────────┘
```

### Component Details

#### 1. Session Storage (`codex-rs/acp/src/session_storage.rs`)

A new module to handle ACP session persistence.

```rust
/// Storage location for ACP sessions
pub const ACP_SESSIONS_DIR: &str = "sessions/acp";

/// Metadata for an ACP session
#[derive(Debug, Serialize, Deserialize)]
pub struct AcpSessionMeta {
    pub session_id: String,
    pub agent_kind: AgentKind,
    pub model_name: String,
    pub cwd: PathBuf,
    pub git_branch: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A single turn in the session history
#[derive(Debug, Serialize, Deserialize)]
pub struct AcpSessionTurn {
    pub timestamp: DateTime<Utc>,
    pub role: TurnRole,
    pub content: Vec<ContentBlock>,
    pub tool_calls: Vec<ToolCallRecord>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TurnRole {
    User,
    Agent,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub call_id: String,
    pub title: String,
    pub kind: Option<String>,
    pub raw_input: Option<serde_json::Value>,
    pub raw_output: Option<serde_json::Value>,
    pub status: String,
}

pub struct AcpSessionStorage {
    nori_home: PathBuf,
}

impl AcpSessionStorage {
    pub fn new(nori_home: &Path) -> Self;

    /// List all ACP sessions, optionally filtered by working directory
    pub async fn list_sessions(
        &self,
        filter_cwd: Option<&Path>,
        limit: usize,
    ) -> Result<Vec<AcpSessionMeta>>;

    /// Load session history for resume
    pub async fn load_session_history(
        &self,
        session_id: &str,
    ) -> Result<Vec<ContentBlock>>;

    /// Create a new session file
    pub fn create_session_file(
        &self,
        meta: &AcpSessionMeta,
    ) -> Result<PathBuf>;

    /// Append a turn to the session file
    pub async fn append_turn(
        &self,
        session_id: &str,
        turn: &AcpSessionTurn,
    ) -> Result<()>;

    /// Get session metadata by ID
    pub async fn get_session_meta(
        &self,
        session_id: &str,
    ) -> Result<Option<AcpSessionMeta>>;
}
```

**Storage Format:**

Sessions are stored in JSONL format at:
```
~/.nori/cli/sessions/acp/<agent>/<year>/<month>/<day>/<session_id>.jsonl
```

Example file structure:
```jsonl
{"type":"meta","session_id":"abc123","agent_kind":"ClaudeCode","model_name":"claude-code","cwd":"/home/user/project","created_at":"2025-01-09T10:00:00Z"}
{"type":"turn","timestamp":"2025-01-09T10:00:01Z","role":"User","content":[{"type":"text","text":"Hello"}]}
{"type":"turn","timestamp":"2025-01-09T10:00:02Z","role":"Agent","content":[{"type":"text","text":"Hi! How can I help?"}],"tool_calls":[]}
```

#### 2. AcpConnection Extensions (`codex-rs/acp/src/connection.rs`)

Add new methods to support session loading:

```rust
/// Commands sent from the main thread to the ACP worker thread.
enum AcpCommand {
    CreateSession { /* existing */ },
    LoadSession {
        session_id: acp::SessionId,
        cwd: PathBuf,
        history: Vec<acp::ContentBlock>,
        response_tx: oneshot::Sender<Result<()>>,
    },
    Prompt { /* existing */ },
    Cancel { /* existing */ },
    #[cfg(feature = "unstable")]
    SetModel { /* existing */ },
}

impl AcpConnection {
    /// Load an existing session with history.
    ///
    /// This sends a `session/load` request to the ACP agent with the
    /// reconstructed conversation history. Use this to resume sessions
    /// that were previously saved.
    ///
    /// # Arguments
    /// * `session_id` - The session ID to load (from saved session)
    /// * `cwd` - Working directory for the session
    /// * `history` - Conversation history as ContentBlocks
    ///
    /// # Returns
    /// Ok(()) if the session was loaded successfully.
    ///
    /// # Errors
    /// Returns an error if:
    /// - The agent doesn't support session loading (`sessions.load` capability)
    /// - The session ID is invalid or not found by the agent
    /// - The worker thread has died
    pub async fn load_session(
        &self,
        session_id: acp::SessionId,
        cwd: &Path,
        history: Vec<acp::ContentBlock>,
    ) -> Result<()> {
        // Check if agent supports session loading
        if !self.agent_capabilities.sessions.as_ref()
            .map(|s| s.load.unwrap_or(false))
            .unwrap_or(false)
        {
            anyhow::bail!("Agent does not support session loading");
        }

        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(AcpCommand::LoadSession {
                session_id,
                cwd: cwd.to_path_buf(),
                history,
                response_tx,
            })
            .await
            .context("ACP worker thread died")?;
        response_rx.await.context("ACP worker thread died")?
    }

    /// Check if the agent supports session loading
    pub fn supports_session_loading(&self) -> bool {
        self.agent_capabilities
            .sessions
            .as_ref()
            .map(|s| s.load.unwrap_or(false))
            .unwrap_or(false)
    }
}
```

Worker thread handling:

```rust
AcpCommand::LoadSession {
    session_id,
    cwd,
    history,
    response_tx,
} => {
    let result = inner
        .connection
        .load_session(acp::LoadSessionRequest::new(session_id.clone(), cwd)
            .history(history))
        .await
        .context("Failed to load ACP session");

    // On success, model state may be updated from response
    // (similar to create_session handling)

    let _ = response_tx.send(result.map(|_| ()));
}
```

#### 3. History Persistence During Prompts

Extend the prompt handling to persist conversation history:

```rust
// In run_command_loop, within AcpCommand::Prompt handling:

AcpCommand::Prompt {
    session_id,
    prompt,
    update_tx,
    response_tx,
    session_storage,  // NEW: Optional storage reference
} => {
    // Track updates for persistence
    let mut turn_updates: Vec<acp::SessionUpdate> = Vec::new();

    // ... existing prompt handling with update collection ...

    // After successful prompt completion:
    if let Some(storage) = session_storage {
        // Convert SessionUpdates to AcpSessionTurn
        let turn = convert_updates_to_turn(&turn_updates);
        if let Err(e) = storage.append_turn(&session_id.to_string(), &turn).await {
            warn!("Failed to persist session turn: {}", e);
        }
    }
}
```

#### 4. Backend Integration (`codex-rs/acp/src/backend.rs`)

Extend `AcpBackend` to support resume:

```rust
pub struct AcpBackendConfig {
    pub model: String,
    pub cwd: PathBuf,
    pub approval_policy: ApprovalPolicy,
    pub sandbox_policy: SandboxPolicy,
    pub resume_session: Option<ResumeSessionConfig>,  // NEW
}

#[derive(Debug, Clone)]
pub struct ResumeSessionConfig {
    pub session_id: String,
    pub history_path: PathBuf,
}

impl AcpBackend {
    pub async fn spawn(
        config: &AcpBackendConfig,
        event_tx: mpsc::Sender<Event>,
    ) -> Result<(Self, acp::SessionId)> {
        let agent_config = get_agent_config(&config.model)?;
        let connection = Arc::new(
            AcpConnection::spawn(&agent_config, &config.cwd).await?
        );

        let session_id = if let Some(resume) = &config.resume_session {
            // Load existing session
            let history = load_history_from_path(&resume.history_path).await?;
            let session_id = acp::SessionId::new(resume.session_id.clone());

            if connection.supports_session_loading() {
                connection.load_session(
                    session_id.clone(),
                    &config.cwd,
                    history,
                ).await?;
            } else {
                // Fallback: Create new session but replay history via prompts
                // (discussed in Alternatives section)
                return Err(anyhow::anyhow!(
                    "Agent does not support session resume"
                ));
            }

            session_id
        } else {
            // Create new session (existing behavior)
            connection.create_session(&config.cwd).await?
        };

        // ... rest of spawn logic ...
    }
}
```

#### 5. TUI Integration (`codex-rs/tui/src/chatwidget/agent.rs`)

Update agent spawning to support resume:

```rust
pub(crate) async fn spawn_agent(
    config: &Config,
    event_tx: mpsc::Sender<Event>,
    resume_session: Option<&ResumeSessionInfo>,  // NEW parameter
) -> Result<SpawnedAgent> {
    let model = &config.model;

    if is_acp_registered(model) {
        let backend_config = AcpBackendConfig {
            model: model.clone(),
            cwd: config.cwd.clone(),
            approval_policy: config.approval_policy,
            sandbox_policy: config.sandbox_policy,
            resume_session: resume_session.map(|r| ResumeSessionConfig {
                session_id: r.session_id.clone(),
                history_path: r.history_path.clone(),
            }),
        };

        let (backend, session_id) = AcpBackend::spawn(&backend_config, event_tx).await?;
        Ok(SpawnedAgent::Acp { backend, session_id })
    } else {
        // HTTP mode handling (existing)
    }
}
```

#### 6. Resume Picker Updates (`codex-rs/tui/src/resume_picker.rs`)

Extend the resume picker to show ACP sessions:

```rust
pub async fn run_resume_picker(
    tui: &mut Tui,
    codex_home: &Path,
    nori_home: &Path,  // NEW: for ACP sessions
    default_provider: &str,
    show_all: bool,
) -> Result<ResumeSelection> {
    // Load sessions from both sources
    let http_sessions = load_http_sessions(codex_home, default_provider).await?;
    let acp_sessions = load_acp_sessions(nori_home).await?;

    // Merge and sort by updated_at
    let all_sessions = merge_sessions(http_sessions, acp_sessions);

    // ... existing picker UI with source indicator ...
}

#[derive(Debug, Clone)]
pub enum ResumeSelection {
    StartFresh,
    ResumeHttp(PathBuf),      // Existing HTTP rollout
    ResumeAcp(AcpSessionRef), // NEW: ACP session reference
    Exit,
}

#[derive(Debug, Clone)]
pub struct AcpSessionRef {
    pub session_id: String,
    pub agent_kind: AgentKind,
    pub history_path: PathBuf,
}
```

### Data Flow: Resuming a Session

```
1. User runs: nori resume
   │
   ├─ Resume picker shows sessions from:
   │  - ~/.codex/rollouts/ (HTTP mode)
   │  - ~/.nori/cli/sessions/acp/ (ACP mode)
   │
2. User selects ACP session
   │
   ├─ ResumeSelection::ResumeAcp(AcpSessionRef) returned
   │
3. App::run() receives selection
   │
   ├─ Passes resume info to ChatWidget::init()
   │
4. spawn_agent() called with resume_session
   │
   ├─ AcpBackend::spawn() with ResumeSessionConfig
   │
5. AcpConnection checks capabilities
   │
   ├─ If supports_session_loading():
   │  │
   │  ├─ Load history from session file
   │  │
   │  └─ Call connection.load_session(session_id, cwd, history)
   │     │
   │     └─ Agent receives session/load request
   │        │
   │        └─ Agent restores internal state + context
   │
   └─ If NOT supported:
      │
      └─ Error: "Agent does not support session resume"

6. Session ready for interaction
   │
   ├─ User prompts continue the conversation
   │
   └─ New turns appended to session file
```

### Data Flow: Persisting Session Updates

```
1. User sends prompt
   │
   └─ AcpBackend::submit(Op::UserInput)
      │
      ├─ Persist user turn to session file
      │
      └─ connection.prompt(session_id, content, update_tx)
         │
         ├─ Collect SessionUpdate events
         │
2. Agent responds
   │
   ├─ AgentMessageChunk events
   │  └─ Accumulated into turn content
   │
   ├─ ToolCall events
   │  └─ Recorded in turn tool_calls
   │
   └─ ToolCallUpdate(Completed)
      └─ Tool output captured
         │
3. Prompt completes
   │
   └─ Persist agent turn to session file
      │
      └─ AcpSessionTurn {
           role: Agent,
           content: [accumulated text],
           tool_calls: [recorded calls],
         }
```

## Implementation Plan

### Phase 1: Session Storage Foundation
1. Create `session_storage.rs` module
2. Implement `AcpSessionStorage` with basic CRUD operations
3. Define JSONL schema for session files
4. Add unit tests for storage operations

### Phase 2: History Persistence
1. Extend `AcpCommand::Prompt` to collect updates
2. Implement `convert_updates_to_turn()` function
3. Add optional storage reference to backend
4. Persist turns after successful prompts

### Phase 3: Session Loading
1. Add `load_session` to `AcpCommand` enum
2. Implement `AcpConnection::load_session()`
3. Add capability checking for session loading
4. Handle worker thread command processing

### Phase 4: Backend Integration
1. Extend `AcpBackendConfig` with resume options
2. Modify `AcpBackend::spawn()` for resume flow
3. Implement history loading from session files
4. Add error handling for unsupported agents

### Phase 5: TUI Integration
1. Update `spawn_agent()` signature for resume
2. Extend resume picker to show ACP sessions
3. Add session source indicator in picker UI
4. Pass resume selection through to backend

### Phase 6: Testing & Polish
1. Add E2E tests with mock-acp-agent
2. Test resume picker with mixed session sources
3. Handle edge cases (corrupted files, missing sessions)
4. Add user feedback for resume operations

## Alternatives Considered

### Alternative 1: Client-Side History Replay

Instead of using `session/load`, the client could:
1. Create a new session via `session/new`
2. Send history as context in the first prompt

**Pros:**
- Works with agents that don't support `session/load`
- Simpler agent implementation

**Cons:**
- May exceed prompt token limits for long conversations
- Agent loses internal state (memory, tool results cache)
- Doesn't preserve the original session ID

### Alternative 2: Unified Storage with codex-core

Store ACP sessions in the same `~/.codex/rollouts/` format.

**Pros:**
- Single source of truth for all sessions
- Reuse existing resume picker logic

**Cons:**
- ACP SessionUpdate format differs from Codex ResponseItem
- May cause confusion when switching between modes
- Tighter coupling between ACP and core

### Alternative 3: Agent-Side Storage Only

Let agents handle all session persistence internally.

**Pros:**
- Simplest client implementation
- Agents control their own state format

**Cons:**
- No visibility into sessions from CLI
- Can't list/browse sessions without querying each agent
- Inconsistent UX across agents

## Security Considerations

1. **Session File Permissions**: Session files should be readable only by the user (0600)
2. **History Content**: Avoid persisting sensitive data (API keys, passwords detected in output)
3. **Session ID Validation**: Sanitize session IDs before using in file paths
4. **Agent Trust**: Only load sessions for agents the user has configured

## Future Enhancements

1. **Session Forking**: Create new sessions branched from a specific point
2. **Session Export**: Export sessions in portable formats (Markdown, JSON)
3. **Session Search**: Full-text search across session content
4. **Session Sharing**: Share sessions between team members (with sanitization)
5. **Cross-Agent Resume**: Resume a Claude session with Gemini (with history translation)

## References

- [ACP Session Setup Specification](https://agentclientprotocol.com/protocol/session-setup)
- [ACP Protocol Overview](https://agentclientprotocol.com/protocol/overview)
- [Existing Resume Implementation](codex-rs/tui/src/resume_picker.rs)
- [ACP Connection Module](codex-rs/acp/src/connection.rs)
- [Rollout Recording](codex-rs/core/src/rollout.rs)
