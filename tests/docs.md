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

**CLI Argument Parsing Tests** (@/tests/cli_args_test.rs):
- `test_agent_long_flag()`: Verifies --agent flag parses correctly with agent name
- `test_agent_short_flag()`: Verifies -a short flag parses correctly
- `test_message_only()`: Verifies positional message argument without agent flag
- `test_agent_and_message()`: Verifies both --agent and message arguments together
- `test_no_arguments()`: Verifies empty CLI args parse to None values
- `test_agent_name_claude()`: Verifies "claude" maps to index 0
- `test_agent_name_codex()`: Verifies "codex" maps to index 1
- `test_agent_name_claudecode()`: Verifies "claudecode" maps to index 2
- `test_agent_name_mock()`: Verifies "mock" maps to index 3
- `test_agent_name_case_insensitive()`: Verifies agent name matching is case-insensitive
- `test_agent_name_invalid()`: Verifies invalid agent names return None from agent_name_to_index()

**Ctrl-C Handling Tests** (@/tests/ctrl_c_handling_test.rs):
- `test_first_ctrl_c_clears_textarea_and_shows_hint()`: Verifies first Ctrl-C press behavior
  - Adds text to textarea via .insert_str()
  - Sends ClearTextarea message
  - Asserts textarea is cleared via .is_empty()
  - Asserts last_ctrl_c_time is set to Some(Instant)
  - Asserts error_message contains "Press Ctrl-C again to exit" hint
- `test_second_ctrl_c_within_timeout_clears_timestamp()`: Verifies second Ctrl-C within 2-second window
  - Sends first ClearTextarea (sets timestamp)
  - Immediately sends second ClearTextarea
  - Asserts last_ctrl_c_time transitions to None (signals quit needed)
  - Asserts error_message cleared
- `test_ctrl_c_after_timeout_clears_textarea_again()`: Verifies timeout expiration behavior
  - Sends first ClearTextarea
  - Manually sets timestamp to 3 seconds in the past (simulates timeout)
  - Adds new text to textarea via .insert_str()
  - Sends ClearTextarea again
  - Asserts textarea cleared again via .is_empty()
  - Asserts new timestamp is set (more recent than old one)
  - Asserts hint shown again (behaves as first press)

**Textarea Clearing Tests** (@/tests/textarea_clearing_test.rs):
- `test_textarea_clears_immediately_on_submit()`: Verifies textarea is cleared immediately when SubmitInput message is processed
  - User types a message into textarea via .insert_str()
  - Sends SubmitInput message
  - Asserts textarea is empty via .is_empty() after SubmitInput (not waiting for stream to complete)
  - Asserts mode transitions to Streaming
- `test_empty_input_does_not_submit()`: Verifies empty textarea does not trigger submission
  - Sends SubmitInput with empty textarea
  - Asserts mode stays in Selection via .is_empty()
  - Asserts no events added to response_events
- `test_whitespace_only_input_does_not_submit()`: Verifies whitespace-only input does not trigger submission
  - Types only whitespace into textarea via .insert_str()
  - Sends SubmitInput
  - Asserts mode stays in Selection
- `test_textarea_stays_clear_during_streaming()`: Verifies textarea remains empty during streaming
  - Submits message via .insert_str() and SubmitInput (clears textarea)
  - Sends StreamEvent message
  - Asserts textarea still empty via .is_empty()
- `test_textarea_stays_clear_after_stream_complete()`: Verifies textarea remains empty after stream completes
  - Submits message via .insert_str() and SubmitInput (clears textarea)
  - Sends StreamComplete message
  - Asserts textarea still empty via .is_empty() and mode is Selection
- `test_textarea_stays_clear_after_cancel()`: Verifies textarea remains empty after stream cancellation
  - Submits message via .insert_str() and SubmitInput (clears textarea)
  - Sets up CancellationToken
  - Sends CancelStream message
  - Asserts textarea still empty via .is_empty() (not restored to original content)

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
  - Submits a message via .insert_str() and SubmitInput (which clears textarea and sets up streaming state)
  - Sets up CancellationToken for the stream
  - Sends CancelStream message
  - Asserts token.is_cancelled() is true
  - Verifies mode transitions to Selection, token cleared from model
  - Verifies textarea is empty via .is_empty()
  - Verifies StreamCancelled event added to response_events

