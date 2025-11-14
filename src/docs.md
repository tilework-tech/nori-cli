# Noridoc: src

Path: @/src

### Overview

Core application modules implementing the TUI's architecture: application state management (app.rs), UI rendering (ui.rs), backend abstractions (backends.rs), and the async event loop entry point (main.rs). Together these modules implement The Elm Architecture pattern for a responsive, chat-style terminal interface with conversation history and fullscreen mode switching for agent selection and install prompts.

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
pub mod cli;          // CLI argument parsing and agent name mapping
pub mod conversation; // JSONL parsing and event rendering
pub mod ui;           // Rendering functions for each mode
```

**Entry Point** (@/src/main.rs):
- `main()`: Parses CLI arguments via clap::Parser, validates agent name (exits with error if invalid), reads from stdin if piped, then sets up terminal (raw mode, Viewport::Inline(8)), runs async event loop, restores terminal on exit with cursor positioning to next line before disabling raw mode to ensure shell prompt appears cleanly below TUI content
- `run_app(agent_index, initial_message)`: Core event loop using tokio::select! to handle messages and render at ~30 fps interval - accepts optional agent_index to skip agent selection screen and optional initial_message to pre-fill textarea - includes mpsc channel for syncing `last_ctrl_c_time` to event handler task, conditionally increments `loading_frame` counter during streaming when using legacy spinner (only when `use_codex_components = false`)
- `handle_event_simple()` / `handle_key_simple()`: Convert crossterm key events to Message based on current mode - Ctrl-C detection happens FIRST before overlay/install prompt checks to ensure double Ctrl-C always works
- `get_backend()`: Factory function that returns appropriate backend (Claude or Codex) based on selected_agent_index
- `spawn_and_stream()`: Consumes backend stream using tokio::select! to multiplex stream consumption with cancellation signal - when cancelled, stream is dropped and child process cleanup happens via Drop semantics
- `wrap_text_to_width()`: Manual text wrapping function that splits Text into multiple Lines fitting within terminal width - required because `insert_before()` only handles single-line insertion and Ratatui's `Paragraph::wrap()` only applies during render phase

**State Management** (@/src/app.rs):
- `AppMode`: Enum with Selection/Input/Streaming states - now primarily tracks Streaming vs non-Streaming (simplified from screen-based modes)
- `Message`: Enum of all possible events that trigger state changes - includes `StreamEvent(ConversationEvent)` for backend events, `CancelStream` for interruption, `ToggleAgentRouter` for overlay, `ClearTextarea` for Ctrl-C keyboard interrupt, and `KeyPress` for textarea input
- `Model`: Struct holding all application state including `show_agent_router: bool` for overlay visibility, `show_debug_events: bool` for debug event filtering (defaults to false), `response_events` vector for full conversation history, `current_stream_token: Option<CancellationToken>` for tracking active stream, `last_ctrl_c_time: Option<Instant>` for tracking Ctrl-C timeout window, `use_codex_components: bool` flag to toggle between Shimmer component (true, default) and legacy spinner (false), and `loading_frame: usize` for legacy spinner animation frame tracking
- `Model::update()`: Pure function that transitions state based on message - implements TEA "update" phase, with navigation/selection now gated by `show_agent_router` flag instead of mode
- SubmitInput handler (@/src/main.rs:113-188): For regular prompts (non-slash-commands), renders UserMessage to scrollback BEFORE backend availability check, captures textarea content, clears textarea immediately (before streaming begins), adds UserMessage to history, transitions to Streaming mode
- CancelStream handler: Calls token.cancel(), transitions to Selection mode, appends StreamCancelled event to history (textarea already cleared by SubmitInput)
- ClearTextarea handler: Implements two-stage Ctrl-C keyboard interrupt - first press clears textarea and shows hint, second press within 2-second timeout signals quit by clearing timestamp (detected in main loop via Some → None transition)
- `create_textarea()` helper (@/src/app.rs:433-435): Factory function that creates TextArea instances with default configuration using `TextArea::new(TextAreaConfig::default())`

**UI Rendering** (@/src/ui.rs):
- `render()`: Routes to appropriate fullscreen renderer based on state flags - install prompt takes priority (blocking action), then agent router, then normal chat view
- `render_chat()`: Four-section vertical layout for normal mode - Input textarea (dynamic height), Agent info (1 line showing selected agent), Loading animation (1 line, only visible during streaming), and Instructions footer (1 line)
- **Conditional Loading Animation**: When `current_mode == AppMode::Streaming`, checks `use_codex_components` flag to select rendering path - if true (default), instantiates `Shimmer::new()` from tui-components with message "{agent_name} processing..." and renders time-based animation; if false, renders legacy spinner using `loading_frame % frames.len()` to cycle through Braille spinner characters ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
- `render_agent_selection_fullscreen()`: Fullscreen agent selection UI using entire viewport - Title (2 lines), Agent list with availability (Min 3 lines, flexible), Instructions (2 lines)
- `render_install_prompt_fullscreen()`: Fullscreen install prompt UI using entire viewport - Title (2 lines), Message with wrapping (Min 2 lines, flexible), Options list (3 lines), Instructions (1 line)
- **Fullscreen mode switching**: Instead of overlaying modals with `Clear` widget and percentage-based positioning, UI switches between three exclusive fullscreen views - works consistently in both inline viewports (8 lines) and fullscreen mode
- **Layout Constraints**: Autocomplete mode uses 4 constraints (textarea, autocomplete, shimmer, instructions), non-autocomplete mode uses 4 constraints (textarea, agent info, shimmer, instructions) - instructions always at chunks[3]

**Conversation Event Handling** (@/src/conversation.rs):
- `ConversationEvent` enum: Structured representation of backend JSONL events - includes UserMessage for chat history, AssistantMessage, SystemEvent, ResultSummary, StderrOutput, StreamCancelled for interruptions, UnknownEvent for unparseable events, StatusMessage for system feedback messages
- `parse_jsonl_event()`: Parses raw JSONL strings into ConversationEvent - handles Claude CLI event format with nested message.content arrays
- `render_event()`: Converts ConversationEvent into styled ratatui Lines - UserMessage renders with cyan `[user]` prefix, StatusMessage renders with green `[status]` prefix, StreamCancelled renders "Interrupted" in red, other events render with type-specific prefixes and colors
- `should_render_event()`: Filters events based on debug mode - SystemEvent and UnknownEvent are considered debug events (hidden when `show_debug: false`), all other events (UserMessage, AssistantMessage, ResultSummary, StderrOutput, StreamCancelled, StatusMessage) are always visible

**CLI Argument Parsing** (@/src/cli.rs):
- `Cli` struct: Derives clap::Parser for command-line argument parsing with two optional fields - `agent: Option<String>` for agent selection and `message: Option<String>` for initial message
- `agent_name_to_index(name)`: Maps agent name strings to backend array indices - supports "claude" (0), "codex" (1), "claudecode" (2), "mock" (3) - case-insensitive matching via .to_lowercase(), returns None for invalid names
- `valid_agent_names()`: Returns Vec of valid agent names for error messages
- Agent selection via CLI bypasses TUI selection screen by setting `model.selected_agent_index` directly in run_app()
- Stdin detection via `io::stdin().is_terminal()` from std::io::IsTerminal trait - reads piped input with `read_to_string()` before TUI initialization
- CLI message argument takes precedence over stdin when both provided

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
  → Ctrl-C (first press) → ClearTextarea clears textarea, sets timestamp, shows "Press Ctrl-C again to exit" hint
  → Ctrl-C (second press within 2 seconds) → ClearTextarea clears timestamp → main loop detects Some → None transition → sends Quit message
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
  → ClaudeBackend yields events from stdout parsed via parse_jsonl_event()
  → When ResultSummary event is received, backend returns immediately (exits the async generator)
  → spawn_and_stream() receives ResultSummary event → sends Message::StreamEvent + immediately sends Message::StreamComplete
  → run_app loop transitions to Selection mode
  → For each streamed event BEFORE ResultSummary: render_event() converts to styled Line
  → wrap_text_to_width() splits into multiple Lines, terminal.insert_before() accumulates lines
  → Model::update() accumulates events in response_events
  → Model::update() → accumulates ALL events in response_events (no filtering at storage level)
  → render_chat() maps all events via render_event() to styled Lines

Cancellation Path
  → User presses Esc → CancelStream message → token.cancel() called
  → spawn_and_stream's cancel branch fires → stream dropped → child process cleanup via Drop
  → StreamCancelled event added to history → UI returns to Selection mode
```

