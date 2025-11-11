# Noridoc: tests

Path: @/tests

### Overview

Test suite for the agent-router-tui application, covering state machine transitions, conversation event parsing/rendering (including UserMessage events), and subprocess JSONL streaming. Uses unit tests for Model::update() and conversation module logic, plus integration tests with MockBackend to verify end-to-end subprocess streaming without requiring real agent CLIs.

### How it fits into the larger codebase

- Imports public API from @/src/lib.rs (app, backends, conversation modules)
- Tests state transitions in isolation via @/src/app.rs:Model::update() without spawning processes
- Tests JSONL parsing and rendering via @/src/conversation.rs functions with example Claude CLI events
- Tests subprocess integration via @/src/backends/mock.rs:MockBackend which outputs actual Claude CLI JSONL format
- Run via `cargo test` in CI workflows (@/.github/workflows/pr-ci.yml and @/.github/workflows/main-ci.yml)
- No tests for @/src/main.rs event loop or @/src/ui.rs rendering functions - these require terminal environment

### Core Implementation

**Textarea Clearing Tests** (@/tests/textarea_clearing_test.rs):
- `test_textarea_clears_immediately_on_submit()`: Verifies textarea is cleared immediately when SubmitInput message is processed
  - User types a message into textarea
  - Sends SubmitInput message
  - Asserts textarea is empty after SubmitInput (not waiting for stream to complete)
  - Asserts mode transitions to Streaming
- `test_empty_input_does_not_submit()`: Verifies empty textarea does not trigger submission
  - Sends SubmitInput with empty textarea
  - Asserts mode stays in Selection
  - Asserts no events added to response_events
- `test_whitespace_only_input_does_not_submit()`: Verifies whitespace-only input does not trigger submission
  - Types only whitespace into textarea
  - Sends SubmitInput
  - Asserts mode stays in Selection
- `test_textarea_stays_clear_during_streaming()`: Verifies textarea remains empty during streaming
  - Submits message (clears textarea)
  - Sends StreamEvent message
  - Asserts textarea still empty
- `test_textarea_stays_clear_after_stream_complete()`: Verifies textarea remains empty after stream completes
  - Submits message (clears textarea)
  - Sends StreamComplete message
  - Asserts textarea still empty and mode is Selection
- `test_textarea_stays_clear_after_cancel()`: Verifies textarea remains empty after stream cancellation
  - Submits message (clears textarea)
  - Sets up CancellationToken
  - Sends CancelStream message
  - Asserts textarea still empty (not restored to original content)

**State Machine Tests** (@/tests/state_machine_test.rs):
- `test_state_transitions()`: Verifies overlay and mode transitions
  - SelectItem closes agent router overlay (sets `show_agent_router` to false)
  - SubmitInput with non-empty text transitions to Streaming mode
  - StreamComplete returns to Selection mode
- `test_stream_event_accumulation()`: Verifies response_events vector accumulates ConversationEvent instances in order
  - Sends two StreamEvent messages with AssistantMessage events
  - Asserts response_events length is 2 and contents match expected text
- `test_toggle_agent_router_overlay()`: Verifies ToggleAgentRouter message toggles `show_agent_router` boolean
- `test_submit_input_adds_user_message_to_history()`: Verifies SubmitInput captures textarea content and adds UserMessage to response_events before spawning agent
- `test_cancel_stream_during_streaming()`: Verifies CancelStream message handling
  - Submits a message via SubmitInput (which clears textarea and sets up streaming state)
  - Sets up CancellationToken for the stream
  - Sends CancelStream message
  - Asserts token.is_cancelled() is true
  - Verifies mode transitions to Selection, token cleared from model
  - Verifies StreamCancelled event added to response_events

**Subprocess Tests** (@/tests/subprocess_test.rs):
- `test_mock_backend_streams_events()`: Verifies MockBackend can stream events
  - Creates CancellationToken and passes to spawn_stream()
  - Consumes stream and collects all events
  - Asserts events contain expected AssistantMessage variants
- `test_mock_backend_completes()`: Verifies stream completes naturally
  - Creates CancellationToken and passes to spawn_stream()
  - Consumes stream and verifies at least one event received
  - Tests stream completion without cancellation

