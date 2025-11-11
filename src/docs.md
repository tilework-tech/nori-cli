# Noridoc: src

Path: @/src

### Overview

Core application modules implementing the TUI's architecture: application state management (app.rs), UI rendering (ui.rs), backend abstractions (backends.rs), and the async event loop entry point (main.rs). Together these modules implement The Elm Architecture pattern for a responsive, chat-style terminal interface with conversation history and an overlay-based agent selector.

### How it fits into the larger codebase

- Called from @/src/main.rs which initializes the tokio runtime and terminal environment
- Exports public modules via @/src/lib.rs for use in @/tests/ integration tests
- Provides Model and Message types to @/tests/state_machine_test.rs for unit testing state transitions
- Provides AgentBackend trait and implementations to @/tests/subprocess_test.rs for testing JSONL parsing
- Backend implementations (@/src/backends/) are instantiated in @/src/main.rs based on user's agent selection

### Core Implementation

**Module Structure** (@/src/lib.rs):
```rust
pub mod app;          // Model, Message, and update logic
pub mod backends;     // AgentBackend trait and implementations
pub mod conversation; // JSONL parsing and event rendering
pub mod ui;           // Rendering functions for each mode
```

**Entry Point** (@/src/main.rs):
- `main()`: Sets up terminal (raw mode, Viewport::Inline(8)), runs async event loop, restores terminal on exit with cursor positioning to next line before disabling raw mode to ensure shell prompt appears cleanly below TUI content
- `run_app()`: Core event loop using tokio::select! to handle messages and render at ~30 fps interval
- `handle_event_simple()` / `handle_key_simple()`: Convert crossterm key events to Message based on current mode
- `get_backend()`: Factory function that returns appropriate backend (Claude or Codex) based on selected_agent_index
- `spawn_and_stream()`: Consumes backend stream using tokio::select! to multiplex stream consumption with cancellation signal - when cancelled, stream is dropped and child process cleanup happens via Drop semantics
- `wrap_text_to_width()`: Manual text wrapping function that splits Text into multiple Lines fitting within terminal width - required because `insert_before()` only handles single-line insertion and Ratatui's `Paragraph::wrap()` only applies during render phase

**State Management** (@/src/app.rs):
- `AppMode`: Enum with Selection/Input/Streaming states - now primarily tracks Streaming vs non-Streaming (simplified from screen-based modes)
- `Message`: Enum of all possible events that trigger state changes - includes `StreamEvent(ConversationEvent)` for backend events, `CancelStream` for interruption, `ToggleAgentRouter` for overlay, and `KeyPress` for textarea input
- `Model`: Struct holding all application state including `show_agent_router: bool` for overlay visibility, `response_events` vector for full conversation history, and `current_stream_token: Option<CancellationToken>` for tracking active stream
- `Model::update()`: Pure function that transitions state based on message - implements TEA "update" phase, with navigation/selection now gated by `show_agent_router` flag instead of mode
- SubmitInput handler (@/src/main.rs:113-188): For regular prompts (non-slash-commands), renders UserMessage to scrollback BEFORE backend availability check, captures textarea content, clears textarea immediately (before streaming begins), adds UserMessage to history, transitions to Streaming mode
- CancelStream handler: Calls token.cancel(), transitions to Selection mode, appends StreamCancelled event to history (textarea already cleared by SubmitInput)

**UI Rendering** (@/src/ui.rs):
- `render()`: Always renders chat view as base layer, then conditionally overlays agent router if `show_agent_router` is true
- `render_chat()`: Four-section vertical layout - Title bar showing selected agent, Messages area with full conversation history, Input textarea at bottom, and Instructions footer
- `render_agent_router_overlay()`: Renders agent list inside centered rectangle (60% width, 40% height) using Clear widget to blank underlying content
- `centered_rect()`: Helper for creating centered popup areas using nested Layout constraints

**Conversation Event Handling** (@/src/conversation.rs):
- `ConversationEvent` enum: Structured representation of backend JSONL events - includes UserMessage for chat history, AssistantMessage, SystemEvent, ResultSummary, StderrOutput, StreamCancelled for interruptions, UnknownEvent
- `parse_jsonl_event()`: Parses raw JSONL strings into ConversationEvent - handles Claude CLI event format with nested message.content arrays
- `render_event()`: Converts ConversationEvent into styled ratatui Lines - UserMessage renders with cyan `[user]` prefix, StreamCancelled renders "Interrupted" in red, other events render with type-specific prefixes and colors

