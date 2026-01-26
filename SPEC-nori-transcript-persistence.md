# Nori Transcript Persistence - Implementation Specification

## Overview

A new persistence layer in the ACP package that captures the **client-side view** of conversations - what the user typed and what the assistant responded with - stored in a format that allows full transcript reload for viewing without replaying the underlying agent session mechanics.

This replaces the core rollout persistence system for ACP sessions.

## Goals

1. **View Previous Sessions**: Users can browse and view complete transcripts of past conversations
2. **Project-Grouped Storage**: Transcripts organized by git project (or cwd) rather than by date
3. **Client-Side Perspective**: Store what the user saw, not internal agent execution details
4. **Complete Reload**: Assistant messages sufficient to render the full conversation without agent replay
5. **Nori-Specific**: Uses `NORI_HOME` and Nori-specific schema, completely separate from core rollout

## Non-Goals (Out of Scope)

- **Session Resume**: Resuming a session with full API context requires agent-level changes
- **Cross-Session Search**: Full-text search across transcripts (future work)
- **Archival/Cleanup Policies**: Automatic transcript rotation or deletion

---

## Comparison with Existing Systems

| Aspect | Core Rollout (`codex-core`) | Message History (`acp`) | **Nori Transcript (new)** |
|--------|----------------------------|------------------------|---------------------------|
| **Perspective** | Agent-side (all internal events) | Simple text log | Client-side (rendered messages) |
| **Location** | `~/.codex/sessions/YYYY/MM/DD/` | `~/.nori/cli/history.jsonl` | `$NORI_HOME/transcripts/` |
| **Organization** | By date | Single file | By git project |
| **Content** | ResponseItems, function calls, events | Session ID + text | User prompts + assistant content blocks |
| **Purpose** | Session resume with full state | Quick history lookup | Transcript viewing |
| **Format** | JSONL with RolloutItem wrapper | JSONL simple entries | JSONL with NoriTranscriptEntry |

---

## Storage Structure

```
$NORI_HOME/
└── transcripts/
    ├── by-project/
    │   ├── {project-id}/                    # Hash-based project identifier
    │   │   ├── project.json                 # Project metadata
    │   │   └── sessions/
    │   │       ├── {session-id}.jsonl       # Individual session transcript
    │   │       └── {session-id}.jsonl
    │   └── {another-project-id}/
    │       └── ...
    └── index.json                           # Optional: quick lookup cache (future)
```

### Project Identification

The `{project-id}` is derived as follows:

1. **Git repository with remote**: SHA-256 hash of the canonical remote URL (first 16 hex chars)
2. **Git repository without remote**: SHA-256 hash of the git root absolute path (first 16 hex chars)
3. **No git**: SHA-256 hash of the working directory absolute path (first 16 hex chars)

This ensures:
- Same project always maps to same directory
- Different projects don't collide
- Path remains stable even if user opens from different subdirectory of same repo

### Project Metadata (`project.json`)

```json
{
  "id": "a1b2c3d4e5f67890",
  "name": "nori-cli",
  "git_remote": "git@github.com:user/nori-cli.git",
  "git_root": "/home/user/projects/nori-cli",
  "cwd": "/home/user/projects/nori-cli",
  "created_at": "2025-01-26T10:30:00Z",
  "updated_at": "2025-01-26T15:45:00Z"
}
```

---

## JSONL Schema (Nori-Specific)

Each line in a session transcript file is a self-contained entry. The schema is designed for the client-side view.

### Entry Wrapper

```rust
#[derive(Serialize, Deserialize)]
pub struct TranscriptLine {
    /// ISO 8601 timestamp
    pub ts: String,
    /// Schema version for forward compatibility
    pub v: u8,
    /// The entry payload
    #[serde(flatten)]
    pub entry: TranscriptEntry,
}
```

### Entry Types

