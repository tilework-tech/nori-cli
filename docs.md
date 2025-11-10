# Noridoc: agent-router-tui

Path: @/

### Overview

A terminal user interface (TUI) application that routes user prompts to different AI coding agent CLIs (Claude Code and GPT Codex). The application provides an interactive interface for selecting agents, entering prompts, and viewing streaming responses in real-time.

### How it fits into the larger codebase

- Standalone TUI application living in a git worktree for isolated development
- Located at @/.worktrees/agent-router-tui, separated from the main repository to avoid polluting main branch history during development
- Uses ratatui framework for terminal rendering, establishing a pattern for future TUI-based tools in the Nori ecosystem
- Demonstrates subprocess integration pattern for wrapping existing CLI tools (claude, codex) with a better UX layer
- Self-contained with no dependencies on other parts of the monorepo - all code lives within this worktree

### Core Implementation

**Entry Point**: @/src/main.rs contains the async main loop that:
1. Initializes terminal with `ratatui::init()` and enables raw mode for immediate key capture
2. Spawns an async event handling task that continuously reads keyboard events via crossterm's EventStream
3. Runs a tokio::select! loop that handles both incoming messages (from event handler and subprocess) and periodic rendering at ~30 fps
4. Delegates subprocess spawning to backend implementations when user submits a prompt
5. Restores terminal on shutdown via `ratatui::restore()` and `LeaveAlternateScreen`

**Architecture Pattern**: The Elm Architecture (TEA)
- Model (@/src/app.rs): Application state including current mode, selected agent, textarea content, and response accumulation
- Message (@/src/app.rs): Enum of all possible events/actions (navigation, input, streaming, errors)
- Update (@/src/app.rs): `Model::update()` handles state transitions based on messages
- Render (@/src/ui.rs): Mode-specific rendering functions that draw UI from current model state

**State Machine**: Three modes with specific transitions
```
Selection (agent list)
    → Enter → Input (prompt entry)
    → Alt+Enter → Streaming (receiving response)
    → StreamComplete/Esc → Selection

Input → Esc → Selection
```

**Subprocess Integration**:
- Backend trait (@/src/backends.rs) defines `spawn_process()` for launching agent CLIs
- Implementations spawn processes with stdout/stderr piped (@/src/backends/claude.rs, @/src/backends/codex.rs)
- Main loop in @/src/main.rs:spawn_and_stream() reads JSONL output via BufReader and parses events
- Events are sent through mpsc channel as Message::StreamChunk to update UI in real-time
- Both stdout (for JSON events) and stderr (for error messages) are captured concurrently

**Dependencies** (@/Cargo.toml):
- ratatui 0.29.0: TUI framework
- tokio (full features): Async runtime for subprocess management and concurrent I/O
- crossterm 0.28.1 (event-stream feature): Terminal manipulation and async event handling
- tui-textarea 0.7: Multi-line text input widget
- serde + serde_json: JSONL parsing
- color-eyre: Error reporting

### Things to Know

**Key Invariants**:
- Terminal must be restored (raw mode disabled, alternate screen exited) on any exit path to avoid broken terminal state
- Event handler task updates local mode tracking to prevent race conditions when converting events to messages
- Response text accumulates across stream chunks - never cleared until next prompt submission

**Subprocess Lifecycle**:
- Child processes are spawned when user submits prompt via tokio::spawn
- stdout/stderr are read line-by-line in separate tokio tasks to avoid blocking
- Process wait() happens in the spawn_and_stream task, followed by StreamComplete or Error message
- Currently no process cancellation - pressing Esc during streaming only changes UI mode, doesn't kill subprocess

**JSONL Event Parsing** (@/src/main.rs:152-234):
- Each line of stdout must be valid JSON with a "type" field
- Supported event types: "agent_message" (text content), "file_change" (file path), "command_execution" (shell command)
- Unknown event types are shown as raw JSON for debugging
- Parse failures are silently skipped - line is not displayed

**Error Handling**:
- Non-zero exit status triggers Message::Error with helpful instructions
- Error message is stored in Model::error_message and displayed in streaming view
- Application stays in Streaming mode when error occurs so user can read stderr output
- User must press Esc to return to Selection mode after error

**Testing Strategy** (@/tests/):
- State machine tests use Model::update() directly without subprocess spawning
- Subprocess tests use MockBackend which wraps `printf` to output test JSONL
- No integration tests with actual claude/codex CLIs to avoid authentication requirements in CI

**Current Limitations**:
- Session resumption fields exist (session_id, thread_id) but are not persisted between runs
- No scrolling in response view - long outputs overflow viewport
- Ctrl+Enter keybinding doesn't work reliably across terminals, so Alt+Enter is used for submit
- Process cancellation not implemented - subprocess continues running even if user presses Esc

Created and maintained by Nori.