### Things to Know

**CLI Argument Initialization Flow** (@/src/main.rs:31-68):
- CLI parsing happens before terminal initialization to allow early exit on invalid agent names
- Agent validation uses fail-fast strategy: invalid agent name prints error and exits with code 1 before TUI setup
- Stdin detection via `!io::stdin().is_terminal()` checks if input is piped before consuming stdin with `read_to_string()`
- Reading stdin consumes the entire input stream before TUI initialization - cannot read stdin after terminal is in raw mode
- Message precedence: CLI argument (--message) takes priority over piped stdin via `cli.message.or(stdin_message)`
- Agent selection bypass: when agent_index is Some, skips TUI agent selection screen by pre-populating `model.selected_agent_index`
- Textarea pre-fill: initial_message sets textarea content via `.set_text()` before event loop starts

**Terminal Cleanup Sequence** (@/src/main.rs):
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
- Event handler also tracks `last_ctrl_c_time` via ctrl_c_rx channel for two-stage Ctrl-C detection
- Main loop sends updated mode and ctrl_c_time after every state change to keep event handler in sync
- This prevents stale state from causing incorrect event-to-message conversions
- Alt+A is handled globally (works regardless of mode) to toggle agent router overlay
- Ctrl-C is checked FIRST in handle_key_simple (before overlay/install prompt checks) to ensure it always works
- 'q' quit key respects mode - works in Selection/Streaming but types 'q' character in Input mode

