# Noridoc: tui-pty-e2e

Path: @/codex-rs/tui-pty-e2e

### Overview

The tui-pty-e2e crate provides end-to-end testing infrastructure for the Nori TUI. It spawns the TUI in a pseudo-terminal and drives it with simulated keyboard input while capturing and validating screen output.

### How it fits into the larger codebase

This is a test-only crate that exercises:
- `@/codex-rs/tui/` - The TUI binary being tested
- `@/codex-rs/mock-acp-agent/` - Mock agent for predictable responses

Tests validate rendering behavior end-to-end by checking the actual terminal screen buffer contents, including the ordering and presence/absence of cells (tool output, agent text, approval prompts). This catches integration issues that unit tests on individual components would miss, such as race conditions between streaming text and tool event rendering.

### Core Implementation

**PTY Management**: Uses `portable_pty` to create a pseudo-terminal with:
- Configurable terminal size
- Input writing capability
- Output capture

**Terminal Parsing**: Uses `vt100::Parser` to interpret ANSI escape sequences and maintain a virtual screen buffer.

**Test Utilities**:
- `wait_for_text()` - Block until expected text appears on screen
- `send_keys()` - Simulate keyboard input
- `get_screen_content()` - Capture current display state

**Tool Call Rendering Tests** (`acp_tool_calls.rs`):

Tests in this file verify that tool call events (Explored, Ran, Searched cells) render in the correct positions relative to agent text. Key test patterns include:
- Verifying tool calls that complete before text streaming appear above the agent message
- Verifying that tool call completions arriving during the final text stream are NOT rendered after the agent's response (the `MOCK_AGENT_TOOL_CALLS_DURING_FINAL_STREAM` scenario)
- Checking for absence of trailing tool output by asserting that screen content after the final agent message position contains no tool-related strings
- Verifying that cascade-deferred tool events do not produce orphan cells (the `MOCK_AGENT_ORPHAN_TOOL_CELLS` scenario), where a Begin is deferred due to a non-empty queue and later discarded, but its End must also be discarded to avoid raw call_id rendering
- Verifying that generic tool calls with no `raw_input` (the `MOCK_AGENT_GENERIC_TOOL_CALL` scenario) display a resolved semantic name from `ev.command` instead of the raw tool call ID, covering the case where the ACP translator skips `ExecCommandBegin` entirely

**Debug Output**: Colorized output (via `owo-colors`) for test debugging:
- Sent input highlighted
- Expected vs actual screen content
- Timing information

### Things to Know

- Tests require the `vt100-tests` feature enabled in nori-tui
- The mock agent is spawned as the ACP backend
- Screen capture includes full ANSI state (colors, attributes)
- Timing-sensitive tests use configurable timeouts
- Debug styles respect color terminal detection
- Snapshot tests use `insta` for visual verification of screen output; snapshots live in `tests/snapshots/`
- `normalize_for_input_snapshot()` normalizes dynamic content before snapshot comparison: session timestamps/IDs become `[TIMESTAMP]`/`[SESSION_ID]`, and the randomly selected whimsical status indicator header becomes `[STATUS]`. It also collapses runs of consecutive blank lines into a single blank line, because PTY timing can cause the exact count of blank lines between content sections to vary between runs. Tests that check for the status indicator being active use `"esc to interrupt"` as the stable anchor text rather than any specific status message.

Created and maintained by Nori.
