# Noridoc: tests

Path: @/tests

### Overview

Test suite for the agent-router-tui application, covering state machine transitions and subprocess JSONL parsing. Uses unit tests for Model::update() logic and integration tests with MockBackend to verify end-to-end subprocess streaming without requiring real agent CLIs.

### How it fits into the larger codebase

- Imports public API from @/src/lib.rs (app, backends modules)
- Tests state transitions in isolation via @/src/app.rs:Model::update() without spawning processes
- Tests subprocess integration via @/src/backends/mock.rs:MockBackend which outputs test JSONL
- Run via `cargo test` in CI workflows (@/.github/workflows/pr-ci.yml and @/.github/workflows/main-ci.yml)
- No tests for @/src/main.rs event loop or @/src/ui.rs rendering functions - these require terminal environment

### Core Implementation

**State Machine Tests** (@/tests/state_machine_test.rs):
- `test_state_transitions()`: Verifies mode transitions match expected state machine
  - Selection → SelectItem → Input
  - Input → SubmitInput → Streaming
  - Streaming → StreamComplete → Selection
  - Input → ExitInputMode → Selection
- `test_stream_chunk_accumulation()`: Verifies response_text vector accumulates chunks in order
  - Sends two StreamChunk messages
  - Asserts response_text length is 2 and contents match expected strings

**Subprocess Tests** (@/tests/subprocess_test.rs):
- `test_mock_backend_spawns_process()`: Verifies MockBackend can spawn process and output is readable
  - Spawns process via backend.spawn_process()
  - Reads stdout line-by-line with tokio BufReader
  - Asserts at least one line of output
  - Parses first line as JSON to verify format
- `test_parse_agent_message_events()`: Verifies JSONL parsing logic extracts content from agent_message events
  - Spawns MockBackend subprocess
  - Reads stdout and parses each line as JSON
  - Filters for type="agent_message" events
  - Extracts "content" field from each matching event
  - Asserts at least one message was extracted

### Things to Know

**Test Isolation Strategy**:
- State machine tests use Model directly, calling update() without any async runtime or subprocess spawning
- This allows fast, deterministic testing of state transitions and data accumulation
- Subprocess tests use MockBackend to avoid external dependencies on claude/codex CLIs
- No integration tests with real backends because they require API authentication

**MockBackend Implementation**:
- Uses `printf` command to output hardcoded JSONL strings
- Two test events: `{"type":"agent_message","content":"Hello from mock"}` and `{"type":"agent_message","content":"This is a test"}`
- Demonstrates minimal contract for backends: any process outputting newline-delimited JSON works
- Defined in @/src/backends/mock.rs and implements AgentBackend trait

**Async Test Pattern**:
- Subprocess tests use `#[tokio::test]` attribute to run in async context
- Allows awaiting async operations like spawn_process() and next_line()
- Each test gets its own tokio runtime - no shared state between tests

**JSONL Parsing Verification**:
- test_parse_agent_message_events mirrors the parsing logic from @/src/main.rs:166-196
- Reads line-by-line from stdout
- Parses each line as serde_json::Value
- Checks for "type" field matching "agent_message"
- Extracts "content" field value
- This verifies the contract between backends and main loop event handling

**Coverage Gaps**:
- No tests for @/src/main.rs event handling (handle_event_simple, handle_key_simple) - would require simulating crossterm events
- No tests for @/src/ui.rs rendering functions - would require terminal buffer assertions
- No tests for error handling paths (non-zero exit status, stderr output) - MockBackend always succeeds
- No tests for session resumption - session_id/thread_id fields are never populated

**CI Integration**:
- Tests run on every PR via @/.github/workflows/pr-ci.yml
- Tests run on pushes to main via @/.github/workflows/main-ci.yml
- Command: `cargo test --verbose` to show test output
- Must pass before merge is allowed

Created and maintained by Nori.