**State Machine Invariants**:
- list_state always has Some(index) selected for agent router navigation
- Navigation (NextItem/PreviousItem) only updates list_state when `show_agent_router` is true
- KeyPress messages only update textarea when `show_agent_router` is false (input blocked when overlay open)
- response_events accumulates across all interactions - conversation history never cleared, preserves full chat context
- textarea is cleared exactly once per submission: in SubmitInput handler immediately after capturing user text, before transitioning to Streaming mode
- textarea is also cleared by first Ctrl-C press via ClearTextarea message
- StreamComplete and CancelStream handlers do NOT clear textarea - it's already empty from SubmitInput
- SubmitInput only transitions to Streaming if textarea contains non-whitespace text
- User messages are rendered to terminal scrollback in SubmitInput handler BEFORE backend availability check and BEFORE slash command processing - ensures messages appear even if install prompt is shown or command execution fails
- Ctrl-C timeout window is 2 seconds - first press sets timestamp, second press within window triggers quit, press after timeout resets to first press behavior
- Main loop detects quit signal by monitoring last_ctrl_c_time transition from Some → None (not by checking timestamp directly)

**Debug Event Filtering** (@/src/conversation.rs:should_render_event, @/src/main.rs:112):
- Two-tier filtering architecture: storage vs rendering
- **Storage level**: ALL events stored in `response_events` vector regardless of debug mode - no filtering at Model::update() level
- **Rendering level**: Events filtered in main event loop based on `model.show_debug_events` before calling `terminal.insert_before()`
- **Debug events**: SystemEvent and UnknownEvent are classified as debug events
  - SystemEvent: Contains raw protocol messages like session initialization, state changes
  - UnknownEvent: Contains unparseable JSONL that doesn't match known event types
- **Always-visible events**: UserMessage, AssistantMessage, ResultSummary, StderrOutput, StreamCancelled, StatusMessage
- **Toggle mechanism**: `/debug` slash command (@/src/commands/debug.rs) toggles `show_debug_events` boolean and emits StatusMessage with feedback
- **Why filtering at render time**: Allows users to toggle debug mode and retroactively view debug events in conversation history without losing data - if filtered at storage, events would be permanently lost
- **Default state**: Debug events hidden (`show_debug_events = false`) to reduce noise from system protocol messages

