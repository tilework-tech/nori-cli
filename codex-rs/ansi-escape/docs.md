# Noridoc: ansi-escape

Path: @/codex-rs/ansi-escape

### Overview

The `codex-ansi-escape` crate provides utilities for parsing and handling ANSI escape sequences. It's used for processing terminal output that may contain color codes, cursor movement, and other control sequences.

### How it fits into the larger codebase

ANSI escape is used by the TUI for terminal output processing:

- **Output processing** strips or preserves ANSI codes as needed
- **Terminal rendering** handles escape sequences properly

### Core Implementation

`lib.rs` provides:
- ANSI escape sequence detection
- Code stripping utilities
- Sequence parsing

### Things to Know

Used when processing output from commands to ensure proper display or storage without escape codes when not appropriate.

Created and maintained by Nori.
