# Nori TUI design imperatives

These rules are the visual contract for shared Nori terminal components. They
combine the strongest conventions in Nori CLI, Codex-derived views, and
Handroll. New components must follow them; existing components should converge
as they move into this crate.

## Composition

1. Do not draw perimeter boxes around Nori content.
2. Establish hierarchy with spacing, background layers, and alignment.
3. Make information-dense pickers full screen. Center only bounded dialogs and
   short action menus.
4. Place page content inside a consistent two-cell horizontal inset.
5. Use bold, left-aligned, sentence-case titles.
6. Keep context-sensitive key hints centered on the last row.

## Color and emphasis

7. Use one primary accent for focus, navigation, titles, and hint keys.
8. Reserve additional colors for semantic state or identity only.
9. Keep primary copy in the terminal foreground and supporting copy muted.
10. Derive neutral surface layers from the terminal background when the
    consumer can inspect it. Keep the relative contrast between layers small.
11. Never use a widget-local selection color. Selection is a theme token.

## Lists and pickers

12. Alternate the neutral backgrounds of adjacent unselected rows.
13. Fill the complete content row with the selected background, including
    padding. Preserve semantic status foregrounds inside that row.
14. Give lists explicit `Compact` and `Normal` density modes.
15. Render compact items as one row. Render normal items as a primary row plus
    an indented description row or equivalent vertical space.
16. Use a marker column, two-cell page inset, and at least one cell between the
    marker and primary content.
17. Remove optional columns in declared priority order as width decreases.
18. Truncate structured cells with `...` or `…`; wrap prose, errors, and logs.
19. Keep selection and filtering in caller-owned state. Components do not own
    terminal setup, event loops, commands, or application actions.

## Details and copy

20. Render metadata as an aligned definition list: right-aligned label gutter,
    structural separator, then value. Do not use trailing colons.
21. Group related metadata with spacing or a subtle rule, not another box.
22. Never use em dashes in user-visible TUI copy. Use a hyphen, middot, blank
    value, or explicit phrase such as `Not reported`.
23. Use sentence case and concise action labels.

## Verification

24. Snapshot every component at representative wide and narrow widths.
25. Snapshot both densities and loading, empty, error, disabled, and selected
    states where they apply.
26. Pair text snapshots with direct buffer assertions for backgrounds,
    foregrounds, and modifiers. Text snapshots do not preserve Ratatui styles.

## Canonical reference

Run the single full-screen design reference from `nori-rs/`:

```console
cargo run -p codex-tui-components --example nori_storybook
```

The Picker, Markdown, Primitives, and States pages are the visual acceptance
target for this crate. The example owns its event loop and uses production
components only.