**Conversation Event Parsing** (@/src/conversation.rs):
- Parsing based on actual Claude CLI output format: `{"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}`
- Assistant messages extract all text blocks from content array and join with newlines
- System events check for `subtype` field and optional `cwd` or `session_id` details
- Result events check `subtype` for "success" vs other values to determine success boolean
- Stderr output wrapped in StderrOutput variant for consistent styling
- Malformed JSON or unparseable events return None from parse_jsonl_event()

**Event Rendering Styles** (@/src/conversation.rs):
- UserMessage: `[user]` prefix in bold cyan, followed by user's prompt text - always visible
- AssistantMessage: Plain white text, no prefix - always visible
- SystemEvent: `[system]` prefix in dim dark gray, subtype and details in dark gray - hidden by default (debug event)
- ResultSummary: `[done]` prefix in bold green (success) or bold red (failure) - always visible
- StderrOutput: Red text, no prefix - always visible
- StreamCancelled: "Interrupted" in red - always visible
- UnknownEvent: `[unknown]` prefix in yellow with raw JSON - hidden by default (debug event)
- StatusMessage: `[status]` prefix in bold green, followed by status text - always visible

**Error Display Strategy**:
- Errors don't transition mode - Model stays in Streaming so stderr output remains visible in conversation history
- Error message stored in Model::error_message for display in chat view
- User must manually press Esc to return to Selection mode after reading error
- Error events (StderrOutput) render in red within conversation history for visibility

**Rendering Performance**:
- Render interval is 33ms (~30 fps) regardless of event frequency
- ratatui only redraws changed terminal cells, so rapid renders are efficient
- Frame is mut reference, allowing widgets to modify cursor position during render

**Conditional Loading Animation Strategy** (@/src/ui.rs:107-124, @/src/app.rs:119-120, @/src/main.rs:374-377):
- Two parallel animation implementations: Shimmer component (new) and legacy spinner (old)
- **Toggle mechanism**: `use_codex_components` flag in Model enables switching at runtime without code changes
- **Default behavior**: Shimmer component enabled (`use_codex_components = true`)
- **Animation architecture differences**:
  - Shimmer: Time-based animation using `Instant::now()`, no Model state required
  - Legacy: Frame-based animation using `loading_frame` counter, incremented per render tick
- **Frame increment optimization**: Counter only advances when both conditions met: `current_mode == AppMode::Streaming && !use_codex_components`
- **Purpose**: Provides flexibility for testing/comparison, demonstrates component library integration while maintaining fallback, enables A/B testing of animation approaches
- **No runtime overhead**: Branch check in render path is negligible, frame increment gated at event loop level

**Component Library Integration** (@/Cargo.toml, @/src/ui.rs, @/src/app.rs, @/src/main.rs):
- nori-cli depends on tui-components as a path dependency (./tui-components)
- **TextArea component**: Uses `tui_components::textarea::TextArea` for multi-line text input - replaced external `tui-textarea` crate to consolidate dependencies within tui-components library
- **TextArea API**: Created via `TextArea::new(TextAreaConfig::default())`, handles key events via `.handle_key(key)`, exposes text via `.text()` returning `&str`, supports `.is_empty()` check, and cursor positioning via `.set_cursor(pos)`
- **Shimmer component**: Conditional rendering approach using `use_codex_components: bool` flag in Model (defaults to true) to toggle between Shimmer component and legacy spinner
- **Shimmer component path** (when `use_codex_components = true`): Shimmer instantiated on-demand during render (`Shimmer::new()`) with time-based animation using `Instant::now()` internally - no Model state tracking required
- **Legacy spinner path** (when `use_codex_components = false`): Uses frame-based animation with `loading_frame: usize` counter in Model, incremented on each render tick in main.rs event loop (lines 374-377), cycles through Braille spinner frames using modulo
- **Frame increment gating** (@/src/main.rs:374-377): Frame counter only increments when `current_mode == AppMode::Streaming && !use_codex_components`, ensuring frame counter doesn't advance when using Shimmer
- **Backward compatibility**: Preserves legacy spinner as fallback option while demonstrating component library integration pattern
- Integration demonstrates nori-cli as a consumer of the extracted component library pattern with graceful fallback mechanism

