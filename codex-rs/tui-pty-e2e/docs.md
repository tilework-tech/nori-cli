# Noridoc: tui-pty-e2e

Path: @/codex-rs/tui-pty-e2e

### Overview

The tui-pty-e2e crate provides end-to-end testing infrastructure for the Nori TUI. It spawns the TUI in a pseudo-terminal and drives it with simulated keyboard input while capturing and validating screen output.

### How it fits into the larger codebase

This is a test-only crate that exercises:
- `@/codex-rs/tui/` - The TUI binary being tested
- `@/codex-rs/mock-acp-agent/` - Mock agent for predictable responses

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

Created and maintained by Nori.
