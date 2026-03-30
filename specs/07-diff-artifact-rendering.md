# Spec 07: Render Diff Artifacts in ClientToolCell

## Summary

`Artifact::Diff` entries are currently filtered out and never rendered by `format_artifacts`. When a `ClientToolCell` carries diff artifacts (from the ACP `content` array) but takes a code path that doesn't go through `PatchHistoryCell`, the diffs are silently lost. This primarily affects non-completed edits and edge cases where `snapshot_file_changes()` returns `None` despite diffs being present.

## Expected Behavior

When a `ClientToolCell` has `Artifact::Diff` entries, they should be rendered as inline diff summaries — the same colored add/remove format used by `PatchHistoryCell` and `create_diff_summary`. For example:

```
⠋ Editing README.md
    1 -# Nori CLI
    1 +# Nori CLI (TEST EDIT)
    2
    3  [![CI](...)]
```

This gives the user a preview of what the edit will do before it completes (and before the approval response comes back).

## Actual Behavior

`tui/src/client_event_format.rs:92-104`:

```rust
pub(crate) fn format_artifacts(artifacts: &[nori_protocol::Artifact]) -> Vec<String> {
    artifacts
        .iter()
        .filter_map(|artifact| match artifact {
            nori_protocol::Artifact::Diff(_) => None,  // <-- filtered out
            nori_protocol::Artifact::Text { text } if text.is_empty() => None,
            nori_protocol::Artifact::Text { text } if text.contains('\n') => {
                Some(format!("Output:\n{text}"))
            }
            nori_protocol::Artifact::Text { text } => Some(format!("Output: {text}")),
        })
        .collect()
}
```

`Artifact::Diff(_) => None` means all diff artifacts are silently discarded during rendering.

## Wire Protocol Evidence

Edit diffs arrive in the `content` array during the in-progress phase:

`screen-examples-new/debug-acp-claude.log:30`:
```json
{
  "toolCallId": "toolu_01GVv61PnjfeusQp81t62iUw",
  "sessionUpdate": "tool_call_update",
  "kind": "edit",
  "content": [{
    "type": "diff",
    "path": "/home/.../README.md",
    "oldText": "# Nori CLI",
    "newText": "# Nori CLI (TEST EDIT)"
  }]
}
```

These diffs are parsed into `Artifact::Diff(FileChange { ... })` by the normalizer (`lib.rs:426-429`) but then dropped during rendering.

Gemini also sends diffs in the approval request:

`screen-examples-new/debug-acp-gemini.log:25`:
```json
{
  "toolCall": {
    "toolCallId": "replace-1774849809779",
    "title": "README.md: # Nori CLI => # Nori Agent CLI",
    "content": [{
      "type": "diff",
      "path": ".../README.md",
      "oldText": "# Nori CLI\n...",
      "newText": "# Nori Agent CLI\n..."
    }]
  }
}
```

## Affected Code

- **`tui/src/client_event_format.rs:96`** — `Artifact::Diff(_) => None` discards all diffs
- **`tui/src/client_event_format.rs:129-157`** — `snapshot_file_changes` converts diffs to `codex_core::protocol::FileChange` (used by the edit path), but this function is only called for completed edits
- **`tui/src/diff_render.rs:177-184`** — `create_diff_summary` can render `HashMap<PathBuf, FileChange>` to `Vec<Line>`, ready to reuse

## Scope

1. In `format_artifacts` (or better, as a separate rendering method in `ClientToolCell`), convert `Artifact::Diff` entries to `codex_core::protocol::FileChange` using the existing `snapshot_file_changes` helper
2. Render the diffs using `create_diff_summary` to produce ratatui `Line`s with colored add/remove formatting
3. This is most useful for in-progress edits (spec 05) where the diff preview shows what's about to change while waiting for approval. For completed edits that already render through `PatchHistoryCell`, the diff artifacts are redundant and can continue to be omitted.
4. For `ClientToolCell` instances that are not edits but happen to carry diff artifacts (unlikely but possible), render them as a secondary section after the invocation/output lines
