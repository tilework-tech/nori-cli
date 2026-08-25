# Nori TUI design imperatives

These rules are the visual contract for shared Nori terminal components. They
combine the strongest conventions in Nori CLI, Codex-derived views, and
Handroll. New components must follow them; existing components should converge
as they move into this crate.

## Composition

1. Never box ordinary Nori content. Reserve perimeter frames for exceptional
   containment: an overlay modal or popover, a complex inline diff note, or the
   prompt-card motif in chat. Use spacing, background surfaces, and alignment
   for ordinary grouping.
2. Establish hierarchy with spacing, background layers, and alignment.
3. Make information-dense pickers full screen. Center only bounded dialogs and
   short action menus.
4. Place page content inside a consistent two-cell horizontal inset.
5. Use bold, left-aligned, sentence-case titles in the terminal foreground.
   Titles establish hierarchy and never consume a functional accent color.
6. Keep context-sensitive key hints centered on the last row.

## Color and emphasis

7. Green is the primary pointer accent. Reserve it for compact indicators on
   interactable elements and current focus, such as row indicators, shortcut
   keys, focused borders, and equivalent interaction signals. Do not color an
   entire control, title, or passive structure green solely to establish
   hierarchy.
8. Cyan is a targeted informational or secondary accent, not a supporting-copy
   color. Apply cyan and colors that communicate semantic state or identity to
   the smallest meaningful target: a marker, key, value, status, or compact
   cell. Color may be loud at that target; it must not wash unrelated copy or
   structural regions.
9. Keep titles and primary copy in the terminal foreground, supporting copy
   readable but muted, and disabled copy distinctly dimmer than supporting
   text.
10. Derive neutral surface layers by blending a small contrasting tint into
    the reported terminal background. Use this only when the
    consumer has both the RGB background and true-color support. Otherwise,
    leave backgrounds unset; never substitute absolute indexed shades.
11. Never use a widget-local selection color. Selection is a theme token.

## Lists and pickers

12. Alternate close neutral background shades only in compact, data-dense
    lists. Normal lists use the page background and vertical spacing.
13. Highlight the selected row with a neutral background only slightly brighter
    than compact row shades, terminal-foreground primary text, and the
    theme's green pointer accent. Fill the complete data row,
    including padding, but do not extend row backgrounds into titles, headers,
    search controls, or footer space. Do not introduce a universal leading
    rail or repeated edge glyphs in transcript, history, file, command-output,
    or other copyable rows.
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

Keep the inactive search affordance concise: display `/ search`, even when a
consumer accepts additional activation aliases such as `f` and Ctrl-F. The
visible hint communicates the conventional binding without becoming an
exhaustive keymap; once search is active, printable keys belong to the query.

Provider identity is explicit metadata, never inferred from display labels.
Provider-toned category tabs retain their identity color when active and add
bold emphasis. Provider-toned cells use the same identity mapping, while
selection and disabled styles remain the stronger interaction signals.

## Overlay menus

An overlay menu is a bounded list of actions, not a picker with different
placement. Use a picker for search and filtering across potentially large data
sets. Use an overlay menu when the choices are few enough to scan directly.

The reusable menu boundary is deliberately narrow:

- The consumer owns terminal initialization and restoration, raw input and its
  mapping to `MenuAction`, event polling, render cadence, application actions
  and routing, focus and modal stacks, confirmation policy, asynchronous work,
  and persistence.
- `MenuState` owns only validated items, the selected item, and menu-local
  viewport position. `MenuOutcome` reports selection, activation, or
  cancellation through the caller's stable item key; it never dispatches an
  application event.
- `OverlayMenu` receives a caller-owned `Rect`, centers a bounded surface
  within that rectangle, and updates only the viewport bookkeeping needed to
  keep selection visible. It does not assume that the rectangle is the whole
  terminal.

Overlay menus follow these interaction rules:

1. Navigation wraps and skips disabled items. Empty and all-disabled menus have
   no selection, and disabled items cannot be activated by selection or a
   shortcut.
2. Character mnemonics are explicit ASCII letters, match the first visible
   character of the label case-insensitively, and are rendered in bold without
   changing the label. Number shortcuts are explicit values from `1` through
   `9` and appear in one aligned column when any item has one.
3. Stable keys and both shortcut families are unique across the complete menu,
   including disabled items. An item may expose both shortcut kinds; either
   activates that item immediately. The consumer's raw-key adapter decides
   whether an input is navigation, a character mnemonic, or a number shortcut,
   so no additional precedence exists inside the component.
4. Consequence tones color an unselected item's identity. Selection always
   uses the green pointer accent, even for warning and destructive actions.

Overlay menu layout responds to the caller's rectangle:

- The centered surface is at most 58 cells wide by default and never exceeds
  the supplied rectangle. Widths of at least 34 cells reserve a two-cell outer
  margin; narrower rectangles surrender that margin before truncating primary
  labels. The title is retained whenever any content can render.
