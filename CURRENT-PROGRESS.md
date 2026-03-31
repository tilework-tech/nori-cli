# ACP TUI Rendering — Progress

> Full specification: [APPLICATION-SPEC.md](./APPLICATION-SPEC.md)
> Remaining spec details: [`./specs/`](./specs/)

## Completed (specs 01–10, 12)

All eight initial specs plus specs 09, 10, and 12 are implemented on branch `feat/acp-tui-specs`.

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
| 09 — ACP-Native Approval Rendering | *pending commit* | `AcpTool` approval variant, native overlay/history/fullscreen for non-exec ACP tools |
| 10 — Failed Edit Tool Visibility | `bd51a208` | Red bullet for failed edits, semantic verb headers, error text fallback, duplicate-cell prevention |
| 12 — Execute Cell Completion Buffering | `c23b3af4` | Parallel execute buffering, description text filtering, List dedup |

Tests: 37 unit + 9 integration added. All 1145 existing tests pass.

## Remaining (spec 11)

| # | Spec | File | Status | Blocked by |
|---|------|------|--------|------------|
| 11 | Delete File Operation Bridge | [`specs/11-delete-file-operation-bridge.md`](specs/11-delete-file-operation-bridge.md) | Not started | 10 ✅; 09 ✅ |

### What it fixes

- **Spec 11**: Eliminates `nori_protocol` → `codex_core::protocol::FileChange` compatibility bridge; unifies all file-operation rendering through `ClientToolCell`. Now fully unblocked.
