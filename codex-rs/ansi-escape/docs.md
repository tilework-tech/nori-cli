# Noridoc: codex-ansi-escape

Path: @/codex-rs/ansi-escape

### Overview

The ansi-escape crate provides utilities for converting ANSI escape sequences to Ratatui `Text` and `Line` types. This enables proper rendering of colorized terminal output in the TUI.

### How it fits into the larger codebase

Used by `@/codex-rs/tui/` to render command output that contains ANSI color codes (e.g., from `ls --color` or test runners).

### Core Implementation

**ansi_escape()**: Converts a string with ANSI sequences to Ratatui `Text<'static>`. Uses the `ansi_to_tui` crate for parsing.

**ansi_escape_line()**: Same as `ansi_escape()` but expects single-line input. Logs a warning if multiple lines are found and returns only the first.

**expand_tabs()**: Replaces tabs with 4 spaces to avoid visual artifacts with TUI gutter prefixes.

### Things to Know

- Tab expansion is applied automatically to avoid alignment issues
- Parse errors cause a panic (should not happen with valid ANSI)
- Returns owned `Text<'static>` to avoid lifetime complexity
- The `ansi_to_tui` crate's `to_text()` was avoided due to lifetime issues

Created and maintained by Nori.