**Model Backend Ordering Tests** (@/tests/model_backend_ordering_test.rs):
- `test_backend_options_ordering()`: Verifies BACKEND_OPTIONS contains backends in correct order
- `test_backend_instantiation()`: Verifies each backend can be instantiated correctly via factory functions
- `test_backend_availability_checks()`: Verifies availability check functions work for each backend
- `test_get_backend_returns_correct_type()`: Verifies get_backend() returns correct backend type for each index
- Ensures centralized backend system maintains consistency across instantiation, availability checking, and UI display

**Blackbox TUI Tests** (@/tests/blackbox_tui_test.rs):
- `test_initial_state()`: Verifies initial TUI rendering with empty model and default state
- `test_typed_hi()`: Verifies textarea rendering after typing "hi"
- `test_multiline_input()`: Verifies textarea rendering with multiple lines
- `test_long_text_wrapping()`: Verifies text wrapping in textarea for long input
- `test_unicode_input()`: Verifies unicode character handling in textarea
- `test_empty_submission_prevented()`: Verifies UI state when empty submission is blocked
- `test_shimmer_renders_during_streaming()`: Verifies Shimmer component renders during streaming mode
  - Sets `model.current_mode = AppMode::Streaming`
  - Sets `model.selected_agent_index = Some(0)` for agent name in shimmer
  - Renders UI and verifies output contains "processing" text
  - Snapshot captures Shimmer animation (not legacy spinner)

**Subprocess Tests** (@/tests/subprocess_test.rs):
- `test_mock_backend_streams_events()`: Verifies MockBackend can stream events
  - Creates CancellationToken and passes to spawn_stream()
  - Consumes stream and collects all events
  - Asserts events contain expected AssistantMessage variants
- `test_mock_backend_completes()`: Verifies stream completes naturally
  - Creates CancellationToken and passes to spawn_stream()
  - Consumes stream and verifies at least one event received
  - Tests stream completion without cancellation

**ACP Process Cleanup Tests** (@/tests/acp_process_cleanup_test.rs):
- `test_process_cleanup_on_runner_drop()`: Verifies process termination when AcpAgentRunner is dropped
  - Spawns mock ACP agent via AcpAgentRunner.spawn_stream()
  - Retrieves PID via agent_pid() method
  - Verifies process is running via OS-level `kill -0 <pid>` check
  - Drops runner without cancelling stream
  - Verifies process is terminated after 100ms (proves Drop impl kills process)
  - Tests RAII pattern - process cleanup happens automatically on scope exit
- `test_process_cleanup_on_reuse()`: Verifies old process is killed when spawning new stream
  - Spawns first stream and captures PID
  - Spawns second stream (triggers drop of first process)
  - Verifies first process is terminated via `kill -0` check
  - Verifies new process is running with different PID
  - Tests process cleanup on stream reuse scenario
- `test_process_cleanup_on_init_failure()`: Verifies cleanup on initialization errors
  - Uses invalid agent config (echo command instead of real agent)
  - Attempts to spawn stream (should fail during initialization)
  - Verifies error is returned
  - Verifies no processes are left hanging (echo exits immediately)
  - Tests error path doesn't panic or leak processes
- All tests use `once_cell::sync::Lazy<Mutex<()>>` guard to prevent parallel execution (avoids port conflicts)
- Tests build mock_acp_agent binary via `cargo build --manifest-path mock-acp-agent/Cargo.toml` before each test
- Process verification uses actual OS calls (`kill -0`) instead of Rust object state - validates real system behavior
- Uses tokio::time::sleep for timing delays to allow OS process cleanup to complete

**Dynamic TextArea Height Tests** (@/tests/dynamic_textarea_height_test.rs):
- `test_single_line_returns_minimum_height()`: Verifies single line of text returns minimum height
  - Creates TextArea via TextArea::new(TextAreaConfig::default())
  - Inserts text via .insert_str()
  - Calls `textarea.desired_height(80)` and asserts it returns 1 (content only, no padding)
- `test_multiline_returns_correct_height()`: Verifies multiple lines return correct height
  - Creates TextArea and inserts multiple lines via .insert_str() with embedded newlines
  - Calls `textarea.desired_height(80)` and asserts height equals line count
