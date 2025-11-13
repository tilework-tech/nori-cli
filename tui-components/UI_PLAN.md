# UI Plan

## Overview

- UI_COMPONENTS.md, documenting every Codex-side TUI component, grouped by area
  (with purpose, complexity, key dependencies, and Codex-only behaviors):
  - infrastructure
  - bottom-pane stack
  - transcript/history
  - rendering utilities
  - markdown/diff
  - status overlays
  - onboarding
  - and supporting glue
- Some pieces already live in tui-components
  (render, key_hint, shimmer, scroll_state, paste_burst)

---

## Priority, complexity, and coupling

- Immediate wins – textarea.rs, wrapping.rs, selection_list.rs, bottom_pane/popup_consts.rs,
  and the duplicate paste_burst.rs/scroll_state.rs under bottom_pane. They’re pure Ratatui/
  textwrap helpers with minimal Codex glue; a basic coding agent needs a multiline input,
  text wrapping, and list popups for commands, so extracting them first gives the most
  leverage with the least risk.
- Next tier – bottom_pane/list_selection_view.rs + selection_popup_common.rs, bottom_pane/
  footer.rs, and public_widgets/composer_input.rs. They add richer UX (selection modals,
  shortcut hints, reusable composer widget) and only touch Codex state via strings/hints, so
  factoring their data inputs makes them good early candidates for reuse in a simple agent
  UI.
- Keep local for now – chat_composer.rs, chatwidget/, history_cell.rs, diff_render.rs, and
  status/. These are deeply coupled to Codex session data, plan updates, and tool semantics;
  a stripped-down coding agent can postpone them until there’s a stable cross-app data
  contract.

### Overlap with existing TUI

- High overlap – bottom_pane/textarea.rs (legacy InputArea), chatwidget/session_header.rs &
  status_indicator_widget.rs (AgentInfo), bottom_pane/footer.rs (InstructionsBar & shortcut
  hints), bottom_pane/list_selection_view.rs + selection_list.rs (AgentSelectionModal
  & InstallPrompt modal list handling), and bottom_pane/command_popup.rs
  (AutocompleteDropdown); these map almost one-to-one with the legacy doc.
- Moderate overlap – bottom_pane/chat_composer.rs (wraps InputArea logic plus legacy
  layout), ascii_animation.rs / shimmer.rs (LoadingIndicator), and bottom_pane/mod.rs layout
  logic (ChatLayout orchestration); they include extra Codex wiring but embody the same
  behaviors described in UI_LEGACY.md.
- Low overlap – other modules (status cards, history cells, onboarding) extend beyond
  the legacy list and don’t have direct counterparts in UI_LEGACY.md, so they can be
  deprioritized when reconciling with the legacy components.

