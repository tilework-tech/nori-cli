# Noridoc: tui-components

Path: @/nori-rs/tui-components

### Overview

- `nori-tui-components` provides reusable, domain-free Ratatui presentation
  and interaction state for Nori terminal applications.
- Components accept caller-owned rectangles and caller-provided data or state.
  Interactive state machines return typed outcomes; presentation-only widgets
  render without taking over application behavior.
- [`DESIGN.md`](DESIGN.md) is the visual contract, while the production
  [`nori_storybook`](examples/nori_storybook.rs) is the interactive acceptance
  reference. Its Details page uses Tab and Shift-Tab to exercise auto and fixed
  label widths with a heading, plus the heading-omitted layout.

### How it fits into the larger codebase

```text
consumer application
  terminal + raw events + app routing + focus/modal orchestration
                         |
                         | domain-free action, state, and Rect
                         v
                nori-tui-components
             menu / picker / detail / primitives
                         |
                         | typed outcome, when interactive
                         v
                 consumer application
```

- Nori frontends may compose this crate's widgets without importing their
  application enums, callbacks, commands, persistence, or asynchronous work
  into the shared component boundary.
- The selectable [`menu`](src/menu/) serves small or bounded action sets. The
  [`picker`](src/picker/) remains the search and filtering surface for
  potentially large data sets; centered overlay placement does not turn one
  into the other.
- Picker consumers translate their terminal keymap into domain-free
  [`PickerAction`](src/picker/) values. The shared state machine owns whether
  filtering is active and returns typed outcomes; consumers retain dismissal,
  command dispatch, and application focus.
- [`DetailPane`](src/detail.rs) is a stateless definition-list renderer for
  caller-positioned side or bottom regions. It owns the inset pane surface and
  internal column layout, while its caller retains placement, scrolling, focus,
  loading, key handling, and application routing. The
  [`picker` renderer](src/picker/render.rs) adapts selected-item details into
  entries and supplies the `Details` heading.
- [`theme`](src/theme/) supplies semantic styles shared across components,
  including the detail-pane surface and agent identity tones selected through
  [`ProviderKind`](src/detail.rs). Neutral backgrounds, including the distinct
  overlay item layer, are derived from a reported terminal RGB background only
  when the consumer knows true color is supported; otherwise those backgrounds
  remain unset.
- Ratatui provides the rendering types and caller rectangles. Crossterm is an
  example-only development dependency, so the public library does not bind a
  consumer to one raw event source.
- Handroll is a design reference and a future consumer. This crate contains no
  Handroll workflow actions or data types, and adoption is deferred from the
  reusable component change.

### Core Implementation

- `MenuItem` pairs a stable caller key with label and optional description,
  explicit character or number shortcuts, availability/current metadata, and
  semantic consequence tone. `MenuState::try_new` validates the aggregate
  before selecting the first enabled item.
- Menu validation returns `MenuModelError` instead of constructing partial
  state when stable keys or shortcuts conflict. Character mnemonics are
  case-insensitive ASCII letters matching the label's first visible character;
  number shortcuts are single digits from `1` through `9`.
- `MenuState::handle` applies domain-free navigation, shortcut, activation,
  and cancellation actions. Navigation wraps and skips disabled items;
  activation and cancellation are returned as `MenuOutcome` values for the
  consumer to route.
- [`PickerState`](src/picker/) separates navigation from filtering with an
  explicit `search_active` state. Search-capable pickers begin inactive;
  `ActivateSearch` and `DeactivateSearch` produce `SearchModeChanged`, while
  query mutation actions are ignored outside active search. Deactivation
  clears the query and selects the first available visible item, and selecting
  `SearchMode::None` also clears both search state and query.
- The [`picker` renderer](src/picker/render.rs) shows its input row only during
  active search and derives footer hints from the same state. Its inactive
  footer deliberately advertises only the concise `/ search` convention, not
  every raw-key alias a consumer may map. This keeps the state machine and
  presentation synchronized while leaving the consumer free to choose which
  raw keys activate search.