- `test_multiline_bordered_returns_correct_height()`: Verifies height calculation with padding
  - Creates TextArea with custom padding (4 top, 5 bottom, 6 left, 7 right)
  - Calls `textarea.desired_height(80)` to get content height
  - Manually adds `config.padding_top + config.padding_bottom` to get total height
  - Asserts total height = content height + padding (3 lines + 4 + 5 = 12)
- `test_long_line_accounts_for_wrapping()`: Verifies long lines account for text wrapping
  - Creates TextArea and inserts 250-character string via .insert_str()
  - Calls `textarea.desired_height(80)` and asserts height equals 4 (250/80 = 3.125 rounded up)
- `test_desired_height_returns_actual_line_count()`: Verifies TextArea returns actual line count without max constraint
  - Creates TextArea and inserts 20 lines
  - Calls `textarea.desired_height(80)` and asserts it returns 20 (no built-in max)
  - Documents that UI code must apply max height constraint separately

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
- `test_render_status_message()`: Verifies StatusMessage renders with `[status]` prefix in bold green with status text
- `test_should_render_system_event_when_debug_enabled()`: Verifies SystemEvent is visible when `show_debug` is true
- `test_should_not_render_system_event_when_debug_disabled()`: Verifies SystemEvent is hidden when `show_debug` is false
- `test_should_render_unknown_event_when_debug_enabled()`: Verifies UnknownEvent is visible when `show_debug` is true
- `test_should_not_render_unknown_event_when_debug_disabled()`: Verifies UnknownEvent is hidden when `show_debug` is false
- `test_should_always_render_user_message()`: Verifies UserMessage is visible regardless of debug mode
- `test_should_always_render_assistant_message()`: Verifies AssistantMessage is visible regardless of debug mode
- `test_should_always_render_status_message()`: Verifies StatusMessage is visible regardless of debug mode

### Things to Know

**Test Isolation Strategy**:
- State machine tests use Model directly, calling update() without any async runtime or subprocess spawning
- Conversation rendering tests are pure unit tests - pass JSONL strings to parse_jsonl_event() and verify ConversationEvent output
- Subprocess tests use MockBackend to avoid external dependencies on claude/codex CLIs
- No integration tests with real backends because they require API authentication
- Rendering tests verify both parsing and styling independently for each event type
- **ACP process cleanup tests verify OS-level behavior**: Use `kill -0` syscall to check if process actually exists, not just Rust object state - validates that Drop implementation actually terminates processes at system level, not just Rust's internal tracking

**TextArea Testing Pattern**:
- Tests use tui_components::textarea::TextArea instead of external tui-textarea crate
- TextArea creation: `TextArea::new(TextAreaConfig::default())` in all tests
- Text insertion: `.insert_str()` method for adding text content (takes string or &str reference)
- Empty check: `.is_empty()` method replaces `.lines()[0].is_empty()` pattern
- Text access: `.text()` returns `&str` instead of `.lines()` returning array
- Newlines: Embedded in strings passed to `.insert_str()` instead of separate `.insert_newline()` calls
- Height calculation: `.desired_height(width)` returns content height (line count with wrapping), padding must be added separately via `config.padding_top + config.padding_bottom`

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

**CLI Argument Test Coverage** (@/tests/cli_args_test.rs):
- Comprehensive tests for clap::Parser argument parsing (11 tests total)
- Tests both long (--agent) and short (-a) flag forms
- Tests positional message argument parsing
- Tests combination of agent flag and message argument
- Tests empty argument scenario (no flags or args)
- Agent name mapping tests verify all four backends (claude, codex, claudecode, mock)
- Case-insensitive matching tests verify .to_lowercase() behavior
- Invalid agent name test verifies None return for unknown agents

