# Noridoc: src/backends

Path: @/src/backends

### Overview

Backend implementations for spawning and interacting with different AI coding agent CLIs. Defines the AgentBackend trait and provides concrete implementations for Claude Code (claude.rs), GPT Codex (codex.rs), and a mock backend for testing (mock.rs).

**NEW (Phase 1 Complete):** ACP (Agent Client Protocol) integration added in @/src/acp_runner.rs. This provides a standardized protocol-based approach that will eventually replace custom backend implementations. See ACP Integration section below.

### How it fits into the larger codebase

- Trait definition and implementations are re-exported via @/src/backends.rs module declaration
- Called from @/src/main.rs:get_backend() which selects backend based on user's agent choice
- @/src/main.rs:spawn_and_stream() receives trait object (Box<dyn AgentBackend>) and calls spawn_process()
- MockBackend is used by @/tests/subprocess_test.rs to avoid requiring actual CLI installations in CI
- All backends return tokio::process::Child with piped stdout/stderr for JSONL parsing

### Core Implementation

**Trait Definition** (@/src/backends.rs):
```rust
pub trait AgentBackend {
    fn spawn_stream(
        &self,
        prompt: String,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = ConversationEvent> + Send>>;
    fn name(&self) -> &str;
    fn command_name(&self) -> &str;
    fn install_url(&self) -> &str;
}

pub fn is_available(command: &str) -> bool {
    which::which(command).is_ok()
}
```
- spawn_stream now accepts CancellationToken for cooperative cancellation
- Backends receive token but may not actively use it - cancellation happens when caller drops the returned stream

**Backend Availability Checking**:
- `is_available()` function checks if a command exists in PATH using the `which` crate
- Called at Model initialization to populate `backend_availability` vec
- Called before spawning to proactively detect missing backends
- Returns true if command found, false otherwise (cross-platform via which crate)

**Claude Code Backend** (@/src/backends/claude.rs):
- Command: `claude --print --output-format stream-json --include-partial-messages --verbose <prompt>`
- `--verbose` is required when using stream-json with --print mode
- Session resumption: Includes `--resume <session_id>` if ClaudeBackend.session_id is Some
- session_id field exists but is never populated - prepared for future persistence feature
- Spawns with stdout/stderr piped to enable JSONL event streaming back to main loop
- Accepts cancel_token parameter but doesn't actively use it - cancellation handled by caller dropping stream
- command_name: "claude"
- install_url: "https://code.claude.com"
- Spawn error handling: ErrorKind::NotFound yields SystemEvent with install message

**GPT Codex Backend** (@/src/backends/codex.rs):
- Command: `codex exec --json <prompt>`
- Session resumption: Prepends `resume <thread_id>` argument if CodexBackend.thread_id is Some
- thread_id field exists but is never populated - infrastructure for multi-turn conversations
- Headless mode via `exec` subcommand for non-interactive operation
- Always uses --json flag to get structured event output
- Accepts cancel_token parameter but doesn't actively use it - cancellation handled by caller dropping stream
- command_name: "codex"
- install_url: "https://developers.openai.com/codex/cli/"
- Spawn error handling: ErrorKind::NotFound yields SystemEvent with install message

**Mock Backend** (@/src/backends/mock.rs):
- Uses `printf` shell command to output hardcoded JSONL without requiring agent CLI installation
- Outputs two test events in actual Claude CLI format: `{"type":"assistant","message":{"content":[{"type":"text","text":"Hello from mock"}]}}`
- Format matches real Claude CLI output structure with nested message.content array of text blocks
- Accepts cancel_token parameter but doesn't use it - test streams complete immediately
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

### Installation Prompting

**User Experience Flow**:
1. User selects backend in agent router (via Alt+A)
2. User submits prompt (via Alt+Enter)
3. @/src/main.rs checks if backend command is available using `is_available(backend.command_name())`
4. If not available: ShowInstallPrompt message sent with backend name, install URL, and optional install_cmd
5. Install prompt fullscreen overlay appears (@/src/ui.rs:render_install_prompt_fullscreen)
6. User sees 2-3 options depending on whether install_cmd exists:
   - With install_cmd: "Run Installation", "Open Installation Page", "Cancel"
   - Without install_cmd: "Open Installation Page", "Cancel"
7. User can navigate options with arrow keys (up/down or j/k vim-style)
8. Enter on "Run Installation" executes install_cmd in background and shows progress
9. Enter on "Open Installation Page" opens URL in default browser via `opener` crate
10. Enter on "Cancel" or Esc closes prompt and returns to selection mode