```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptEntry {
    /// Session metadata (first line of file)
    SessionMeta(SessionMetaEntry),
    /// User message
    User(UserEntry),
    /// Complete assistant turn
    Assistant(AssistantEntry),
    /// Tool execution (stored like core rollout for consistency)
    ToolCall(ToolCallEntry),
    /// Tool result
    ToolResult(ToolResultEntry),
}
```

### Session Metadata Entry

```rust
#[derive(Serialize, Deserialize)]
pub struct SessionMetaEntry {
    pub session_id: String,
    pub project_id: String,
    pub started_at: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub cli_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
}
```

### User Entry

```rust
#[derive(Serialize, Deserialize)]
pub struct UserEntry {
    /// Unique message ID
    pub id: String,
    /// The user's input text
    pub content: String,
    /// Optional: images or other attachments (paths or base64)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub attachments: Vec<Attachment>,
}
```

### Assistant Entry

```rust
#[derive(Serialize, Deserialize)]
pub struct AssistantEntry {
    /// Unique message ID
    pub id: String,
    /// Content blocks (mirrors Anthropic API structure)
    pub content: Vec<ContentBlock>,
    /// Model that generated this response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Thinking { thinking: String },
    // Tool use is recorded separately as ToolCall/ToolResult entries
}
```

### Tool Call Entry (Matching Core Rollout Design)

```rust
#[derive(Serialize, Deserialize)]
pub struct ToolCallEntry {
    /// Unique call ID (for correlating with result)
    pub call_id: String,
    /// Tool name (e.g., "shell", "read", "edit")
    pub name: String,
    /// Tool input (JSON-serialized arguments)
    pub input: serde_json::Value,
}
```

### Tool Result Entry

```rust
#[derive(Serialize, Deserialize)]
pub struct ToolResultEntry {
    /// Correlates with ToolCallEntry.call_id
    pub call_id: String,
    /// Tool output (may be truncated for large outputs)
    pub output: String,
    /// Whether output was truncated
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub truncated: bool,
    /// Exit code for shell commands
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}
```

---

## Persistence Approach: Streaming (Per-Item)

Based on analysis of the core rollout system (`codex-rs/core/src/rollout/recorder.rs`):

**Core rollout uses streaming persistence:**
- Each item is written immediately via async channel
- `JsonlWriter::write_rollout_item()` writes and flushes after each item
- No staging/batching of items before write

**Nori transcript will follow the same pattern:**
- Write each entry as it occurs (user message, tool call, tool result, assistant content)
- Immediate flush ensures durability (crash safety)
- Async channel decouples event handling from I/O

This means:
- User messages are persisted when submitted
- Tool calls are persisted when they begin
- Tool results are persisted when they complete
- Assistant content is persisted as complete turns (after streaming completes)

---

## API Surface

### Module Location

```
codex-rs/acp/src/transcript/
├── mod.rs           # Public exports
├── recorder.rs      # TranscriptRecorder implementation
├── loader.rs        # TranscriptLoader for reading
├── project.rs       # Project identification logic
└── types.rs         # Schema types (TranscriptEntry, etc.)
```

### TranscriptRecorder

