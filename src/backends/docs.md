# Noridoc: src/backends

Path: @/src/backends

### Overview

Backend implementations for spawning and interacting with different AI coding agent CLIs. Defines the AgentBackend trait and provides concrete implementations for Claude Code (claude.rs), GPT Codex (codex.rs), ACP-based backends (codex_acp.rs, claude_code_acp.rs, gemini_acp.rs), and a mock backend for testing (mock.rs).

**NEW (Phase 1 Complete):** ACP (Agent Client Protocol) integration added in @/src/acp_runner.rs. This provides a standardized protocol-based approach that will eventually replace custom backend implementations. The system now includes three ACP-based backends: Codex ACP and Claude Code ACP from @zed-industries npm packages, plus Gemini ACP from @google/gemini-cli. See ACP Integration section below.

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
- Called dynamically via BACKEND_OPTIONS availability check functions
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
- Uses ACP protocol via mock_acp_agent binary (wraps AcpAgentRunner)
- Binary built from @/mock-acp-agent/src/main.rs using agent-client-protocol crate
- Implements full Agent trait with initialize, new_session, prompt methods
- Outputs test messages via SessionUpdate notifications
- Demonstrates real ACP protocol handshake and communication
- Used in @/tests/subprocess_test.rs and @/tests/acp_runner_test.rs
- Pattern: Wraps AcpAgentRunner just like CodexAcpBackend

**Codex ACP Backend** (@/src/backends/codex_acp.rs):
- Wraps AcpAgentRunner to launch @zed-industries/codex-acp via bunx/npx
- JavaScript runtime detection: Prioritizes Bun (bunx) over npm (npx)
- Command construction: `bunx @zed-industries/codex-acp` or `npx @zed-industries/codex-acp`
- No direct subprocess management - delegates entirely to AcpAgentRunner
- Runtime detection cached at backend creation via javascript_runtime module
- command_name: Returns "bunx" or "npx" based on detected runtime
- install_url: "https://www.npmjs.com/package/@zed-industries/codex-acp"
- install_command: `npm install -g @zed-industries/codex-acp`
- Error handling: Emits SystemEvent if no JavaScript runtime available

**Claude Code ACP Backend** (@/src/backends/claude_code_acp.rs):
- Wraps AcpAgentRunner to launch @zed-industries/claude-code-acp via bunx/npx
- Identical architecture to CodexAcpBackend - follows exact same pattern
- JavaScript runtime detection: Prioritizes Bun (bunx) over npm (npx)
- Command construction: `bunx @zed-industries/claude-code-acp` or `npx @zed-industries/claude-code-acp`
- No direct subprocess management - delegates entirely to AcpAgentRunner
- Runtime detection cached at backend creation via javascript_runtime module
- command_name: Returns "bunx" or "npx" based on detected runtime
- install_url: "https://www.npmjs.com/package/@zed-industries/claude-code-acp"
- install_command: `npm install -g @zed-industries/claude-code-acp`
- Error handling: Emits SystemEvent if no JavaScript runtime available

**Gemini ACP Backend** (@/src/backends/gemini_acp.rs):
- Wraps AcpAgentRunner to launch @google/gemini-cli via bunx/npx
- Identical architecture to CodexAcpBackend and ClaudeCodeAcpBackend - follows exact same pattern
- JavaScript runtime detection: Prioritizes Bun (bunx) over npm (npx)
- Command construction: `bunx @google/gemini-cli` or `npx @google/gemini-cli`
- No direct subprocess management - delegates entirely to AcpAgentRunner
- Runtime detection cached at backend creation via javascript_runtime module
- command_name: Returns "bunx" or "npx" based on detected runtime
- install_url: "https://www.npmjs.com/package/@google/gemini-cli"
- install_command: `npm install -g @google/gemini-cli`
- Error handling: Emits SystemEvent if no JavaScript runtime available

**JavaScript Runtime Detection** (@/src/backends/javascript_runtime.rs):
- Detects Bun or npm/Node.js availability on system
- Detection order: bun/bunx → npm/npx → None
- JavaScriptRuntime enum: Bun | Npm
- Runtime.command() returns executable name: "bunx" or "npx"
- Used by CodexAcpBackend to determine package execution method
- Bun preferred for faster package loading (downloads on-demand)
- npm requires global package installation but more widely available

**Instantiation Pattern** (@/src/app.rs):
- `BACKEND_OPTIONS`: Centralized constant containing all backend metadata and factory functions
- `BackendOption` struct: Contains backend name, availability check function, and factory function
- `get_backend()`: Uses BACKEND_OPTIONS to instantiate the appropriate backend based on selected_agent_index
- Eliminates hardcoded match statements and ensures consistent backend ordering across the application
- Backend ordering: Claude Code ACP (0), Codex ACP (1), Gemini ACP (2), Mock ACP Agent (3), Claude commandline SDK (4)

