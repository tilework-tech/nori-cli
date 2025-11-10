# Noridoc: src/backends

Path: @/src/backends

### Overview

Backend implementations for spawning and interacting with different AI coding agent CLIs. Defines the AgentBackend trait and provides concrete implementations for Claude Code (claude.rs), GPT Codex (codex.rs), and a mock backend for testing (mock.rs).

### How it fits into the larger codebase

- Trait definition and implementations are re-exported via @/src/backends.rs module declaration
- Called from @/src/main.rs:get_backend() which selects backend based on user's agent choice
- @/src/main.rs:spawn_and_stream() receives trait object (Box<dyn AgentBackend>) and calls spawn_process()
- MockBackend is used by @/tests/subprocess_test.rs to avoid requiring actual CLI installations in CI
- All backends return tokio::process::Child with piped stdout/stderr for JSONL parsing

### Core Implementation

**Trait Definition** (@/src/backends.rs):
```rust
#[async_trait]
pub trait AgentBackend {
    async fn spawn_process(&self, prompt: String) -> Result<Child>;
    fn name(&self) -> &str;
}
```

**Claude Code Backend** (@/src/backends/claude.rs):
- Command: `claude --print --output-format stream-json --include-partial-messages --verbose <prompt>`
- `--verbose` is required when using stream-json with --print mode
- Session resumption: Includes `--resume <session_id>` if ClaudeBackend.session_id is Some
- session_id field exists but is never populated - prepared for future persistence feature
- Spawns with stdout/stderr piped to enable JSONL event streaming back to main loop

**GPT Codex Backend** (@/src/backends/codex.rs):
- Command: `codex exec --json <prompt>`
- Session resumption: Prepends `resume <thread_id>` argument if CodexBackend.thread_id is Some
- thread_id field exists but is never populated - infrastructure for multi-turn conversations
- Headless mode via `exec` subcommand for non-interactive operation
- Always uses --json flag to get structured event output

**Mock Backend** (@/src/backends/mock.rs):
- Uses `printf` shell command to output hardcoded JSONL without requiring agent CLI installation
- Outputs two test events in actual Claude CLI format: `{"type":"assistant","message":{"content":[{"type":"text","text":"Hello from mock"}]}}`
- Format matches real Claude CLI output structure with nested message.content array of text blocks
- Used exclusively in @/tests/subprocess_test.rs and @/tests/conversation_rendering_test.rs to verify JSONL parsing logic
- Demonstrates the minimal contract: any process that outputs newline-delimited JSON to stdout works

**Instantiation Pattern** (@/src/main.rs:144-150):
```rust
fn get_backend(model: &Model) -> Box<dyn AgentBackend + Send> {
    match model.selected_agent_index {
        Some(0) => Box::new(ClaudeBackend::new()),
        Some(1) => Box::new(CodexBackend::new()),
        _ => Box::new(ClaudeBackend::new()), // Default
    }
}
```

### Things to Know

**Async Trait Pattern**:
- `#[async_trait]` macro is required because Rust doesn't natively support async functions in traits yet
- Macro transforms async trait method into one that returns a boxed Future
- All implementations must also use `#[async_trait]` macro before impl block

**Session Persistence Not Implemented**:
- Both ClaudeBackend and CodexBackend have session ID fields (session_id, thread_id)
- Fields are initialized to None and never updated during execution
- Backend structs are created fresh on each prompt submission, so even if fields were set, they wouldn't persist
- Future implementation would need to store session IDs in Model and pass to backend constructor

**JSONL Event Contract**:
- Backends must output newline-delimited JSON where each line has a "type" field
- @/src/main.rs parses stdout line-by-line via @/src/conversation.rs:parse_jsonl_event()
- Known types: "assistant" (nested message.content array), "system" (subtype and details), "result" (success/failure indication)
- Assistant messages follow Claude CLI format: `{"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}`
- Unknown types are wrapped in ConversationEvent::UnknownEvent with raw JSON for debugging
- Parse failures (malformed JSON) return None from parse_jsonl_event() - line is skipped

**Process Lifecycle**:
- Child processes are spawned via tokio::process::Command, which returns a tokio::process::Child
- stdout and stderr are piped (std::process::Stdio::piped()) so they can be read asynchronously
- Spawning happens in @/src/main.rs:spawn_and_stream() which is called from tokio::spawn, so it runs concurrently
- Process is waited on via child.wait().await in spawn_and_stream, blocking that task until exit
- No cancellation mechanism - child continues running even if user escapes streaming view

**Error Paths**:
- spawn_process() returns Result<Child> - spawn failure (CLI not found, permission denied) propagates to caller
- @/src/main.rs:spawn_and_stream() returns Result<()> and sends errors via mpsc channel as Message::Error
- stderr output is captured and wrapped in ConversationEvent::StderrOutput, sent as Message::StreamEvent for visibility with red styling

**Testing Strategy**:
- MockBackend allows testing entire JSONL parsing pipeline without external dependencies
- No integration tests with real claude/codex CLIs because they require API keys and authentication
- @/tests/subprocess_test.rs verifies MockBackend can spawn process and output is parseable JSON

Created and maintained by Nori.
