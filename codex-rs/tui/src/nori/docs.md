# Noridoc: nori

Path: @/codex-rs/tui/src/nori

### Overview

The `nori` module contains Nori-specific TUI customizations that replace or extend the default Codex UI behavior. Currently, the primary component is a branded session header that displays at the start of each TUI session.

### How it fits into the larger codebase

- **Called by** `history_cell.rs` via `new_session_info()` which delegates to `new_nori_session_info()`
- **Replaces** the original `SessionHeaderHistoryCell` (preserved as dead code for potential future feature flag selection)
- **Uses** `HistoryCell` trait from `@/codex-rs/tui/src/history_cell.rs` for consistent rendering
- **Reads** `~/.nori-config.json` for Nori profile information

### Core Implementation

**Session Header (`session_header.rs`):**

The `NoriSessionHeaderCell` struct implements `HistoryCell` and renders:

```
╭──────────────────────────────────────╮
│   _   _  ___  ____  ___              │
│  | \ | \/ _ \|  _ \|_ _\             │
│  |  \| | | | | |_) || |              │
│  | |\  | |_| |  _ < | |              │
│  \_| \_|\___/\_| \_\___|             │
│                                      │
│ version:   v0.x.x                    │
│ directory: ~/path/to/project         │
│ agent:     claude-sonnet             │
│ profile:   senior-swe                │
╰──────────────────────────────────────╯

  Powered by Nori AI

  Run 'npx nori-ai install' to set up Nori AI enhancements
```

**Key functions:**

- `new_nori_session_info()`: Entry point called by `history_cell::new_session_info()`. Creates the composite cell with header + help text
- `read_nori_profile()`: Parses `~/.nori-config.json` to extract `profile.baseProfile`
- `format_directory()`: Relativizes paths to home directory with truncation for narrow terminals

**ASCII Banner Styling:**

The banner uses green+bold for alphabetic characters and dark gray for structural characters (pipes, slashes) to create a two-tone visual effect.

### Things to Know

**Profile Display:**

- When `~/.nori-config.json` contains a `profile.baseProfile`, that value is displayed
- When the file is missing or has no profile, displays "(none)"
- Config parsing is permissive - missing fields or invalid JSON result in `None` profile

**Integration Point:**

The original Codex session header (`SessionHeaderHistoryCell`) is preserved with `#[allow(dead_code)]` annotations. The `new_session_info()` function in `history_cell.rs` unconditionally calls the Nori version. Future work could add a feature flag or config option to toggle between them.

**Width Handling:**

The session header uses a max inner width of 60 characters. Directory paths are center-truncated when they exceed available space (e.g., `~/a/b/…/y/z`).

Created and maintained by Nori.
