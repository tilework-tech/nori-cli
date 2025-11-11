# Noridoc: agent-router-tui

Path: @/

### Overview

A terminal user interface (TUI) application that routes user prompts to different AI coding agent CLIs (Claude Code and GPT Codex). The application provides a chat-style interface where conversation history is always visible, with an overlay for agent selection and an input field at the bottom for natural interaction.

### How it fits into the larger codebase

- Standalone TUI application living in a git worktree for isolated development
- Located at @/.worktrees/agent-router-tui, separated from the main repository to avoid polluting main branch history during development
- Uses ratatui framework for terminal rendering, establishing a pattern for future TUI-based tools in the Nori ecosystem
- Demonstrates subprocess integration pattern for wrapping existing CLI tools (claude, codex) with a better UX layer
- Self-contained with no dependencies on other parts of the monorepo - all code lives within this worktree

### Core Implementation

**Entry Point**: @/src/main.rs contains the async main loop that:
1. Initializes terminal with `ratatui::init_with_options()` using Viewport::Inline(8) and enables raw mode for immediate key capture
2. Spawns an async event handling task that continuously reads keyboard events via crossterm's EventStream
3. Runs a tokio::select! loop that handles both incoming messages (from event handler and subprocess) and periodic rendering at ~30 fps
4. Delegates subprocess spawning to backend implementations when user submits a prompt
5. Restores terminal on shutdown: moves cursor to next line, disables raw mode, calls `ratatui::restore()` - cursor positioning ensures shell prompt appears below TUI content instead of in the middle

**Architecture Pattern**: The Elm Architecture (TEA)
- Model (@/src/app.rs): Application state including overlay visibility (`show_agent_router`), selected agent, textarea content, and full conversation history in `response_events`
- Message (@/src/app.rs): Enum of all possible events/actions (navigation, input, streaming, overlay toggle, errors)
- Update (@/src/app.rs): `Model::update()` handles state transitions - navigation gated by overlay state, input blocked when overlay open
- Render (@/src/ui.rs): Layered rendering with chat view as base and optional agent router overlay on top

**UI Layout**: Chat-based interface with persistent conversation
```
┌─ Title (selected agent) ─────────────┐
│                                       │
├─ Messages (conversation history) ────┤
│ [user] What is the weather?           │
│ The weather is...                     │
│ [user] Tell me more                   │
│ ...streaming response...              │
├─ Input (textarea at bottom) ──────────┤
│ Type your message here...             │
├─ Instructions ────────────────────────┤
│ Enter: send | /switch-model: agents | q   │
└───────────────────────────────────────┘

/switch-model overlays agent selector (60% width, 40% height centered)
```

**Subprocess Integration**:
- Backend trait (@/src/backends.rs) defines `spawn_stream()` for launching agent CLIs and streaming events
- Implementations spawn processes with stdout/stderr piped (@/src/backends/claude.rs, @/src/backends/codex.rs)
- Main loop in @/src/main.rs:spawn_and_stream() uses tokio::select! to multiplex stream consumption with cancellation
- CancellationToken from tokio-util enables cooperative cancellation - when token fires, stream is dropped
- Events are sent through mpsc channel as Message::StreamEvent to update UI in real-time
- Both stdout (for JSON events) and stderr (for error messages) are captured concurrently

**Dependencies** (@/Cargo.toml):
- ratatui 0.29.0: TUI framework
- tokio (full features): Async runtime for subprocess management and concurrent I/O
- tokio-util: Provides CancellationToken for cooperative cancellation
- crossterm 0.28.1 (event-stream feature): Terminal manipulation and async event handling
- tui-textarea 0.7: Multi-line text input widget
- serde + serde_json: JSONL parsing
- color-eyre: Error reporting

### Things to Know

**Key Invariants**:
- Terminal must be restored in correct order (cursor positioning -> disable raw mode -> ratatui::restore()) on any exit path to avoid broken terminal state or mispositioned cursor
- Cursor must be moved to next line before disabling raw mode when using Viewport::Inline(8) to ensure shell prompt appears cleanly below TUI content
- Event handler task receives mode updates via channel to prevent race conditions when converting events to messages
- Conversation history (`response_events`) accumulates indefinitely - includes both UserMessage and assistant responses, never cleared
- Navigation and text input are mutually exclusive based on `show_agent_router` flag - overlay blocks input, chat blocks navigation

**Subprocess Lifecycle**:
- Child processes are spawned when user submits prompt via tokio::spawn
- CancellationToken created and stored in Model when stream starts
- stdout/stderr are read line-by-line in separate tokio tasks to avoid blocking
- spawn_and_stream uses tokio::select! to multiplex stream consumption with cancellation signal
- Pressing Esc during streaming triggers CancelStream message which calls token.cancel()
- When cancel fires, stream is dropped, closing file handles and triggering child process cleanup via Drop
- Process wait() happens naturally when stream completes or is cancelled

**JSONL Event Parsing** (@/src/main.rs:152-234):
- Each line of stdout must be valid JSON with a "type" field
- Supported event types: "agent_message" (text content), "file_change" (file path), "command_execution" (shell command)
- Unknown event types are shown as raw JSON for debugging
- Parse failures are silently skipped - line is not displayed

**Error Handling**:
- Non-zero exit status triggers Message::Error with helpful instructions
- Error message is stored in Model::error_message and displayed in chat view
- Application stays in Streaming mode when error occurs so user can read stderr output in conversation history
- StderrOutput events render in red within conversation history for visibility
- User must press Esc to return to Selection mode after error

**Testing Strategy** (@/tests/):
- State machine tests use Model::update() directly without subprocess spawning
- Subprocess tests use MockBackend which wraps `printf` to output test JSONL
- No integration tests with actual claude/codex CLIs to avoid authentication requirements in CI

**Current Limitations**:
- Session resumption fields exist (session_id, thread_id) but are not persisted between runs
- Terminal scrolling handles long conversations - no custom scroll implementation yet
- Conversation history grows indefinitely in memory - no pagination or pruning
- Stream cancellation relies on Drop semantics - no explicit process.kill() called

**UI/UX Design Decisions**:
- Chat view always visible to maintain context across interactions
- Input at bottom matches familiar chat application UX patterns
- Agent router as overlay avoids disrupting conversation flow
- UserMessage events added to history on submit, before agent response streams in
- /switch-model command for agent switching works from the input prompt

Created and maintained by Nori.