**State Management** (@/src/app.rs):
- Model tracks `show_install_prompt`, `install_prompt_backend`, `install_prompt_url`, `install_prompt_cmd`, `install_prompt_choice`
- InstallChoice enum: RunInstallation | OpenInstallPage | Cancel
- Navigation: NavigateInstallChoiceNext (down arrow) and NavigateInstallChoicePrevious (up arrow) for directional cycling
- Context-aware cycling: When install_cmd exists, all 3 options are available; otherwise only OpenInstallPage and Cancel
- Default selection: RunInstallation when install_cmd exists, OpenInstallPage otherwise
- Message handlers: ShowInstallPrompt, NavigateInstallChoiceNext, NavigateInstallChoicePrevious, ConfirmInstall, CancelInstall
- ConfirmInstall: Runs installation command if RunInstallation selected, opens URL if OpenInstallPage selected, closes prompt if Cancel

**Visual Indication** (@/src/ui.rs):
- Model.backend_availability vec tracks installation status for each agent
- Checked once at startup in Model::default()
- Agent router displays unavailable backends with "[Not Installed]" suffix in dark gray
- Provides visual feedback before user attempts to use unavailable backend

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
- Stream consumption multiplexed with cancellation signal via tokio::select! in spawn_and_stream
- When cancellation fires, stream is dropped which closes file handles and triggers child process cleanup via Drop
- No explicit process.kill() - relies on Drop semantics and closed handles for cleanup

**Error Paths**:
- spawn_process() returns Result<Child> - spawn failure (CLI not found, permission denied) propagates to caller
- @/src/main.rs:spawn_and_stream() returns Result<()> and sends errors via mpsc channel as Message::Error
- stderr output is captured and wrapped in ConversationEvent::StderrOutput, sent as Message::StreamEvent for visibility with red styling

**Testing Strategy**:
- MockBackend allows testing entire JSONL parsing pipeline without external dependencies
- No integration tests with real claude/codex CLIs because they require API keys and authentication
- @/tests/subprocess_test.rs verifies MockBackend can spawn process and output is parseable JSON

### ACP Integration (Phase 1 - Foundation Complete)

**Location**: @/src/acp_runner.rs

The Agent Client Protocol (ACP) is a standardized protocol for communication between code editors and AI coding agents, similar to how LSP standardized language server integration. This integration will eventually replace the custom per-agent implementations above.

**What's Been Implemented**:

1. **Extended ConversationEvent Enum** (@/src/conversation.rs):
   - `ToolCallStarted` - When agent initiates a tool call (edit, write, bash, etc.)
   - `ToolCallProgress` - Tool execution progress with status and optional content
   - `AgentPlan` - Agent's multi-step execution plan
   - `AgentThinking` - Agent's internal reasoning/thought process
   - `PlanEntry` - Individual step with content, status (pending/in_progress/completed), priority

2. **Event Translation Layer** (`translate_session_update` function):
   - Converts ACP `SessionUpdate` → `ConversationEvent`
   - Handles all message chunk types (agent, user, thought)
   - Maps tool call lifecycle (pending → in_progress → completed/failed)
   - Translates plan entries with priority levels
   - Returns `None` for non-text content (images, audio)
   - 7 passing unit tests covering all translation paths

3. **AcpClientHandler** - Full `Client` trait implementation:
   - `request_permission()` - Auto-approves by selecting first "allow" option (AllowOnce/AllowAlways)
   - `session_notification()` - Forwards SessionUpdate to event stream via mpsc channel
   - `read_text_file()` - Reads from working directory, handles absolute/relative paths
   - `write_text_file()` - Writes with auto-created parent directories
   - Terminal methods blocked - returns `method_not_found` errors (security requirement)
   - Uses cancellation token to return `Cancelled` outcome if session cancelled mid-flight

**Architecture**:
```
User Prompt → AcpAgentRunner::spawn_stream() → JSON-RPC over stdio → Agent Process
                         ↓
              AcpClientHandler (implements Client trait)
                         ↓
              Handles file reads/writes, permissions
                         ↓
              SessionUpdate → translate_session_update → ConversationEvent → UI
```

**What's NOT Yet Implemented** (See @/ACP_IMPLEMENTATION_PLAN.md):
- `AcpAgentRunner::spawn_stream()` - Core method to spawn agent and manage JSON-RPC lifecycle
- JSON-RPC transport layer over stdin/stdout
- Initialize handshake with capability negotiation
- Session creation and prompt sending
- Integration into main.rs (still using old backends)
- Agent configurations (CLAUDE_CONFIG, CODEX_CONFIG)

**Benefits of ACP**:
- No more custom JSONL parsing per agent
- Standardized tool call visualization
- Agent plans visible to user
- File operations auto-approved (simpler UX)
- Any ACP-compliant agent works (Claude Code, Codex, Goose, etc.)
- Protocol-level cancellation support

**Migration Strategy**:
- Keep old AgentBackend trait during transition
- Add `--use-acp` flag to test new implementation
- Make ACP default after thorough testing
- Remove old backends in future release

**Testing**:
- Unit tests for event translation ✅ (7 tests, all passing)
- Client handler tested via compilation ✅
- Integration tests with mock agent (TODO)
- Manual testing with Claude Code (TODO)

Created and maintained by Nori.
