# Noridoc: TUI Integration Tests

Path: @/codex-rs/tui-pty-e2e

### Overview

- Black-box integration testing framework for the Codex TUI using PTY (pseudo-terminal) emulation
- Spawns the real `codex` binary in a simulated terminal and exercises full application stack
- Uses VT100 parser to capture and validate terminal screen output via snapshot testing
- Provides programmatic keyboard input simulation and screen state polling

### How it fits into the larger codebase

- Tests the complete integration between `@/codex-rs/cli`, `@/codex-rs/tui`, `@/codex-rs/core`, and `@/codex-rs/acp`
- Complements unit tests in `@/codex-rs/tui/src/chatwidget.rs` by testing full application behavior
- Uses `@/codex-rs/mock-acp-agent` as the ACP backend for deterministic test scenarios
- Validates CLI argument parsing, TUI event loop, ACP protocol communication, and terminal rendering
- Part of the workspace at `@/codex-rs/Cargo.toml:46`

### Core Implementation

**Test Harness:** `TuiSession` in `@/codex-rs/tui-pty-e2e/src/lib.rs`

The main API provides:
- `spawn(rows, cols)` - Launch codex binary with mock-acp-agent in PTY with automatic temp directory
- `spawn_with_config(rows, cols, config)` - Launch with custom configuration and automatic temp directory
- `send_str(text)` - Simulate typing text
- `send_key(key)` - Send keyboard events (Enter, Escape, Ctrl-C, etc.)
- `wait_for_text(needle, timeout)` - Poll screen until text appears
- `wait_for(predicate, timeout)` - Poll screen until condition matches
- `screen_contents()` - Get current terminal screen as string

**Debugging Aids:**

`TuiSession` implements `Drop` to print screen state when tests panic, making it easier to diagnose PTY timing issues:
```rust
impl Drop for TuiSession {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("\n=== TUI Screen State at Panic ===");
            eprintln!("{}", self.screen_contents());
            eprintln!("=================================\n");
        }
    }
}
```

The crate exports helper functions for consistent test patterns:
- `TIMEOUT: Duration` - Standard 5-second timeout constant for use across all tests
- `normalize_for_snapshot(contents: String) -> String` - Normalizes dynamic content for snapshot testing (see below)

**Automatic Test Isolation:**

All tests run in isolated temporary directories created in `/tmp/`:
- Each `spawn()` or `spawn_with_config()` call creates a new temp directory
- Directory contains a `hello.py` file with `print('Hello, World!')`
- A `config.toml` is automatically generated in the temp directory (used as CODEX_HOME)
- Temp directory is automatically cleaned up when `TuiSession` is dropped
- Tests no longer run in user's home directory for better isolation

**Generated config.toml:**

By default, each session creates a `config.toml` in the temp directory with:
- `model` - Set to the configured model (defaults to `"mock-model"`)
- `model_provider = "mock_provider"` - Uses a custom provider that doesn't require OpenAI auth
- `trust_level = "trusted"` for the working directory - Skips trust approval screen
- `wire_api = "acp"` - Routes through ACP registry for model resolution

Custom config.toml content can be provided via `SessionConfig::with_config_toml(content)`

**Architecture:**

```
Test Code
    ↓
TuiSession (portable_pty)
    ↓
PTY Master ←→ PTY Slave
    ↓           ↓
VT100 Parser   codex binary (--model mock-model)
    ↓           ↓
Screen State   ACP registry lookup → mock-acp provider
                ↓
            ACP JSON-RPC over stdin/stdout
                ↓
            mock_acp_agent (env var configured)
```

**Key Input Handling:** `Key` enum in `@/codex-rs/tui-pty-e2e/src/keys.rs`

Converts high-level key events to ANSI escape sequences:
- `Key::Enter` → `\r`
- `Key::Escape` → `\x1b`
- `Key::Up/Down/Left/Right` → `\x1b[A/B/D/C`
- `Key::Backspace` → `\x7f`
- `Key::Ctrl('c')` → Control character encoding

**Session Configuration:** `SessionConfig` in `@/codex-rs/tui-pty-e2e/src/lib.rs`

