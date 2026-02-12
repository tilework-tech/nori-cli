# Noridoc: codex-file-search

Path: @/codex-rs/file-search

### Overview

The file-search crate provides fast fuzzy file finding for Nori. It walks the filesystem respecting gitignore rules and uses the `nucleo` matcher for relevance scoring.

### How it fits into the larger codebase

Used by `@/codex-rs/tui/` (`file_search.rs`) to power the file picker UI when users need to reference files in prompts.

### Core Implementation

**Parallel Walking**: Uses `ignore::WalkBuilder` (from ripgrep) for efficient parallel directory traversal. Respects `.gitignore` patterns by default.

**Git-Aware Gitignore Handling**: Before walking, `find_git_root()` locates the git repository root by walking up from the search directory looking for `.git` (supports both regular repos and worktrees where `.git` is a file). The walker starts from the git root with `.parents(false)` to prevent reading gitignore files from parent directories outside the repository. Results are filtered to only include files within the original search directory. This prevents unrelated parent repositories (e.g., a company monorepo root with `*` in its gitignore) from affecting search results.

**Fuzzy Matching**: Uses `nucleo_matcher` with:
- Smart case matching
- Smart normalization
- Fuzzy atom kind

**Result Management**: Each worker thread maintains a `BestMatchesList` min-heap to track top matches. Results are merged across threads at the end.

**Match Output** (`FileMatch`):
- `score` - Relevance score from matcher
- `path` - Relative path to matched file
- `indices` - Optional highlight positions (when `compute_indices=true`)

### Things to Know

- Follows symbolic links by default
- Hidden files are included (`.hidden` is searched)
- Gitignore traversal is limited to the current git repository boundary
- If search directory is not in a git repo, walks from search directory directly
- Cancellation is supported via `cancel_flag` atomic
- Check interval of 1024 files between cancel flag checks
- Tie-breaking sorts by ascending path when scores are equal
- Can be run as a CLI tool for testing

Created and maintained by Nori.
