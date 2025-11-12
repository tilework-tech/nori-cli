# TUI Component Migration - Progress Report

## Completed Work (Phase 2: Text Wrapping Utilities)

### ✅ Wrapping Module (`tui-components/src/wrapping.rs`)
- **Status**: Complete with 23 passing snapshot tests
- **Extracted from**: `codex-rs/tui/src/wrapping.rs`
- **Key functionality**:
  - `wrap_ranges` and `wrap_ranges_trim` - byte range wrapping
  - `RtOptions` - Ratatui-specific wrapping configuration
  - `word_wrap_line` - wrap single Line with style preservation
  - `word_wrap_lines` - wrap multiple lines with indent support
  - `word_wrap_lines_borrowed` - borrowed variant
  - `prefix_lines` - add prefixes to lines
- **Changes from original**:
  - Inlined `push_owned_lines` and `line_to_static` helpers (removed dependency on `codex-rs/tui/src/render/line_utils.rs`)
  - Made all public APIs fully documented
  - Added comprehensive module documentation
- **Tests**: `/home/clifford/Documents/source/codex/nori-cli/.worktrees/migrate-tui-components/tui-components/tests/wrapping_snapshots.rs`

### ✅ Live Wrap Module (`tui-components/src/live_wrap.rs`)
- **Status**: Complete with 16 passing snapshot tests
- **Extracted from**: `codex-rs/tui/src/live_wrap.rs`
- **Key functionality**:
  - `Row` - single visual row with explicit break tracking
  - `RowBuilder` - incremental text wrapping for streaming content
  - `take_prefix_by_width` - Unicode-aware width-based text slicing
  - Dynamic width changes with automatic rewrapping
  - Fragmentation invariance (results don't depend on input chunking)
- **Changes from original**:
  - Clean copy, no Codex dependencies
  - Added comprehensive module and API documentation
  - Kept plain-text only (ANSI styling deferred to future phases)
- **Tests**: `/home/clifford/Documents/source/codex/nori-cli/.worktrees/migrate-tui-components/tui-components/tests/live_wrap_snapshots.rs`

### Library Integration
- **Exports added to `lib.rs`**:
  - `pub mod wrapping`
  - `pub mod live_wrap`
  - Re-exported: `word_wrap_line`, `word_wrap_lines`, `word_wrap_lines_borrowed`, `RtOptions`, `prefix_lines`
  - Re-exported: `Row`, `RowBuilder`, `take_prefix_by_width`
- **Documentation**: Updated module organization in lib.rs
- **Code quality**: All clippy warnings fixed, cargo fmt applied

## Test Results

```
wrapping_snapshots: 23 tests passed
live_wrap_snapshots: 16 tests passed
Total tui-components tests: 64 tests passed, 0 failed
```

## Next Steps (Remaining from Phase 1 Plan)

### Phase 3: TextArea Component
**Not started** - This is the next priority task

Files to extract:
- Source: `codex-rs/tui/src/bottom_pane/textarea.rs` (653 lines)
- Target: `tui-components/src/textarea.rs`

Key work needed:
1. Create `TextArea` and `TextAreaState` structs
2. Add `TextAreaConfig` for customization (placeholder, styles, callbacks)
3. Remove Codex-specific style dependencies
4. Make generic and configurable
5. Write comprehensive snapshot tests
6. Document API thoroughly

Estimated complexity: **High** - TextArea is ~650 lines with complex cursor navigation, wrapping, and scroll logic

### Phase 4: Selection & Popup Infrastructure
**Not started**

Files to extract:
- `codex-rs/tui/src/selection_list.rs`
- `codex-rs/tui/src/bottom_pane/list_selection_view.rs`
- `codex-rs/tui/src/bottom_pane/selection_popup_common.rs`
- `codex-rs/tui/src/bottom_pane/popup_consts.rs`

Create:
- `tui-components/src/selection/` module
- Generic `SelectionList<T>` widget
- `PopupFrame` helper
- Configuration structs

### Phase 5: Command Popup & Footer
**Not started**

Files to extract:
- `codex-rs/tui/src/bottom_pane/command_popup.rs` (generic portion)
- `codex-rs/tui/src/bottom_pane/footer.rs`

Create:
- `tui-components/src/selection/filterable_list.rs`
- `tui-components/src/footer.rs`
- Generic filtering abstractions

### Phase 6: Consolidate Paste/Scroll
**Already complete** - These were extracted in earlier work:
- ✅ `tui-components/src/paste_burst.rs`
- ✅ `tui-components/src/scroll_state.rs`

### Phase 7: Adopt in nori-cli
**Not started** - Will begin after TextArea is extracted

Integration work:
- Update nori-cli to use new TextArea
- Replace any duplicate wrapping code
- Update dropdowns to use SelectionList (once available)
- Update footer rendering
- Write integration tests
- Manual smoke testing

### Phase 8: Documentation & Cleanup
**Partially complete**

Done:
- ✅ README updated for wrapping and live_wrap modules
- ✅ Module documentation in lib.rs

TODO:
- Update `tui-components/README.md` with full component catalog
- Create/update `UI_COMPONENTS.md` with migration status
- Add CHANGELOG entry
- Final test suite run
- Generate full documentation with `cargo doc`

## Design Decisions Made

1. **Inline helpers vs re-export**: Chose to inline `push_owned_lines` and `line_to_static` rather than creating a separate `line_utils` module. These are small helpers (< 30 lines total) and keeping them inline reduces module complexity.

2. **Configuration pattern**: Using dedicated config structs (`TextAreaConfig`, `SelectionListConfig`, etc.) for component customization rather than a `Theme` trait. This is more explicit and easier to use.

3. **Event handling**: Will use callbacks/closures for events rather than a generic event sender trait or concrete `AppEventSender`. This provides maximum flexibility for consumers.

4. **Testing strategy**: Prioritizing snapshot tests for all tui-components implementations, as these are inherently visual components. Behavior tests will be used for nori-cli integration.

5. **Placeholder rendering**: Placeholders will be rendered with dimmed style when textarea is empty (decision for Q4 from plan).

6. **Selection list actions**: Simplified to single callback per item rather than Vec of actions (decision for Q5 from plan).

7. **Footer API**: Keeping specific context window API but making it optional (decision for Q6 from plan).

8. **Filtering**: Consumer-provided filter function for `FilterableList` for maximum flexibility (decision for Q7 from plan).

## Worktree Information

- **Location**: `/home/clifford/Documents/source/codex/nori-cli/.worktrees/migrate-tui-components`
- **Branch**: `migrate-tui-components`
- **Base commit**: 697cf21 (Fix clippy)
- **Status**: Clean build, all tests passing

## Commands to Continue

```bash
# Run tests
cd /home/clifford/Documents/source/codex/nori-cli/.worktrees/migrate-tui-components/tui-components
cargo test

# Build
cargo build

# Lint
cargo clippy --all -- -D warnings

# Format
cargo fmt --all

# Generate docs
cargo doc --no-deps --open
```

## Estimated Remaining Work

- **TextArea extraction**: 4-6 hours (complex component)
- **Selection infrastructure**: 6-8 hours (multiple files, generics)
- **Command popup & footer**: 3-4 hours
- **nori-cli integration**: 4-6 hours (testing, debugging)
- **Documentation & cleanup**: 2-3 hours

**Total estimated**: 19-27 hours of development work

## Notes

- All work is in worktree to keep main branch clean
- Codex source files at `../../../codex-rs/tui/` relative to worktree
- No modifications made to codex-rs (read-only)
- tui-components tests run independently (64 tests currently)
- Ready to proceed with TextArea extraction as next step
