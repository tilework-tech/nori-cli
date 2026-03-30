# ACP TUI Rendering — Progress

> Full specification: [APPLICATION-SPEC.md](./APPLICATION-SPEC.md)
> Remaining spec details: [`./specs/`](./specs/)

## Completed (specs 01–08, 12)

All eight initial specs plus spec 12 are implemented on branch `feat/acp-tui-specs`.

| Spec | Commit | Summary |
|------|--------|---------|
| 01 — Execute Native Rendering | `512c505e` | Semantic verbs, bash highlighting, exit-code bullet, output truncation |
| 02 — Exploring Cell Grouping | `2a482c09` | Multi-snapshot exploring cells, grouped reads, Search/List sub-items |
| 03 — Codex Command Array Extraction | `cc12bf6b` | Codex `rawInput.command` array → `Invocation::Command` |
| 04 — Path Display Normalization | `f4320a7e` | cwd-relative paths in titles and invocations |
| 05 — In-Progress Edit Rendering | `94268dc0` | Spinner for pending edits, clean transition to PatchHistoryCell |
| 06 — Artifact Text Cleanup | `771bca1a` | Code fence stripping, redundant invocation suppression |
| 07 — Diff Artifact Rendering | `7e7e9f96` | Inline diff previews for in-progress edits |
| 08 — Gemini Empty Content Fallback | `12f3fae5` | Location fallback invocations, Gemini title sanitization |
| 12 — Execute Cell Completion Buffering | *pending commit* | Parallel execute buffering, description text filtering, List dedup |

Tests: 22 unit + 9 integration added. All 1131 existing tests pass.

## Remaining (specs 09–11)

| # | Spec | File | Status | Blocked by |
|---|------|------|--------|------------|
| 10 | Failed Edit Tool Visibility | [`specs/10-failed-edit-tool-visibility.md`](specs/10-failed-edit-tool-visibility.md) | Not started | — |
| 09 | ACP-Native Approval Rendering | [`specs/09-acp-native-approval-rendering.md`](specs/09-acp-native-approval-rendering.md) | Not started | — |
| 11 | Delete File Operation Bridge | [`specs/11-delete-file-operation-bridge.md`](specs/11-delete-file-operation-bridge.md) | Not started | 10; approval bridge waits for 09 |

### What each fixes

- **Spec 10**: Failed edits have dim bullet (not red), generic header (not semantic verb), no error detail.
- **Spec 09**: Approval history shows `✔ You approved Nori to runrm...` (missing space, raw command); overlay wrong for non-execute tools.
- **Spec 11**: Eliminates `nori_protocol` → `codex_core::protocol::FileChange` compatibility bridge; unifies all file-operation rendering through `ClientToolCell`.
