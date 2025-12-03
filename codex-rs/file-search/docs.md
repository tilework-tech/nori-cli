# Noridoc: file-search

Path: @/codex-rs/file-search

### Overview

The `codex-file-search` crate provides fast file search utilities for finding files by name patterns. It uses the `ignore` crate for gitignore-aware traversal and supports fuzzy matching.

### How it fits into the larger codebase

File search is used by TUI and app-server:

- **TUI** file picker uses for fuzzy file finding
- **App-server** `fuzzy_file_search.rs` uses for IDE autocomplete
- **Respects** gitignore patterns

### Core Implementation

**Key Files:**

- `lib.rs`: Core search functionality
- `cli.rs`: Command-line interface
- `main.rs`: Standalone binary

### Things to Know

**Gitignore Aware:**

Uses `ignore` crate which respects:
- `.gitignore`
- `.ignore`
- Hidden files

**Fuzzy Matching:**

Integrates with `codex-common` fuzzy matching for ranked results.

Created and maintained by Nori.
