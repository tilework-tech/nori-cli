# Noridoc: apply-patch

Path: @/codex-rs/apply-patch

### Overview

The `codex-apply-patch` crate implements the custom patch format used by Codex for structured file modifications. It parses patch definitions, validates them against the filesystem, computes unified diffs for display, and applies changes atomically. This format is simpler than unified diff and designed for LLM-generated edits.

### How it fits into the larger codebase

Apply-patch is a core tool used throughout Codex:

- **Core** tool handler at `@/codex-rs/core/src/tools/handlers/apply_patch.rs`
- **TUI** uses `unified_diff_from_chunks()` for diff display
- **CLI** provides `codex apply` command via `codex-chatgpt` integration
- **Model instructions** reference `APPLY_PATCH_TOOL_INSTRUCTIONS`

### Core Implementation

**Patch Format:**

```
*** Begin Patch
*** Add File: path/to/new.txt
+line 1
+line 2

*** Delete File: path/to/remove.txt

*** Update File: path/to/modify.txt
*** Move to: path/to/renamed.txt  (optional)
@@
 context line
-old line
+new line
*** End Patch
```

**Parsing Pipeline:**

1. `parse_patch()` in `parser.rs` -> `ApplyPatchArgs { patch, hunks }`
2. `maybe_parse_apply_patch()` -> Detect if args are apply_patch call
3. `maybe_parse_apply_patch_verified()` -> Validate against filesystem

**Key Types:**

```rust
pub enum Hunk {
    AddFile { path, contents },
    DeleteFile { path },
    UpdateFile { path, move_path, chunks },
}

pub enum ApplyPatchFileChange {
    Add { content },
    Delete { content },
    Update { unified_diff, move_path, new_content },
}
```

### Things to Know

**Bash Heredoc Parsing:**

The crate handles `bash -lc` scripts with heredocs:
```bash
cd /path && apply_patch <<'EOF'
*** Begin Patch
...
*** End Patch
EOF
```

Uses Tree-sitter Bash grammar for reliable parsing of this pattern.

**Seek Sequence Matching:**

`seek_sequence.rs` implements fuzzy line matching that:
- Handles Unicode punctuation normalization (EN DASH -> ASCII hyphen)
- Finds context lines when exact positions aren't specified
- Supports `*** End of File` marker for EOF additions

**Unified Diff Generation:**

`unified_diff_from_chunks()` converts Codex patch format to standard unified diff for display, using the `similar` crate's `TextDiff`.

**Error Handling:**

```rust
pub enum ApplyPatchError {
    ParseError(ParseError),
    IoError(IoError),
    ComputeReplacements(String),  // Match failures
    ImplicitInvocation,  // Raw patch without apply_patch call
}
```

**Standalone Executable:**

`standalone_executable.rs` enables running apply-patch as a separate binary for direct patch application outside of Codex.

**Tool Instructions:**

`APPLY_PATCH_TOOL_INSTRUCTIONS` constant contains detailed documentation embedded in model system prompts, loaded from `apply_patch_tool_instructions.md`.

Created and maintained by Nori.