```rust
// acp/src/transcript/recorder.rs

/// Records transcript entries for a session.
/// Uses async channel for non-blocking writes (same pattern as core RolloutRecorder).
#[derive(Clone)]
pub struct TranscriptRecorder {
    tx: Sender<TranscriptCmd>,
    session_id: String,
    project_id: String,
    transcript_path: PathBuf,
}

enum TranscriptCmd {
    Write(TranscriptEntry),
    Flush { ack: oneshot::Sender<()> },
    Shutdown { ack: oneshot::Sender<()> },
}

impl TranscriptRecorder {
    /// Initialize for a new session.
    /// - Detects project from cwd (git root or cwd path)
    /// - Creates project directory if needed
    /// - Opens new session JSONL file
    /// - Writes SessionMeta as first entry
    pub async fn new(
        nori_home: &Path,
        cwd: &Path,
        model: Option<String>,
    ) -> std::io::Result<Self>;

    /// Record a user message.
    pub async fn record_user_message(
        &self,
        content: &str,
        attachments: Vec<Attachment>,
    ) -> std::io::Result<()>;

    /// Record a tool call (when tool execution begins).
    pub async fn record_tool_call(
        &self,
        call_id: &str,
        name: &str,
        input: &serde_json::Value,
    ) -> std::io::Result<()>;

    /// Record a tool result (when tool execution completes).
    pub async fn record_tool_result(
        &self,
        call_id: &str,
        output: &str,
        truncated: bool,
        exit_code: Option<i32>,
    ) -> std::io::Result<()>;

    /// Record a complete assistant turn (after streaming finishes).
    pub async fn record_assistant_message(
        &self,
        content: Vec<ContentBlock>,
        model: Option<String>,
    ) -> std::io::Result<()>;

    /// Flush all pending writes.
    pub async fn flush(&self) -> std::io::Result<()>;

    /// Graceful shutdown.
    pub async fn shutdown(&self) -> std::io::Result<()>;

    /// Get the path to this session's transcript file.
    pub fn transcript_path(&self) -> &Path;

    /// Get the session ID.
    pub fn session_id(&self) -> &str;

    /// Get the project ID.
    pub fn project_id(&self) -> &str;
}
```

### TranscriptLoader

```rust
// acp/src/transcript/loader.rs

/// Loads and lists transcripts for viewing.
pub struct TranscriptLoader {
    nori_home: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub id: String,
    pub name: String,
    pub git_remote: Option<String>,
    pub cwd: PathBuf,
    pub session_count: usize,
    pub last_session_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_id: String,
    pub project_id: String,
    pub started_at: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub message_count: usize,
}

#[derive(Debug)]
pub struct Transcript {
    pub meta: SessionMetaEntry,
    pub entries: Vec<TranscriptLine>,
}

impl TranscriptLoader {
    pub fn new(nori_home: PathBuf) -> Self;

    /// List all projects that have transcripts.
    pub async fn list_projects(&self) -> std::io::Result<Vec<ProjectInfo>>;

    /// List all sessions for a specific project.
    pub async fn list_sessions(
        &self,
        project_id: &str,
    ) -> std::io::Result<Vec<SessionInfo>>;

    /// Find sessions for the current working directory.
    /// Useful for showing "recent sessions in this project".
    pub async fn find_sessions_for_cwd(
        &self,
        cwd: &Path,
    ) -> std::io::Result<Vec<SessionInfo>>;

    /// Load a complete transcript for display.
    pub async fn load_transcript(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> std::io::Result<Transcript>;

    /// Load just the session metadata (for quick listing).
    pub async fn load_session_meta(
        &self,
        project_id: &str,
        session_id: &str,
    ) -> std::io::Result<SessionMetaEntry>;
}
```

### Project Identification

```rust
// acp/src/transcript/project.rs

/// Compute project ID from working directory.
pub fn compute_project_id(cwd: &Path) -> std::io::Result<ProjectId>;

/// Project identification result.
pub struct ProjectId {
    /// The hash-based identifier (16 hex chars)
    pub id: String,
    /// Human-readable project name (directory name or repo name)
    pub name: String,
    /// Git remote URL if available
    pub git_remote: Option<String>,
    /// Git root path if in a git repo
    pub git_root: Option<PathBuf>,
    /// The original cwd
    pub cwd: PathBuf,
}
```

---

## Integration Points in ACP Backend

### Hook Locations in `backend.rs`

1. **Session Start** (`AcpBackend::new` or first operation):
   ```rust
   // Initialize TranscriptRecorder when backend starts
   let transcript_recorder = TranscriptRecorder::new(
       &config.nori_home,
       &config.cwd,
       Some(config.model.clone()),
   ).await?;
   ```