**Conversation Rendering Tests** (@/tests/conversation_rendering_test.rs):
- `test_parse_assistant_message_event()`: Verifies parsing of Claude CLI assistant message format
- `test_parse_system_init_event()`: Verifies parsing of system events with subtype and details fields
- `test_parse_result_success_event()`: Verifies parsing of result events and success boolean extraction
- `test_parse_malformed_json()`: Verifies graceful handling of invalid JSON (returns None)
- `test_parse_multiple_text_blocks()`: Verifies joining multiple text content blocks with newlines
- `test_render_assistant_message()`: Verifies rendering produces plain Line without prefix
- `test_render_system_event()`: Verifies [system] prefix and dark gray styling
- `test_render_result_success()`: Verifies [done] prefix in green for success
- `test_render_result_failure()`: Verifies [done] prefix in red for non-success
- `test_render_stderr()`: Verifies red styling for stderr output
- `test_render_unknown()`: Verifies [unknown] prefix in yellow with raw JSON
- `test_parse_result_without_details()`: Verifies default "Completed" text when result field missing
- `test_render_user_message()`: Verifies UserMessage renders with `[user]` prefix in bold cyan with message text

### Things to Know

**Test Isolation Strategy**:
- State machine tests use Model directly, calling update() without any async runtime or subprocess spawning
- Conversation rendering tests are pure unit tests - pass JSONL strings to parse_jsonl_event() and verify ConversationEvent output
- Subprocess tests use MockBackend to avoid external dependencies on claude/codex CLIs
- No integration tests with real backends because they require API authentication
- Rendering tests verify both parsing and styling independently for each event type

**MockBackend Implementation**:
- Uses `printf` command to output hardcoded JSONL strings in actual Claude CLI format
- Two test events with full message structure: `{"type":"assistant","message":{"content":[{"type":"text","text":"Hello from mock"}]}}`
- Changed from simplified test format to real Claude CLI format to verify parsing handles nested structure
- Demonstrates minimal contract for backends: any process outputting newline-delimited JSON works
- Defined in @/src/backends/mock.rs and implements AgentBackend trait

**Async Test Pattern**:
- Subprocess tests use `#[tokio::test]` attribute to run in async context
- Allows awaiting async operations like spawn_process() and next_line()
- Each test gets its own tokio runtime - no shared state between tests

**JSONL Parsing Verification**:
- test_parse_assistant_message_events mirrors the parsing logic from @/src/main.rs using conversation::parse_jsonl_event()
- Reads line-by-line from stdout
- Parses each line via parse_jsonl_event() which handles full Claude CLI structure
- Filters for ConversationEvent::AssistantMessage variants
- Extracts text field from AssistantMessage
- Verifies contract between backends and main loop: backends output Claude CLI format, parse_jsonl_event() produces ConversationEvent

**Conversation Module Test Coverage**:
- Comprehensive tests for all ConversationEvent variants (14 tests total including UserMessage and StreamCancelled)
- Edge cases: malformed JSON, missing fields, multiple text blocks, empty details
- Both parsing (JSONL → ConversationEvent) and rendering (ConversationEvent → styled Line) tested independently
- Rendering tests verify color, style modifiers (dim, bold), and prefix text for each variant
- UserMessage test verifies chat history rendering with cyan styling
- StreamCancelled renders "Interrupted" in red to provide visual feedback for cancelled streams

**Textarea Clearing Test Coverage** (@/tests/textarea_clearing_test.rs):
- Comprehensive tests for textarea lifecycle across all state transitions (6 tests total)
- Verifies immediate clearing on submit (before streaming begins)
- Verifies edge cases: empty input, whitespace-only input
- Verifies textarea stays clear throughout streaming, completion, and cancellation
- Tests ensure textarea is never restored after cancellation (matches modern chat UX expectations)

**Coverage Gaps**:
- No tests for @/src/main.rs event handling (handle_event_simple, handle_key_simple) - would require simulating crossterm events including Alt+A global shortcut
- No tests for @/src/ui.rs rendering functions (render_chat, render_agent_router_overlay) - would require terminal buffer assertions
- No tests for overlay interaction blocking (navigation disabled in chat, input disabled in overlay) - requires event handler integration
- No tests for error handling paths (non-zero exit status, stderr output) - MockBackend always succeeds
- No tests for session resumption - session_id/thread_id fields are never populated
- No tests verifying conversation history persistence across multiple submit cycles
- No integration test for actual stream cancellation with long-running process - test_cancel_stream_during_streaming only tests state machine

**CI Integration**:
- Tests run on every PR via @/.github/workflows/pr-ci.yml
- Tests run on pushes to main via @/.github/workflows/main-ci.yml
- Command: `cargo test --verbose` to show test output
- Must pass before merge is allowed

Created and maintained by Nori.