**Conversation Module Test Coverage**:
- Comprehensive tests for all ConversationEvent variants (21 tests total including UserMessage, StreamCancelled, StatusMessage, and filtering logic)
- Edge cases: malformed JSON, missing fields, multiple text blocks, empty details
- Both parsing (JSONL → ConversationEvent) and rendering (ConversationEvent → styled Line) tested independently
- Rendering tests verify color, style modifiers (dim, bold), and prefix text for each variant
- UserMessage test verifies chat history rendering with cyan styling
- StreamCancelled renders "Interrupted" in red to provide visual feedback for cancelled streams
- StatusMessage test verifies system feedback messages with green styling
- Event filtering tests verify `should_render_event()` behavior for debug mode toggle (7 tests)
  - SystemEvent and UnknownEvent hidden when debug disabled
  - SystemEvent and UnknownEvent visible when debug enabled
  - UserMessage, AssistantMessage, and StatusMessage always visible regardless of debug mode

**Ctrl-C Handling Test Coverage** (@/tests/ctrl_c_handling_test.rs):
- Comprehensive tests for two-stage Ctrl-C keyboard interrupt mechanism (3 tests total)
- Verifies first press behavior: textarea clearing, timestamp setting, hint display
- Verifies second press within timeout: timestamp cleared to signal quit
- Verifies timeout expiration: behaves as first press after 2-second window
- All tests use Model directly without event loop - tests state machine logic in isolation
- Timeout simulation via manual timestamp manipulation (no actual sleeping)

**Textarea Clearing Test Coverage** (@/tests/textarea_clearing_test.rs):
- Comprehensive tests for textarea lifecycle across all state transitions (6 tests total)
- Verifies immediate clearing on submit (before streaming begins)
- Verifies edge cases: empty input, whitespace-only input
- Verifies textarea stays clear throughout streaming, completion, and cancellation
- Tests ensure textarea is never restored after cancellation (matches modern chat UX expectations)

**TUI Component Adoption Test Coverage**:
- Height calculation tests (@/tests/dynamic_textarea_height_test.rs) updated to use `textarea.desired_height()` instead of `calculate_textarea_height()` function
- Tests verify `desired_height()` returns content height only, requiring manual padding addition for total height
- Test renamed from `test_height_respects_maximum_bound()` to `test_desired_height_returns_actual_line_count()` to reflect that TextArea does not enforce max height
- New test `test_shimmer_renders_during_streaming()` verifies Shimmer component renders during streaming mode
- Snapshot verification ensures Shimmer animation appears (not legacy spinner with Braille characters)

**Coverage Gaps**:
- No tests for @/src/main.rs event handling (handle_event_simple, handle_key_simple) - would require simulating crossterm events including Alt+A global shortcut and Ctrl-C priority detection
- No tests for @/src/ui.rs rendering functions (render_chat, render_agent_router_overlay) beyond blackbox snapshot tests - would require detailed terminal buffer assertions
- No tests for overlay interaction blocking (navigation disabled in chat, input disabled in overlay) - requires event handler integration
- No tests for error handling paths (non-zero exit status, stderr output) - MockBackend always succeeds
- No tests for session resumption - session_id/thread_id fields are never populated
- No tests verifying conversation history persistence across multiple submit cycles
- No integration test for actual stream cancellation with long-running process - test_cancel_stream_during_streaming only tests state machine
- No integration test for Ctrl-C priority over overlays/install prompts - tests only verify Model state transitions, not actual key event routing in handle_key_simple
- No test for main loop quit detection (Some → None timestamp transition) - would require full event loop integration
- No integration tests for CLI argument flow - stdin reading, agent validation with exit code, agent selection bypass, textarea pre-fill - only unit tests for clap parsing and agent name mapping
- No test for CLI message precedence (CLI arg vs stdin) - would require mocking stdin and command-line args together
- No test for MAX_HEIGHT constraint enforcement in UI code - tests verify TextArea returns actual line count, but UI applies 10-line max separately

**Test Coverage Added for Process Cleanup**:
- ✅ ACP runner Drop trait implementation verified via @/tests/acp_process_cleanup_test.rs
- ✅ Process termination on normal runner drop
- ✅ Process termination when spawning new stream (reuse scenario)
- ✅ Process cleanup on initialization failure
- ✅ OS-level verification using `kill -0` syscall (not just Rust object state)

**CI Integration**:
- Tests run on every PR via @/.github/workflows/pr-ci.yml
- Tests run on pushes to main via @/.github/workflows/main-ci.yml
- Command: `cargo test --verbose` to show test output
- Must pass before merge is allowed

Created and maintained by Nori.
