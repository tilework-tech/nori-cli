# ACP TUI Rendering Fixes — Current Progress

## Completed Specs

### Spec 08: Gemini Empty Content Fallback
- **Commit:** `12f3fae5`
- **Changes:**
  - Location-based invocation fallback in `nori-protocol` normalizer
  - Title sanitization stripping `[current working directory ...]` and trailing `(description text)` from Gemini titles
  - Minimal completed cell rendering shows location paths when no other detail lines

### Spec 06: Artifact Text Output Cleanup
- **Changes:**
  - `format_artifacts` now strips code fence markers from text artifacts via shared `strip_code_fences()` function
  - Removed `Output:` prefix from artifact text — details render as plain text lines
  - Added `is_invocation_redundant()` helper to suppress invocation detail lines that duplicate the title
  - Moved `strip_code_fences` from local function in `client_tool_cell.rs` to shared function in `client_event_format.rs`

### Spec 04: Path Display Normalization (partial)
- **Changes:**
  - `ClientToolCell` now carries a `cwd: PathBuf` field
  - `relativize_paths_in_text()` helper strips cwd prefix from absolute paths in header and invocation lines
  - All tests updated to pass `cwd` parameter

## Remaining Specs (in recommended order)

1. **Spec 04** — Complete remaining path normalization (home-relative fallback, `display_path_for` integration)
2. **Spec 05** — In-progress Edit/Delete/Move rendering
3. **Spec 07** — Diff artifact rendering in ClientToolCell
4. **Spec 02** — Exploring cell grouping (largest, most complex)