- Picker consumers may attach explicit [`ProviderKind`](src/detail.rs) values
  to category names and individual cells. Category tabs retain their provider
  foreground when active and add bold emphasis; toned cells are used only for
  enabled, unselected rows so disabled and selected state styles remain the
  stronger interaction signal. The shared theme supplies this default identity
  mapping:

  | Agent identity | Terminal tone |
  | --- | --- |
  | Claude | Warm yellow/orange |
  | Codex | White |
  | Gemini and Antigravity | Blue |
  | Nori | Green |

  Pi retains the warning tone and unknown providers use normal text.
- `OverlayMenu` centers a content-derived, maximum-width surface inside the
  supplied rectangle. Rendering reconciles only menu-local viewport offset and
  capacity so the selection stays visible when content exceeds the available
  height.
- `DetailPane` accepts key/value entries and structural rules, trims trailing
  colons from labels, and maps semantic or provider tones through the shared
  theme. It styles the full pane surface one horizontal cell inside the caller's
  rectangle, then places content behind one more horizontal padding cell.
- Detail labels and values are both left-aligned with two blank cells between
  their columns. Auto and fixed label widths are calculated from the padded
  content width and bounded so a value column remains; callers explicitly
  choose whether each value wraps or truncates. Wrapped values reserve rows
  using the same Ratatui layout semantics that render them, so later entries
  begin after the rendered value.
- An optional heading occupies the first content row and leaves the next row
  blank before entries. Structural rules draw no glyph and instead reserve a
  blank grouping row.
- The overlay renderer preserves titles and primary labels first, suppresses
  optional subtitles on constrained rectangles, wraps descriptions by Unicode
  display width, and emits overflow markers when not all items fit. Key hints
  occupy the bottom of the remaining surface and clamp below the title so tiny
  or non-zero-origin caller rectangles remain bounded.
- Enabled, unselected menu items fill their complete item rectangles with the
  darker [`Theme::menu_item_surface`](src/theme/mod.rs) layer. Its terminal-aware
  default starts from `menu_surface` and lowers each RGB channel by a small
  fixed amount, so enabled items keep the same depth direction on dark and
  light terminal backgrounds. Selected items retain the normal primary accent
  across semantic tones and use symmetric thin edge rails; disabled items
  remain faded on `menu_surface` and stay unavailable to navigation or
  activation. Warning and destructive colors identify consequences only while
  an item is not selected.

### Things to Know

- The consumer owns terminal initialization and restoration, input polling and
  raw-key mapping, render cadence, application actions and routing, focus and
  modal stacks, confirmation policy, asynchronous loading, and persistence.
- Search state is independent of a consumer's editing mode. A consumer must
  explicitly map activation and deactivation keys to picker actions; printable
  query input has no effect until activation, so navigation shortcuts remain
  available while search is inactive. Nori's picker adapters support `/`, `f`,
  and Ctrl-F even though the visible inactive hint names only `/`.
- The storybook event adapters demonstrate the inverse routing invariant:
  while picker search is active, every printable character belongs to the
  query, so page navigation, quit, density, mode, and state shortcuts remain
  suspended until search deactivates.
- Provider tones are opt-in metadata rather than label inference. Untoned
  categories keep the existing accent-or-muted active treatment, and untoned
  cells keep the row's normal state style.
- A shortcut invokes its matching enabled item immediately. When an item has
  both shortcut families, either maps to the same stable key; precedence among
  raw input meanings belongs to the consumer adapter.
- Disabled items remain visible but cannot receive selection or activation.
  Empty and all-disabled menus are valid and have no selected item.
- Surface height is content-derived and viewport-limited. Descriptions use at
  most two rows, blank spacing separates items, and selected content remains
  visible as the viewport changes.
- `backdrop` styles only the caller-provided rectangle. Without a derived RGB
  theme, the backdrop, menu surface, and menu-item surface remain unset instead
  of substituting an absolute neutral color.
- Detail-pane surface styling is independent of heading presence and fills the
  pane's inset height, including blank heading and rule spacing. Only the outer
  horizontal caller gutters remain outside `Theme::detail_surface`.
- No public selectable-list core is extracted: menu and picker semantics do
  not yet have another proven shared consumer beyond their existing
  state-in/typed-outcome-out convention.

Created and maintained by Nori.
