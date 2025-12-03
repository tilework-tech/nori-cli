# Noridoc: utils/git

Path: @/codex-rs/utils/git

### Overview

The `codex-utils-git` crate provides git repository operations for Codex, including patch application, ghost commit creation/restoration, branch operations, and cross-platform symlink handling. It enables Codex to manage repository state during sessions and apply model-generated diffs.

### How it fits into the larger codebase

This utility crate is used by core and other components for git operations:

- **Core** uses for applying diffs and managing session state
- **Cloud tasks** uses for applying remote task changes
- **Enables** undo functionality via ghost commits

### Core Implementation

**Key Modules:**

| Module | Purpose |
|--------|---------|
| `apply.rs` | Apply git patches via `git apply` |
| `ghost_commits.rs` | Create/restore repository snapshots for undo |
| `branch.rs` | Branch operations (merge-base) |
| `operations.rs` | Common git command wrappers |
| `platform.rs` | Cross-platform symlink creation |

**Ghost Commits:**

The `GhostCommit` type captures repository state:
- Commit ID
- Parent commit (if any)
- Preexisting untracked files/directories

Used to snapshot state before operations and restore on undo.

### Things to Know

**Patch Application:**

`apply_git_patch()` applies unified diffs using `git apply`. `extract_paths_from_patch()` parses affected paths for display.

**Symlink Handling:**

`platform.rs` handles platform-specific symlink creation (Unix vs Windows).

**Error Types:**

`GitToolingError` provides structured errors for git operation failures.

Created and maintained by Nori.