**Centralized Backend Ordering System**:
- `BACKEND_OPTIONS` constant in @/src/app.rs serves as single source of truth for backend metadata
- Contains `BackendOption` structs with name, availability check, and factory function for each backend
- Eliminates maintenance issues from disconnected sources (separate `agents` and `backend_availability` vectors)
- Ensures consistency when adding/removing/modifying backends
- Used by UI rendering (@/src/ui.rs), backend instantiation (@/src/app.rs), and testing (@/tests/model_backend_ordering_test.rs)

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
- Message handlers: ShowInstallPrompt, NavigateInstallChoiceNext, NavigateInstallChoicePrevious, ConfirmInstall, CancelInstall, InstallationComplete
- ConfirmInstall: Runs installation command if RunInstallation selected, opens URL if OpenInstallPage selected, closes prompt if Cancel
- InstallationComplete: Updates backend availability status when installation completes successfully

**Visual Indication** (@/src/ui.rs):
- BACKEND_OPTIONS availability check functions determine installation status for each agent
- Checked dynamically when needed rather than cached in Model
- Agent router displays unavailable backends with "[Not Installed]" suffix in dark gray
- Provides visual feedback before user attempts to use unavailable backend

### Things to Know

**libc Dependency for Process Cleanup**:
- Added `libc = "0.2"` dependency in @/Cargo.toml for synchronous process termination
- Required because Drop trait must be synchronous but `tokio::process::Child::kill()` is async
- Cannot use `tokio::runtime::Handle::block_on()` inside Drop when already in a tokio runtime (causes panic)
- Direct syscall via `libc::kill(pid, SIGTERM)` provides synchronous, reliable cleanup without runtime issues
- Platform-specific: Unix-only implementation via `#[cfg(unix)]`, Windows would need different approach
- Trade-off: Small unsafe block in Drop for guaranteed resource cleanup vs potential process leaks

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

**ACP Process Cleanup (RAII Pattern)** (@/src/acp_runner.rs:362-403):
- AcpAgentRunner implements Drop trait to guarantee process termination regardless of how runner is disposed
- When runner is dropped (normal drop, panic, stream reuse), Drop implementation sends SIGTERM to child process via `libc::kill`
- Uses synchronous syscall (`libc::kill`) instead of async `tokio::process::Child::kill()` because Drop must be synchronous and cannot use `block_on` inside an existing tokio runtime
- Process cleanup happens on three scenarios: (1) runner drop at end of scope, (2) spawning new stream while old process still running, (3) initialization failure during spawn_stream
- Unix-only implementation using `#[cfg(unix)]` - Windows would require different approach
- Includes tracing logs for process cleanup events (pid, agent name, success/failure)
- Critical for preventing orphaned agent processes that would persist after application exit
- `agent_pid()` method (@/src/acp_runner.rs:265-269) exposes process PID for testing - returns `Option<u32>`

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
   - `request_permission()` - Auto-approves by selecting first "allow" option (AllowOnce/AllowAlways) - logs permission requests and granted options at debug level
   - `session_notification()` - Forwards SessionUpdate to event stream via mpsc channel - logs all session updates at debug level
   - `read_text_file()` - Reads from working directory, handles absolute/relative paths - logs file path at debug level, logs failures at warn level
   - `write_text_file()` - Writes with auto-created parent directories - logs file path and content length at debug level, logs failures at warn level
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

**Structured Tracing** (@/src/acp_runner.rs):
- ACP runner includes comprehensive tracing instrumentation using Rust's `tracing` crate
- **Lifecycle events** (info level):
  - Connection initialization start: "Starting ACP connection initialization"
  - Successful initialization with protocol version and agent info
  - Session creation with session ID
  - Prompt completion with stop reason
- **Detailed events** (debug level):
  - Initialize request with protocol version
  - Session creation request with working directory
  - Prompt sending with session ID and prompt length
  - All session updates (SessionUpdate messages from protocol)
  - Permission requests and granted options
  - File operations (read/write) with paths and content lengths
- **Error events** (warn level):
  - Initialization failures with error message
  - Initialization timeouts (30s)
  - Unsupported protocol versions
  - Session creation failures
  - Prompt execution failures
  - File operation failures (read/write)
- **Log output**: Writes to `~/.nori-cli/logs/` when disk logging enabled in TuiApp
- **Purpose**: Debug ACP transport issues, understand lifecycle timing, investigate file operations without disrupting TUI

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
