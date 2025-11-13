# Phase 1: Shared TUI Component Extraction

This plan covers the first tranche of Codex TUI components to extract into the
`tui-components` crate so `nori-cli` (and future consumers) can build a slim
coding agent UI on top of reusable primitives.

The `codex-rs/tui/` package is maintained by a separate upstream team, DO NOT
waste time modifying any of these files. The changes will be ignored.

## Goals

- Copy the lowest-effort, highest-impact primitives out of `codex-rs/tui`.
- Do NOT retain any Codex-specific behavior, only expose generic data
  structures/functionality/config flags through `tui-components`.
- Replace duplicate implementations (e.g., `paste_burst`, `scroll_state`) with
  shared versions to avoid drift.
- Ensure `nori-cli` uses the new components immediately so we validate APIs with
  a second consumer.

## Scope for Phase 1

| Component | Reason | Legacy Overlap | Notes |
| --- | --- | --- | --- |
| `bottom_pane/textarea.rs` | Essential multiline input, minimal Codex logic. | Matches `UI_LEGACY` InputArea. | Needs configurable placeholder + paste hooks. |
| `wrapping.rs` + `live_wrap.rs` helpers | Pure textwrap utilities reused in multiple widgets. | Powers InputArea + instructions text. | Extract as `tui_components::wrapping`. |
| `selection_list.rs` + `bottom_pane/list_selection_view.rs` & `selection_popup_common.rs` | Generic list modal foundation used across Codex and legacy agent selector/install prompts. | Matches `AgentSelectionModal` & `InstallPrompt`. | Introduce neutral `SelectionItem` structs. |
| `bottom_pane/popup_consts.rs` | Shared padding/layout constants. | Used by legacy dropdowns. | Move with selection views. |
| `bottom_pane/command_popup.rs` (generic portion) | Slash-command dropdown logic reuses selection list + scroll state. | Mirrors `AutocompleteDropdown`. | Split Codex-specific command data into adapter layer. |
| `bottom_pane/footer.rs` (shortcut hints) | Legacy InstructionsBar + LoadingIndicator hints. | Provide configurable hint rows. |
| `bottom_pane/paste_burst.rs`, `bottom_pane/scroll_state.rs` | Duplicate of shared crate; consolidate to a single implementation. | N/A | Remove local copies after migration. |

## Work Breakdown

1. **Prep & Dependencies**
   - Ensure `tui-components` exposes feature flags for textwrap, unicode helpers.
   - Confirm `nori-cli` already depends on `tui-components`.

2. **Extract Text Wrapping Utilities**
   - Move `wrapping.rs` APIs (including `RtOptions`, `word_wrap_line`,
     `prefix_lines`) into `tui-components::wrapping`.
   - Port `live_wrap.rs` helpers that only depend on wrapping + Ratatui `Line`.
   - Update Nori to import from the new module; run `just fmt` and targeted
     tests (`cargo test -p tui-components`).

3. **Extract TextArea**
   - Create `tui_components::textarea::{TextArea, TextAreaState}` mirroring the
     Codex implementation but parameterized over:
       - placeholder text
       - optional status/paste callbacks
       - style configuration (pass a theme struct instead of touching `style.rs`)
   - Add snapshot/unit tests under `tui-components/tests`.

4. **Selection & Popup Infrastructure**
   - Introduce a `selection` module exposing:
       - `SelectionList<T>` renderable + scroll state
       - `PopupFrame` helper that consumes `popup_consts`
       - data structs for titles, instructions, and list entries.
   - Move the shared logic from `selection_list.rs`,
     `bottom_pane/list_selection_view.rs`, and `selection_popup_common.rs`
     into the new module.
   - Keep Codex-only copy text (feedback, custom prompts) in adapters that feed
     the shared struct.

5. **Command Popup & Footer Helpers**
   - Rebuild `CommandPopup` on top of the shared selection module, exposing a
     `FilterableList` abstraction for typeahead.
   - Extract `FooterProps`, `FooterMode`, and renderer into a neutral module;
     pass labels/icons from Codex so the shared code only handles layout and
     key-hint formatting.

6. **Consolidate Paste/Scroll Utilities**
   - Delete the duplicate `bottom_pane/paste_burst.rs` and
     `bottom_pane/scroll_state.rs`, replacing imports with the existing shared
     modules.
   - Add docs/tests ensuring the shared versions continue to satisfy Codex’s
     needs (burst thresholds, wrap-around navigation).

7. **Adopt in `nori-cli`**
   - Update the legacy UI described in `UI_LEGACY.md` to optionally use the new
     components, verifying that `nori-cli` renders the InputArea, dropdowns, and
     footer via shared code.
   - Confirm `cargo test -p nori-cli` (or equivalent) passes with the new APIs.

8. **Documentation & Cleanup**
   - Update `tui-components/README.md` with new modules (wrapping, textarea,
     selection, footer).
   - Note migration status inside `UI_COMPONENTS.md` or link back to this plan.
   - Remove obsolete references inside Codex (e.g., `TODO: Extract textarea`).

## Validation

- `cargo test -p tui-components` (including new snapshot tests).
- `cargo test -p nori-cli` (or the relevant crate) to ensure the new APIs meet
  the legacy agent UI requirements.

## Exit Criteria

- `tui-components` documents and exports the new primitives.
- `nori-cli` renders its legacy UI solely through shared components when
  `use_codex_components` is set, proving the APIs are usable outside Codex.