2. **User Input** (`handle_user_input`):
   ```rust
   // At start of handle_user_input, after extracting text
   self.transcript_recorder.record_user_message(&prompt_text, vec![]).await?;
   ```

3. **Tool Execution Begin** (in translator or update handling):
   ```rust
   // When a tool call event is received
   self.transcript_recorder.record_tool_call(
       &call_id,
       &tool_name,
       &tool_input,
   ).await?;
   ```

4. **Tool Execution End** (in update handling):
   ```rust
   // When tool result is received
   self.transcript_recorder.record_tool_result(
       &call_id,
       &output,
       truncated,
       exit_code,
   ).await?;
   ```

5. **Assistant Turn Complete** (after `TaskComplete` event):
   ```rust
   // Collect streamed content blocks and record
   self.transcript_recorder.record_assistant_message(
       collected_content_blocks,
       Some(model.clone()),
   ).await?;
   ```

6. **Shutdown** (`Op::Shutdown` handling):
   ```rust
   self.transcript_recorder.shutdown().await?;
   ```

### Disabling Core Rollout

Since this replaces core rollout for ACP:

1. The ACP backend does not use `RolloutRecorder` from `codex-core`
2. Core rollout code remains in `codex-core` for potential future use
3. No changes needed to core rollout code itself

---

## Content Accumulation for Assistant Messages

Since the ACP backend receives streaming deltas (`AgentMessageDelta`), we need to accumulate content before recording:

```rust
// In AcpBackend, add fields to track current turn
struct TurnAccumulator {
    content_blocks: Vec<ContentBlock>,
    current_text: String,
    model: Option<String>,
}

impl AcpBackend {
    fn handle_agent_delta(&mut self, delta: &str) {
        self.turn_accumulator.current_text.push_str(delta);
    }

    fn finalize_assistant_turn(&mut self) {
        if !self.turn_accumulator.current_text.is_empty() {
            self.turn_accumulator.content_blocks.push(
                ContentBlock::Text { 
                    text: std::mem::take(&mut self.turn_accumulator.current_text) 
                }
            );
        }
        // Record to transcript
        // ...
        // Reset accumulator
        self.turn_accumulator = TurnAccumulator::default();
    }
}
```

---

## Example Transcript File

`$NORI_HOME/transcripts/by-project/a1b2c3d4e5f67890/sessions/550e8400-e29b-41d4-a716-446655440000.jsonl`:

