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
10. Derive neutral surface layers by blending a small contrasting tint into
    the reported terminal background. Use this only when the
    consumer has both the RGB background and true-color support. Otherwise,
    leave backgrounds unset; never substitute absolute indexed shades.
11. Never use a widget-local selection color. Selection is a theme token.

## Lists and pickers

12. Alternate close neutral background shades only in compact, data-dense
    lists. Normal lists use the page background and vertical spacing.
13. Highlight the selected row with primary-accent text and a neutral
    background only slightly brighter than compact row shades. Fill the
    complete data row, including padding, but do not extend row backgrounds
    into titles, headers, search controls, or footer space.
14. Give lists explicit `Compact` and `Normal` density modes.
15. Render compact items as one row. Render normal items as a primary row plus
    an indented description row or equivalent vertical space.
16. Use a marker column, two-cell page inset, and at least one cell between the
    marker and primary content.
17. Remove optional columns in declared priority order as width decreases.
18. Truncate structured cells with `...` or `…`; wrap prose, errors, and logs.
19. Keep selection and filtering in caller-owned state. Components do not own
    terminal setup, event loops, commands, or application actions.
20. Mark search with a text magnifying-glass character, not an emoji. Shade
    only the editable input region after the marker.

## Details and copy

21. Render metadata as an aligned definition list: right-aligned label gutter,
    structural separator, then value. Do not use trailing colons.
22. Group related metadata with spacing or a subtle rule, not another box.
23. Never use em dashes in user-visible TUI copy. Use a hyphen, middot, blank
    value, or explicit phrase such as `Not reported`.
24. Use sentence case and concise action labels.

## Detail panes

`DetailPane` is a presentation-only definition-list widget. Callers provide
its rectangle, choose side or bottom placement, and retain scrolling, focus,
key handling, loading, routing, and application actions. It intentionally does
not make breakpoints or overlay decisions. Handroll adoption is deferred to a
separate consumer migration.

## Verification

25. Snapshot every component at representative wide and narrow widths.
26. Snapshot both densities and loading, empty, error, disabled, and selected
    states where they apply.
27. Pair text snapshots with direct buffer assertions for backgrounds,
    foregrounds, and modifiers. Text snapshots do not preserve Ratatui styles.

## Canonical reference

Run the single full-screen design reference from `nori-rs/`:

```console
cargo run -p codex-tui-components --example nori_storybook
```

The Picker, Markdown, Primitives, and States pages are the visual acceptance
target for this crate. The example owns its event loop and uses production
components only.
