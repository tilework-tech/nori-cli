# Noridoc: codex-apply-patch

Path: @/nori-rs/apply-patch

### Overview

The apply-patch crate implements a custom patch format for AI-driven file modifications. It parses patch instructions, validates them against the filesystem, and applies changes atomically. The format is designed to be LLM-friendly, using clear markers like `*** Begin Patch` and `*** Update File:`.

### How it fits into the larger codebase

This crate is used by:
- `@/nori-rs/core/` - for applying file patches requested by AI models
- `@/nori-rs/acp/` - for patch validation and preview generation

The crate can also be run as a standalone executable for testing.

### Core Implementation

**Patch Format** (`parser.rs`): Parses patches with three hunk types:
- `AddFile` - Create a new file with content
- `DeleteFile` - Remove an existing file
- `UpdateFile` - Modify file contents with optional move

**Shell Detection** (`shell_parsing.rs`, `heredoc.rs`): Detects `apply_patch` invocations from shell scripts:
- Unix shells (bash, zsh, sh) with heredoc syntax (`heredoc.rs`)
- PowerShell
- Windows cmd

Uses tree-sitter-bash for robust AST-based parsing of shell scripts.

**Change Application** (`application.rs`): The `apply_patch()` function:
1. Parses patch into hunks
2. For updates, uses `seek_sequence` to find matching lines
3. Computes replacements and applies them
4. Reports affected files (added, modified, deleted)

**Verified Parsing** (`maybe_parse_apply_patch_verified`): Returns detailed change information including:
- Unified diffs for each file
- New file contents
- Working directory resolution

### Things to Know

**Module Structure:** The crate's `lib.rs` serves as the public API surface and re-exports from submodules: `application.rs` (patch application logic), `shell_parsing.rs` (shell script detection and parsing), `heredoc.rs` (heredoc extraction from shell ASTs), and `tests.rs`.

- The patch format supports context lines (` `), additions (`+`), and deletions (`-`)
- `@@` markers with optional context lines help locate changes in files
- `*** End of File` marker indicates EOF-relative changes
- Unicode normalization handles typographic characters (em-dash, curly quotes) that may appear in LLM output
- The `APPLY_PATCH_TOOL_INSTRUCTIONS` constant provides model-facing documentation

Created and maintained by Nori.