**Backend Abstraction** (@/src/backends.rs):
- `AgentBackend` trait: `spawn_stream(prompt, cancel_token) -> Pin<Box<dyn Stream<Item = ConversationEvent>>>` and metadata methods
- Accepts CancellationToken for cooperative cancellation - backends receive token but child process cleanup happens via Drop
- Enables polymorphism for different CLI tools while maintaining same subprocess streaming interface
- Re-exports backend modules (claude, codex, mock) for external use

**Message Flow**:
```
User Input (keyboard)
  → Alt+A toggles show_agent_router overlay
  → Enter submits prompt → SubmitInput renders UserMessage to scrollback using render_event() + wrap_text_to_width() + terminal.insert_before(), adds UserMessage to history, creates CancellationToken, spawns stream
  → Esc during streaming → CancelStream triggers token.cancel()
  → EventStream task → Message (via mpsc channel)
  → run_app loop → Model::update()
  → render() displays chat + optional overlay on next tick

User Message Rendering (@/src/main.rs:140-156)
  → SubmitInput handler creates UserMessage event from prompt text
  → render_event() converts to styled Line with "[user]" prefix in cyan
  → wrap_text_to_width() splits into multiple Lines fitting terminal width
  → terminal.insert_before() called once per wrapped line → scrollback buffer accumulates all lines
  → User message appears in terminal scrollback BEFORE backend processing begins

Subprocess Output (JSONL)
  → spawn_and_stream task → tokio::select! on cancel_token.cancelled() vs stream.next()
  → parse_jsonl_event() → ConversationEvent (if stream continues)
  → Message::StreamEvent (via mpsc channel)
  → run_app loop → render_event() converts to styled Line → wrap_text_to_width() splits into multiple Lines
  → terminal.insert_before() called once per wrapped line → scrollback buffer accumulates all lines
  → Model::update() → accumulates in response_events
  → render_chat() maps all events via render_event() to styled Lines

Cancellation Path
  → User presses Esc → CancelStream message → token.cancel() called
  → spawn_and_stream's cancel branch fires → stream dropped → child process cleanup via Drop
  → StreamCancelled event added to history → UI returns to Selection mode
```

### Things to Know

**Terminal Cleanup Sequence** (@/src/main.rs:27-31):
- Cleanup happens in specific order: move cursor to next line, disable raw mode, call ratatui::restore()
- `MoveToNextLine(1)` from crossterm positions cursor below TUI content before raw mode is disabled
- Without cursor positioning, shell prompt would appear in middle of painted TUI area with Viewport::Inline(8)
- Must execute cursor command before disable_raw_mode() to ensure command is processed while terminal is still in raw mode

**Async Architecture**:
- Three concurrent tasks: event handler (reads keyboard), subprocess streaming (reads stdout/stderr), render interval (ticks at 30 fps)
- All tasks communicate via unbounded mpsc channel - sender is cloned for each task
- Event handler maintains local `current_mode` copy to avoid race conditions when converting events to messages
- Render task uses tokio::interval to decouple rendering from event/message frequency

**Mode Transitions in Event Handler** (@/src/main.rs:39-60):
- Event handler tracks mode locally via mode_rx channel receiving updates from main loop after each Model::update()
- Main loop sends updated mode after every state change to keep event handler in sync
- This prevents stale mode from causing incorrect event-to-message conversions
- Alt+A is handled globally (works regardless of mode) to toggle agent router overlay
- 'q' quit key respects mode - works in Selection/Streaming but types 'q' character in Input mode

**State Machine Invariants**:
- list_state always has Some(index) selected for agent router navigation
- Navigation (NextItem/PreviousItem) only updates list_state when `show_agent_router` is true
- KeyPress messages only update textarea when `show_agent_router` is false (input blocked when overlay open)
- response_events accumulates across all interactions - conversation history never cleared, preserves full chat context
- textarea is cleared exactly once per submission: in SubmitInput handler immediately after capturing user text, before transitioning to Streaming mode
- StreamComplete and CancelStream handlers do NOT clear textarea - it's already empty from SubmitInput
- SubmitInput only transitions to Streaming if textarea contains non-whitespace text
- User messages are rendered to terminal scrollback in SubmitInput handler BEFORE backend availability check and BEFORE slash command processing - ensures messages appear even if install prompt is shown or command execution fails

**Conversation Event Parsing** (@/src/conversation.rs):
- Parsing based on actual Claude CLI output format: `{"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}`
- Assistant messages extract all text blocks from content array and join with newlines
- System events check for `subtype` field and optional `cwd` or `session_id` details
- Result events check `subtype` for "success" vs other values to determine success boolean
- Stderr output wrapped in StderrOutput variant for consistent styling
- Malformed JSON or unparseable events return None from parse_jsonl_event()

