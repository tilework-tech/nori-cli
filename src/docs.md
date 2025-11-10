# Noridoc: src

Path: @/src

### Overview

Core application modules implementing the TUI's architecture: application state management (app.rs), UI rendering (ui.rs), backend abstractions (backends.rs), and the async event loop entry point (main.rs). Together these modules implement The Elm Architecture pattern for a responsive, mode-based terminal interface.

### How it fits into the larger codebase

- Called from @/src/main.rs which initializes the tokio runtime and terminal environment
- Exports public modules via @/src/lib.rs for use in @/tests/ integration tests
- Provides Model and Message types to @/tests/state_machine_test.rs for unit testing state transitions
- Provides AgentBackend trait and implementations to @/tests/subprocess_test.rs for testing JSONL parsing
- Backend implementations (@/src/backends/) are instantiated in @/src/main.rs based on user's agent selection

### Core Implementation

**Module Structure** (@/src/lib.rs):
```rust
pub mod app;      // Model, Message, and update logic
pub mod backends; // AgentBackend trait and implementations
pub mod ui;       // Rendering functions for each mode
```

**Entry Point** (@/src/main.rs):
- `main()`: Sets up terminal (raw mode, alternate screen), runs async event loop, restores terminal on exit
- `run_app()`: Core event loop using tokio::select! to handle messages and render at ~30 fps interval
- `handle_event_simple()` / `handle_key_simple()`: Convert crossterm key events to Message based on current mode
- `get_backend()`: Factory function that returns appropriate backend (Claude or Codex) based on selected_agent_index
- `spawn_and_stream()`: Spawns subprocess, reads stdout/stderr concurrently, parses JSONL, sends StreamChunk messages

**State Management** (@/src/app.rs):
- `AppMode`: Enum representing three UI states (Selection, Input, Streaming)
- `Message`: Enum of all possible events that trigger state changes
- `Model`: Struct holding all application state (current_mode, list_state, textarea, response_text, etc.)
- `Model::update()`: Pure function that transitions state based on message - implements TEA "update" phase

**UI Rendering** (@/src/ui.rs):
- `render()`: Dispatches to mode-specific render functions based on model.current_mode
- `render_selection()`: Draws agent list with ListState for navigation, shows error popup if present
- `render_input()`: Draws TextArea widget for multi-line prompt entry
- `render_streaming()`: Displays accumulated response_text, changes title color to red if error occurred
- `centered_rect()`: Helper for creating centered popup areas using Layout constraints

**Backend Abstraction** (@/src/backends.rs):
- `AgentBackend` trait: Async `spawn_process(prompt) -> Result<Child>` and `name() -> &str`
- Enables polymorphism for different CLI tools while maintaining same subprocess streaming interface
- Re-exports backend modules (claude, codex, mock) for external use

**Message Flow**:
```
User Input (keyboard)
  → EventStream task → Message (via mpsc channel)
  → run_app loop → Model::update()
  → render() on next tick

Subprocess Output (JSONL)
  → spawn_and_stream task → Message::StreamChunk (via mpsc channel)
  → run_app loop → Model::update()
  → render() on next tick
```

### Things to Know

**Async Architecture**:
- Three concurrent tasks: event handler (reads keyboard), subprocess streaming (reads stdout/stderr), render interval (ticks at 30 fps)
- All tasks communicate via unbounded mpsc channel - sender is cloned for each task
- Event handler maintains local `current_mode` copy to avoid race conditions when converting events to messages
- Render task uses tokio::interval to decouple rendering from event/message frequency

**Mode Transitions in Event Handler** (@/src/main.rs:42-62):
- Event handler tracks mode locally because Model lives in main loop and isn't accessible from spawned task
- After sending message, handler updates local mode based on message type to stay in sync
- This prevents stale mode from causing incorrect event-to-message conversions
- Example: After sending Message::SelectItem, mode becomes AppMode::Input so next Esc key generates ExitInputMode

**State Machine Invariants**:
- Selection mode: list_state always has Some(index) selected, never None
- Input mode: selected_agent_index is always Some after transitioning from Selection
- Streaming mode: response_text accumulates and is never cleared (grows indefinitely for now)
- textarea is reset on StreamComplete to clear prompt for next submission

**Error Display Strategy** (@/src/ui.rs:119-151):
- Errors don't transition mode - Model stays in Streaming so stderr output remains visible
- Title bar changes to "Error - See details below" in red color
- Instructions section shows full error message with wrap enabled
- User must manually press Esc to return to Selection mode after reading error

**Rendering Performance**:
- Render interval is 33ms (~30 fps) regardless of event frequency
- ratatui only redraws changed terminal cells, so rapid renders are efficient
- Frame is mut reference, allowing widgets to modify cursor position during render

**Key Event Handling**:
- Input mode passes KeyEvent directly to textarea via Message::KeyPress
- textarea.input() handles cursor movement, text editing, newlines internally
- 'q' quits from any mode except Input (where it types 'q')
- Alt+Enter submits because Ctrl+Enter doesn't work reliably across terminal emulators

Created and maintained by Nori.