- Supporting subtitles require at least 40 content cells and a height of 14
  rows. They disappear before labels or item descriptions when space is tight.
- Descriptions wrap by Unicode display width to at most two rows. Structured
  labels and cells truncate with an ellipsis only when they cannot fit. A blank
  row separates menu items; items without descriptions reserve equivalent
  vertical rhythm.
- Content-derived height is capped by the caller's height. When the list does
  not fit, the selected item remains visible and muted top or bottom overflow
  markers communicate that more items exist.
- Key hints remain centered at the bottom of the surface and use at most two
  rows. Their height is clamped to the content remaining below the title, so
  even tiny caller rectangles with non-zero origins stay within bounds.
  Header, list, and footer spacing compress before essential content is
  removed.

The overlay may shade only the caller-provided area. `backdrop` and
`menu_surface` are terminal-relative neutral theme tokens: derive them from a
reported RGB terminal background only when true-color support is known. The
`menu_item_surface` token starts from that derived menu surface and lowers each
RGB channel by a small fixed amount, keeping enabled items slightly darker on
both dark and light terminals. Leave all of these backgrounds unset otherwise,
and never replace them with indexed grays. A selected item fills its complete
rendered height and padding with the selected neutral surface. Symmetric rails
are a caller-level full-screen overlay treatment, not the default menu-row
treatment: a caller composing this menu in its application's full-screen
overlay layer may opt into matching one-cell accent rails on both edges. The
shared widget never infers full-screen context from its `Rect`. This exception
never applies to embedded menus or copyable content. Unselected enabled rows
use `menu_item_surface`; disabled rows remain faded on `menu_surface`. Menu rows
are not zebra striped.

There is intentionally no public `SelectableListState` abstraction. The menu
and picker both expose caller-held state and typed outcomes, but their
navigation, filtering, layout, and activation semantics are not yet a proven
shared consumer boundary. Handroll informed the overlay's interaction and
information design; adopting it there remains separate consumer work.

## Details and copy

21. Render metadata as a two-column definition list with left-aligned labels
    and values, a two-cell gap between them, and a ragged-right edge. Do not use
    trailing colons or structural separator glyphs.
22. Group related metadata with one blank row, not another box or a rule glyph.
23. Never use em dashes in user-visible TUI copy. Use a hyphen, middot, blank
    value, or explicit phrase such as `Not reported`.
24. Use sentence case and concise action labels.

## Detail panes

`DetailPane` is a presentation-only definition-list widget. Callers provide
its rectangle, choose side or bottom placement, and retain scrolling, focus,
key handling, loading, routing, and application actions. It intentionally does
not make breakpoints or overlay decisions. Handroll adoption is deferred to a
separate consumer migration.

The component shades one continuous `detail_surface` pane, inset one cell from
the caller-owned rectangle on both horizontal edges. Content has one additional
cell of horizontal padding inside that surface. When the caller supplies the
optional heading, the component renders it at the same left edge as the two
columns and leaves one row of space before the definition list. The caller
still owns the surrounding page and placement.

Compact density, two-column layout, and a plain pane surface are the defaults.
Normal density inserts one blank row only between adjacent key/value entries;
it does not add space beside an explicit `Rule`. Optional zebra styling fills
the complete surface width of every physical line belonging to a logical
entry, alternates `row` and `row_alt`, and restarts after a `Rule`.

Stacked layout places each label above its value and insets the value by two
cells. Responsive layout selects that stacked form below the caller-provided
outer-width threshold and uses columns at or above it. `required_height(width)`
uses the same layout resolution and wrapping measurement as rendering so
callers can reserve an exact-height region before placing the pane.

## Verification

25. Snapshot every component at representative wide and narrow widths.
26. Snapshot both densities and loading, empty, error, disabled, and selected
    states where they apply.
27. Pair text snapshots with direct buffer assertions for backgrounds,
    foregrounds, and modifiers. Text snapshots do not preserve Ratatui styles.

## Canonical reference

Run the single full-screen design reference from `nori-rs/`:

```console
cargo run -p nori-tui-components --example nori_storybook
```

The Picker, Markdown, Primitives, States, Detail pane, and Overlay menu pages
are the visual acceptance target for this crate. Detail pane is page `5`; use
`Tab`/`Shift-Tab` there to compare compact columns, zebra bands, normal density,
responsive stacking, fixed label width, and an omitted heading. Page `6`,
Overlay menu, is interactive. Use arrows or `j`/`k`
to move, `Enter` to activate, `Tab`/`Shift-Tab` to change the menu case, and the
displayed number or character shortcuts to invoke actions. The example owns its
terminal and event loop, adapts raw keys to domain-free actions, and uses
production components only. While Picker search is active, printable keys
belong exclusively to the query: global page, quit, density, mode, and state
shortcuts resume only after search deactivates, and Escape deactivates search
before leaving the example.
