# Noridoc: tui-components

Path: @/nori-rs/tui-components

### Overview

- `codex-tui-components` provides reusable, domain-free Ratatui presentation
  and interaction state for Nori terminal applications.
- Components accept caller-owned rectangles and caller-provided data or state.
  Interactive state machines return typed outcomes; presentation-only widgets
  render without taking over application behavior.
- [`DESIGN.md`](DESIGN.md) is the visual contract, while the production
  [`nori_storybook`](examples/nori_storybook.rs) is the interactive acceptance
  reference. Detail pane is page `5`, followed by the interactive Overlay menu
  on page `6`.

### How it fits into the larger codebase

```text
consumer application
  terminal + raw events + app routing + focus/modal orchestration
                         |
                         | domain-free action, state, and Rect
                         v
                codex-tui-components
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
- [`DetailPane`](src/detail.rs) is a stateless definition-list renderer for
  caller-positioned side or bottom regions. Its caller retains placement,
  scrolling, focus, loading, key handling, and application routing.
- [`theme`](src/theme/) supplies semantic styles shared across components.
  Neutral backgrounds are derived from a reported terminal RGB background
  only when the consumer knows true color is supported; otherwise those
  backgrounds remain unset.
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
- `OverlayMenu` centers a content-derived, maximum-width surface inside the
  supplied rectangle. Rendering reconciles only menu-local viewport offset and
  capacity so the selection stays visible when content exceeds the available
  height.
- `DetailPane` accepts key/value entries and structural rules, trims trailing
  colons from labels, and maps semantic or provider tones through the shared
  theme. Its auto or fixed label gutter stays within the caller's width;
  callers explicitly choose whether each value wraps or truncates.
- The overlay renderer preserves titles and primary labels first, suppresses
  optional subtitles on constrained rectangles, wraps descriptions by Unicode
  display width, and emits overflow markers when not all items fit. Key hints
  occupy the bottom of the remaining surface and clamp below the title so tiny
  or non-zero-origin caller rectangles remain bounded.
- Selected items retain the normal primary accent across semantic tones, fill
  the complete row surface, and use symmetric thin edge rails. Warning and
  destructive colors identify consequences only while an item is not selected.

### Things to Know

- The consumer owns terminal initialization and restoration, input polling and
  raw-key mapping, render cadence, application actions and routing, focus and
  modal stacks, confirmation policy, asynchronous loading, and persistence.
- A shortcut invokes its matching enabled item immediately. When an item has
  both shortcut families, either maps to the same stable key; precedence among
  raw input meanings belongs to the consumer adapter.
- Disabled items remain visible but cannot receive selection or activation.
  Empty and all-disabled menus are valid and have no selected item.
- Surface height is content-derived and viewport-limited. Descriptions use at
  most two rows, blank spacing separates items, and selected content remains
  visible as the viewport changes.
- `backdrop` styles only the caller-provided rectangle. Without a derived RGB
  theme, neither the backdrop nor menu surface substitutes an absolute neutral
  color.
- No public selectable-list core is extracted: menu and picker semantics do
  not yet have another proven shared consumer beyond their existing
  state-in/typed-outcome-out convention.

Created and maintained by Nori.