```jsonl
{"ts":"2025-01-26T10:30:00.000Z","v":1,"type":"session_meta","session_id":"550e8400-e29b-41d4-a716-446655440000","project_id":"a1b2c3d4e5f67890","started_at":"2025-01-26T10:30:00.000Z","cwd":"/home/user/projects/nori-cli","model":"claude-sonnet-4-20250514","cli_version":"0.1.0","git":{"branch":"main","commit_hash":"abc123"}}
{"ts":"2025-01-26T10:30:05.123Z","v":1,"type":"user","id":"msg-001","content":"What files are in the src directory?"}
{"ts":"2025-01-26T10:30:06.456Z","v":1,"type":"tool_call","call_id":"call-001","name":"shell","input":{"command":"ls -la src/"}}
{"ts":"2025-01-26T10:30:07.789Z","v":1,"type":"tool_result","call_id":"call-001","output":"total 48\ndrwxr-xr-x  5 user user  4096 Jan 26 10:00 .\n-rw-r--r--  1 user user  1234 Jan 26 10:00 main.rs\n-rw-r--r--  1 user user  5678 Jan 26 10:00 lib.rs","exit_code":0}
{"ts":"2025-01-26T10:30:08.012Z","v":1,"type":"assistant","id":"msg-002","content":[{"type":"text","text":"The src directory contains:\n\n- `main.rs` (1.2 KB) - The main entry point\n- `lib.rs` (5.7 KB) - The library module\n\nWould you like me to show the contents of any of these files?"}],"model":"claude-sonnet-4-20250514"}
{"ts":"2025-01-26T10:31:00.000Z","v":1,"type":"user","id":"msg-003","content":"Show me main.rs"}
{"ts":"2025-01-26T10:31:01.234Z","v":1,"type":"tool_call","call_id":"call-002","name":"read","input":{"path":"src/main.rs"}}
{"ts":"2025-01-26T10:31:01.567Z","v":1,"type":"tool_result","call_id":"call-002","output":"fn main() {\n    println!(\"Hello, world!\");\n}"}
{"ts":"2025-01-26T10:31:02.890Z","v":1,"type":"assistant","id":"msg-004","content":[{"type":"text","text":"Here's the contents of `main.rs`:\n\n```rust\nfn main() {\n    println!(\"Hello, world!\");\n}\n```\n\nIt's a simple hello world program."}],"model":"claude-sonnet-4-20250514"}
```

---

## Testing Strategy

### Unit Tests

1. **Project ID computation**:
   - Git repo with remote -> consistent hash
   - Git repo without remote -> path-based hash
   - Non-git directory -> path-based hash
   - Same repo, different subdirectory -> same project ID

2. **TranscriptRecorder**:
   - Creates correct directory structure
   - Writes valid JSONL
   - SessionMeta is first line
   - Entries have correct timestamps
   - Flush/shutdown work correctly

3. **TranscriptLoader**:
   - Lists projects correctly
   - Lists sessions for project
   - Finds sessions by cwd
   - Loads complete transcript
   - Handles malformed entries gracefully

### Integration Tests

1. **Full flow**: User input -> tool calls -> assistant response -> all recorded
2. **Multi-turn**: Multiple user/assistant exchanges in one session
3. **Reload**: Write transcript, load it, verify contents match

### E2E Tests (tui-pty-e2e)

1. Start session, send message, verify transcript file created
2. Multiple sessions in same project -> same project directory
3. Different projects -> different project directories

---

## Migration / Compatibility

- **No migration needed**: This is a new system, not replacing existing data
- **Core rollout files remain**: Existing `~/.codex/sessions/` files are untouched
- **Parallel operation initially**: Could run both systems during transition if needed

---

## Future Considerations (Out of Scope)

1. **Session Resume**: Would require storing API conversation history format, not just display format
2. **Transcript Search**: Full-text search across all transcripts
3. **Transcript Export**: Export to markdown, HTML, or other formats
4. **Automatic Cleanup**: Retention policies, archival
5. **Encryption**: Encrypt transcripts at rest

---

## Implementation Order

1. **Phase 1: Types and Project ID** (`types.rs`, `project.rs`)
   - Define all schema types
   - Implement project ID computation
   - Unit tests for project identification

2. **Phase 2: TranscriptRecorder** (`recorder.rs`)
   - Async channel-based writer
   - Session initialization
   - All record_* methods
   - Unit tests

3. **Phase 3: TranscriptLoader** (`loader.rs`)
   - Project/session listing
   - Transcript loading
   - Unit tests

4. **Phase 4: ACP Integration** (`backend.rs`)
   - Add TranscriptRecorder to AcpBackend
   - Hook into user input, tool calls, assistant messages
   - Integration tests

5. **Phase 5: E2E Tests** (`tui-pty-e2e`)
   - Verify transcript files are created
   - Verify content is correct

---

## Open Questions Resolved

1. **Tool use storage**: Same as core rollout - separate ToolCall and ToolResult entries
2. **Streaming vs staged**: Streaming (per-item writes with immediate flush), matching core rollout
3. **Schema**: Nori-specific `TranscriptEntry` enum with versioning
4. **Resume vs reload**: Reload only (viewing); resume is out of scope
5. **Core rollout**: Replaced for ACP; core code remains for potential future use
