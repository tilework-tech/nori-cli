# Noridoc: codex-utils

Path: @/codex-rs/utils

### Overview

The utils directory contains small, focused utility crates that provide shared functionality across the Nori codebase. These crates are designed to be lightweight and independent.

### How it fits into the larger codebase

These utilities are consumed by various crates throughout the workspace, primarily `@/codex-rs/core/` and `@/codex-rs/tui/`.

### Utility Crates

| Crate | Description |
|-------|-------------|
| `cache` | Thread-safe LRU cache with SHA1 hashing |
| `git` | Git operations (patches, ghost commits, branch operations, worktree management) |
| `image` | Image processing (resize, encode, base64) |
| `json-to-toml` | Converts JSON values to TOML values |
| `pty` | PTY session management for command execution |
| `readiness` | Async readiness flag with token-based authorization |
| `string` | String truncation at char boundaries |

### Core Implementations

**cache**: `BlockingLruCache<K, V>` provides get-or-insert semantics with Tokio mutex protection. Includes `sha1_digest()` for content hashing.

**git**: Ghost commits allow non-destructive workspace snapshots. Also provides worktree management primitives for creating, naming, and renaming isolated workspaces. Key functions: `create_ghost_commit()`, `restore_ghost_commit()`, `apply_git_patch()`, `create_worktree()`, `ensure_gitignore_entry()`, `generate_worktree_branch_name()`, `summary_to_branch_name()`, `rename_worktree_branch()`.

`summary_to_branch_name()` converts a prompt summary string into a git-branch-safe slug with `auto/` prefix and timestamp (e.g., "Fix auth bug" becomes `auto/fix-auth-bug-20260202-120000`). It sanitizes non-alphanumeric characters, collapses consecutive hyphens, truncates at 40 characters on word boundaries, and falls back to `generate_worktree_branch_name()` for empty input. `rename_worktree_branch()` performs a `git branch -m` to rename the branch in place. The worktree directory is left unchanged so that processes running inside it are not disrupted.

**image**: Resizes images to `MAX_WIDTH=2048` / `MAX_HEIGHT=768` and encodes as JPEG/PNG with base64. Uses LRU cache to avoid re-encoding.

**pty**: `ExecCommandSession` manages PTY-based command execution with async I/O channels for input/output streaming.

**readiness**: `ReadinessFlag` allows multiple subscribers to mark ready, with async waiting via Tokio watch channels.

**string**: `take_bytes_at_char_boundary()` and `take_last_bytes_at_char_boundary()` for safe UTF-8 truncation.

### Things to Know

- All crates are designed for async (Tokio) usage
- The `git` crate handles cross-platform symlink creation
- Image caching uses SHA1 digests as keys
- PTY sessions support exit status tracking and async output broadcasting

Created and maintained by Nori.