**Event Rendering Styles** (@/src/conversation.rs):
- UserMessage: `[user]` prefix in bold cyan, followed by user's prompt text
- AssistantMessage: Plain white text, no prefix
- SystemEvent: `[system]` prefix in dim dark gray, subtype and details in dark gray
- ResultSummary: `[done]` prefix in bold green (success) or bold red (failure)
- StderrOutput: Red text, no prefix
- UnknownEvent: `[unknown]` prefix in yellow with raw JSON for debugging

**Error Display Strategy**:
- Errors don't transition mode - Model stays in Streaming so stderr output remains visible in conversation history
- Error message stored in Model::error_message for display in chat view
- User must manually press Esc to return to Selection mode after reading error
- Error events (StderrOutput) render in red within conversation history for visibility

**Rendering Performance**:
- Render interval is 33ms (~30 fps) regardless of event frequency
- ratatui only redraws changed terminal cells, so rapid renders are efficient
- Frame is mut reference, allowing widgets to modify cursor position during render

**Key Event Handling**:
- KeyEvent passed to textarea via Message::KeyPress only when `show_agent_router` is false
- textarea.input() handles cursor movement, text editing, newlines internally
- Alt+A globally toggles agent router overlay - handled before mode-specific logic in @/src/main.rs:117-121
- 'q' quits from Selection/Streaming modes but types 'q' character when in Input mode
- Alt+Enter submits because Ctrl+Enter doesn't work reliably across terminal emulators
- Esc during streaming sends CancelStream message to interrupt the stream
- Esc closes agent router overlay when open via ExitInputMode message

**Stream Cancellation Mechanism**:
- CancellationToken from tokio-util used for cooperative cancellation
- Token created in main loop when spawning stream task, stored in Model.current_stream_token
- spawn_and_stream uses tokio::select! with three branches: cancel signal, stream next, stream end
- When cancel branch fires, stream is dropped which closes file handles
- Child process cleanup happens via Drop trait implementation on the stream
- StreamCancelled event provides visual feedback in conversation history
- No explicit process killing - relies on Drop semantics and closed handles

**Text Wrapping for Scrollback** (@/src/main.rs:339-443):
- `wrap_text_to_width()` performs manual text wrapping before inserting into scrollback buffer
- **Why manual wrapping is required**: `insert_before()` captures only one line at a time, but Ratatui's `Paragraph::wrap()` applies during render phase after capture - this causes long lines to be truncated or overflow instead of wrapping
- **Algorithm**: Word-level wrapping with character-level fallback for extremely long words (JSON strings, URLs)
  - Splits text at word boundaries when possible to preserve readability
  - For words exceeding terminal width, falls back to character-by-character splitting
  - Preserves span styling across wrapped lines by creating new Spans with same style
  - Uses unicode-width crate for accurate width calculation (handles multi-byte UTF-8 characters)
- **Integration**: Used in two places - StreamEvent messages (@/src/main.rs:88-112) and UserMessage rendering in SubmitInput handler (@/src/main.rs:140-156) - gets terminal width (minus 2 for borders), converts rendered Line to Text, wraps via `wrap_text_to_width()`, then calls `insert_before()` separately for each wrapped line
- **Edge cases**: Returns original line if width < 10, preserves empty lines, ensures at least one line is always returned
- **Dependency**: Added unicode-width = "0.2" to Cargo.toml for UnicodeWidthStr trait

**User Message Scrollback Rendering** (@/src/main.rs:140-156):
- User messages are rendered to terminal scrollback in SubmitInput handler, not in Model::update()
- **Why rendering happens in main.rs**: Model::update() is pure state management with no side effects - it cannot call terminal.insert_before() which requires mutable terminal reference only available in main event loop
- **Timing is critical**: User message must appear BEFORE backend availability check so message is visible even if install prompt appears
- **Prevents message loss**: Without scrollback rendering, user's text disappears from textarea on submit but never appears in conversation history above TUI - only stored in response_events vector but not visible to user
- **Follows existing pattern**: UserMessage rendering uses exact same pipeline as StreamEvent rendering (lines 88-112) - both call render_event(), wrap_text_to_width(), and terminal.insert_before() in identical sequence
- **Slash command handling**: Slash commands bypass user message rendering entirely - they are not stored as conversation events and should not appear in chat history

Created and maintained by Nori.