Builder pattern for test environment setup:
- `model` field - Model name to use (defaults to `"mock-model"` which resolves to mock-acp-agent via ACP registry)
- `with_mock_response(text)` - Set `MOCK_AGENT_RESPONSE` env var
- `with_stream_until_cancel()` - Set `MOCK_AGENT_STREAM_UNTIL_CANCEL=1`
- `with_agent_env(key, value)` - Pass custom env vars to mock agent
- `with_approval_policy(policy)` - Set approval policy (defaults to `OnFailure`)
- `without_approval_policy()` - Remove approval policy to test trust screen
- `with_config_toml(content)` - Provide custom config.toml content (overrides default generation)
- `cwd` field - Optional working directory (auto-created temp directory if None)
- `config_toml` field - Optional custom config.toml content (None generates default)

**Approval Policy:** `ApprovalPolicy` enum controls when codex asks for command approval:
- `Untrusted` - Only run trusted commands without approval
- `OnFailure` - Ask for approval only when commands fail (default for tests)
- `OnRequest` - Model decides when to ask for approval
- `Never` - Never ask for approval

By default, all spawned sessions use `ApprovalPolicy::OnFailure` which:
- Skips the trust directory approval screen at startup
- Allows tests to run without manual intervention
- Sets both `--ask-for-approval on-failure` and `--sandbox workspace-write` flags

### Things to Know

**PTY Input Timing Pattern:**

To avoid race conditions between sending input and the TUI processing it, tests add a 100ms delay after `send_str()` and `send_key()` operations when submitting prompts or navigating UI:

```rust
session.send_str("testing!!!").unwrap();
std::thread::sleep(Duration::from_millis(100));
session.send_key(Key::Enter).unwrap();
std::thread::sleep(Duration::from_millis(100));
```

This delay allows the PTY subprocess time to process input and update the display before assertions check for results. The delay is added in test code (not in `TuiSession` methods) for flexibility—not all operations need delays.

**Test Files Structure:**

| File | Coverage |
|------|----------|
| `@/codex-rs/tui-pty-e2e/tests/startup.rs` | TUI initialization, prompt display, trust screen skipping, snapshot testing for 4 startup scenarios, non-blocking PTY verification |
| `@/codex-rs/tui-pty-e2e/tests/prompt_flow.rs` | Prompt submission and agent responses |
| `@/codex-rs/tui-pty-e2e/tests/input_handling.rs` | Text editing, backspace, Ctrl-C clearing, arrow key navigation with snapshot testing |
| `@/codex-rs/tui-pty-e2e/tests/streaming.rs` | Prompt submission with timing delays, agent response streaming |
| `@/codex-rs/tui-pty-e2e/tests/live_acp.rs` | Live authenticated ACP tests for Gemini and Claude with real API connections (opt-in, marked `#[ignore]`) |

**Snapshot Files:**

| File | Test Coverage |
|------|---------------|
| `@/codex-rs/tui-pty-e2e/tests/snapshots/startup__*.snap` | Various startup screen scenarios (welcome, dimensions, temp directory, trust screen) |
| `@/codex-rs/tui-pty-e2e/tests/snapshots/input_handling__*.snap` | Input handling scenarios (ctrl-c clear, typing/backspace, model changed) |
| `@/codex-rs/tui-pty-e2e/tests/snapshots/streaming__submit_input.snap` | Prompt submission and streaming response |

**Snapshot Testing with Insta:**

Tests use `insta::assert_snapshot!()` to capture terminal output for visual regression testing:
```rust
assert_snapshot!("startup_screen", normalize_for_snapshot(session.screen_contents()));
```

Snapshots stored in `@/codex-rs/tui-pty-e2e/tests/snapshots/*.snap` for regression detection. Each snapshot captures the exact terminal output state at a specific test point.

**Snapshot Normalization:**

The `normalize_for_snapshot()` helper function exported from `@/codex-rs/tui-pty-e2e/src/lib.rs` ensures stable snapshots across test runs by replacing dynamic content:

Normalization rules:
1. Temp directory paths (`/tmp/.tmpXXXXXX`) → `[TMP_DIR]` placeholder
2. Random default prompts on lines starting with `› ` → `[DEFAULT_PROMPT]` placeholder
   - Detects specific default prompt patterns: "Find and fix a bug", "Explain this codebase", "Write tests for", etc.
   - Preserves user-entered prompts and UI text like "? for shortcuts"

Implementation in `@/codex-rs/tui-pty-e2e/src/lib.rs:456-488`:
```rust
pub fn normalize_for_snapshot(contents: String) -> String {
    // Replace /tmp/.tmpXXXXXX with [TMP_DIR]
    // Replace known default prompts with [DEFAULT_PROMPT]
    // Preserves UI structure and user input
}
```