**Inline Viewport Compatibility** (@/src/ui.rs):
- UI designed to work in both inline viewports (Viewport::Inline(8) from main.rs) and fullscreen mode
- Uses fullscreen mode switching instead of overlay modals to avoid percentage-based positioning issues in constrained viewports
- All UI modes (chat, agent selection, install prompt) use Constraint::Min() for flexible sections that adapt to available viewport height
- Text wrapping enabled on install prompt message to handle varying viewport widths without manual text size management

**TextArea Creation Pattern** (@/src/app.rs, @/src/main.rs):
- All TextArea instances created via `create_textarea()` helper function in @/src/app.rs:433-443
- TextArea creation happens in multiple locations: Model::default() initialization, Model::update() handlers (SubmitInput, ClearTextarea, AutocompleteSelect), and main.rs (slash command execution)
- `create_textarea()` provides DRY pattern with consistent styling configuration:
  - DarkGray background via `.with_background_style(Style::default().bg(Color::DarkGray))`
  - 1 row top/bottom padding, 0 columns left/right padding via `.with_padding(1, 1, 0, 0)`
  - › prefix symbol via `.with_prefix("›", Style::default())`
  - "Write a message..." placeholder via `.with_placeholder()`
- AutocompleteSelect handler creates TextArea with initial text content and positions cursor at end of text using `.set_cursor(text.len())`
- Styling provides visual distinction from terminal background and clear input affordance

**Key Event Handling**:
- KeyEvent passed to textarea via Message::KeyPress only when `show_agent_router` is false
- textarea.handle_key(key) processes keyboard input for cursor movement, text editing, and newlines internally
- Ctrl-C is checked FIRST in handle_key_simple (@/src/main.rs:320-326) - takes priority over all other key handling including overlays and install prompts
- Alt+A globally toggles agent router overlay - handled before mode-specific logic in @/src/main.rs:117-121
- 'q' quits from Selection/Streaming modes but types 'q' character when in Input mode
- Alt+Enter submits because Ctrl+Enter doesn't work reliably across terminal emulators
- Esc during streaming sends CancelStream message to interrupt the stream
- Esc closes agent router overlay when open via ExitInputMode message

**Ctrl-C Keyboard Interrupt Mechanism** (@/src/app.rs:265-288, @/src/main.rs:120-134, @/src/main.rs:320-326):
- Two-stage interrupt pattern: first Ctrl-C clears textarea and shows hint, second Ctrl-C within 2 seconds exits application
- **Priority**: Ctrl-C detection happens FIRST in handle_key_simple, before all other key checks (overlays, install prompts, mode-specific handling)
- **State tracking**: Model.last_ctrl_c_time stores Option<Instant> - None initially, Some(time) after first press, None again after second press
- **Timeout logic**: Lives in Model::update() ClearTextarea handler - compares current time to last_ctrl_c_time using 2-second const
- **State sync**: Main loop syncs last_ctrl_c_time to event handler via ctrl_c_rx channel after every state change
- **Quit detection**: Main loop monitors transition from Some → None (lines 121-128) - when detected, sends Message::Quit
- **Visual feedback**: First press displays "Press Ctrl-C again to exit" via Model.error_message field (reuses existing error display mechanism)
- **Timeout reset**: If Ctrl-C pressed after 2-second window expires, timestamp is updated and hint shown again (behaves as first press)
- **Works everywhere**: Ctrl-C always functional regardless of application state - during streaming, with overlays open, in install prompt

**Stream Completion and Cancellation**:
- **Normal completion**: Driven by semantic signals (ResultSummary events from JSONL output), not process exit. ClaudeBackend returns immediately upon receiving ResultSummary, stream stops yielding events, spawn_and_stream receives no more events and sends Message::StreamComplete
- **User cancellation**: CancellationToken from tokio-util used for cooperative cancellation - Token created in main loop when spawning stream task, stored in Model.current_stream_token - When cancel branch fires in spawn_and_stream, stream is dropped which closes file handles - Child process cleanup happens via Drop trait implementation - StreamCancelled event provides visual feedback in conversation history - No explicit process killing - relies on Drop semantics and closed handles

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
