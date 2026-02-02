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

**git**: Ghost commits allow non-destructive workspace snapshots. Also provides worktree management primitives for creating isolated workspaces. Key functions: `create_ghost_commit()`, `restore_ghost_commit()`, `apply_git_patch()`, `create_worktree()`, `ensure_gitignore_entry()`, `generate_worktree_branch_name()`.

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