This normalization allows snapshot assertions to focus on UI structure and static content rather than ephemeral runtime values. All tests import and use this function consistently: `use tui_pty_e2e::{normalize_for_snapshot, ...};`

**PTY Implementation Details:**

- Uses `portable-pty` crate for cross-platform PTY support
- PTY master is set to **non-blocking mode** using `fcntl(O_NONBLOCK)` on Unix systems
- This prevents `read()` from blocking indefinitely when no data is available
- Sets `TERM=xterm-256color` for terminal feature detection
- NO_COLOR=1 by default for deterministic output parsing
- Terminal size configurable (default 24x80, some tests use 40x120)

**Polling Pattern:**

`poll()` method performs non-blocking read from PTY master:
- PTY file descriptor is set to non-blocking mode during session initialization
- Reads up to 8KB buffer per poll
- Intercepts and responds to terminal control sequences before parsing
- Feeds processed data to VT100 parser incrementally
- Returns immediately with `WouldBlock` error when no data is available
- `wait_for()` loops with 50ms sleep between polls, checking timeout after each iteration
- Timeout mechanism works correctly because `read()` never blocks indefinitely

**Control Sequence Interception:**

The `intercept_control_sequences()` method handles terminal queries that require responses:
- Detects cursor position query (`ESC[6n`) in output stream from codex binary
- Writes cursor position response (`ESC[1;1R`) back to PTY input
- Removes control sequences from parser stream to avoid rendering artifacts
- Enables crossterm terminal initialization without real terminal support

**Mock Agent Integration:**

Tests use the model name `"mock-model"` which the ACP registry (`@/codex-rs/acp/src/registry.rs`) resolves to the mock-acp-agent subprocess. The registry returns configuration with:
- `provider: "mock-acp"`
- `command: <path-to-mock_acp_agent-binary>`
- `args: []`

Tests control mock agent behavior via environment variables:
- `MOCK_AGENT_RESPONSE` - Custom response text instead of defaults
- `MOCK_AGENT_DELAY_MS` - Simulate streaming delays
- `MOCK_AGENT_STREAM_UNTIL_CANCEL` - Stream until Escape pressed

See `@/codex-rs/mock-acp-agent/docs.md` for full list of env vars.

**Binary Discovery:**

`codex_binary_path()` locates the compiled binary:
```
test_exe: target/debug/deps/startup-abc123
          ↓
target/debug/deps (parent)
          ↓
target/debug (parent.parent)
          ↓
target/debug/codex (join "codex")
```

**Live ACP Testing:**

Two opt-in E2E tests in `@/codex-rs/tui-pty-e2e/tests/live_acp.rs` validate integration with real ACP providers:
- `test_gemini_acp_live_response` - Tests gemini-acp with real Gemini API (requires GEMINI_API_KEY environment variable)
- `test_claude_acp_live_response` - Tests claude-acp with real Claude API (requires ANTHROPIC_API_KEY environment variable)
- Both tests are marked `#[ignore]` to be opt-in and run separately: `cargo test --package tui-pty-e2e -- --ignored`
- Use 30-second timeout vs 5-second standard timeout to account for network latency and model processing time
- Generate dynamic config.toml with `wire_api = "acp"` to route through ACP registry
- Verify basic response reception without requiring specific output text

**Known Limitations:**

- VT100 parser may not perfectly emulate all terminal behaviors
- Terminal size changes after spawn not currently supported
- Color codes disabled (NO_COLOR=1) for test determinism

**Dependencies:**

- `portable-pty = "0.8"` - PTY creation and management
- `vt100 = "0.15"` - Terminal emulator/parser
- `insta = "1"` - Snapshot testing framework
- `anyhow = "1"` - Error handling
- `tempfile = "3"` - Temporary directory creation for test isolation
- `nix = "0.27"` (Unix only) - fcntl for non-blocking I/O setup
- `libc = "0.2"` (Unix only) - Low-level fcntl operations

**Debugging:**

Set `DEBUG_TUI_PTY=1` environment variable to enable detailed logging of PTY operations:
```bash
DEBUG_TUI_PTY=1 cargo test test_name -- --nocapture
```

This shows:
- Each `poll()` call and its duration
- Read results (bytes read, WouldBlock, EOF)
- `wait_for()` loop iterations and elapsed time
- Screen contents preview at each iteration

Created and maintained by Nori.
