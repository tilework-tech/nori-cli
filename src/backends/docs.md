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
- **Stream completion behavior** (lines 129-134): When a ResultSummary event is parsed from JSONL stdout, the stream returns immediately without attempting to read stderr or wait for process exit. The Claude subprocess continues running in the background (stdout/stderr remain open), but the stream terminates as soon as semantic completion is signaled via the ResultSummary event. This prevents the backend from hanging indefinitely waiting for process cleanup while the UI transitions to Selection mode.

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
- **Stream completion**: The stream is driven by semantic completion signals (ResultSummary events from JSONL) rather than process exit status. The ClaudeBackend returns immediately upon receiving a ResultSummary, and @/src/main.rs:spawn_and_stream() terminates stream consumption immediately upon receiving ResultSummary, sending Message::StreamComplete to the UI. The subprocess may still be running in the background, but the stream is semantically complete from the user's perspective.

**Error Paths**:
- spawn_process() returns Result<Child> - spawn failure (CLI not found, permission denied) propagates to caller
- @/src/main.rs:spawn_and_stream() returns Result<()> and sends errors via mpsc channel as Message::Error
- stderr output is captured and wrapped in ConversationEvent::StderrOutput, sent as Message::StreamEvent for visibility with red styling

**Testing Strategy**:
- MockBackend allows testing entire JSONL parsing pipeline without external dependencies
- No integration tests with real claude/codex CLIs because they require API keys and authentication
- @/tests/subprocess_test.rs verifies MockBackend can spawn process and output is parseable JSON

Created and maintained by Nori.
