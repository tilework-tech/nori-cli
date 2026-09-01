# Nori TUI components

`nori-tui-components` is the public, domain-free component library used by
Nori terminal applications. Components render inside caller-owned rectangles
from caller-provided data or state; interactive state machines return typed
outcomes. Applications remain responsible for terminal setup, event
collection, update loops, and command dispatch.

The v0 surface contains:

- a centered selectable overlay menu for bounded action sets, with stable
  typed keys, validated explicit shortcuts, disabled and consequence states,
  responsive label-and-description items, Normal or Dense spacing, optional
  zebra surfaces, and typed interaction outcomes;
- a configurable searchable picker with responsive columns, tabs, details,
  single/toggle/multi modes, opt-in full-screen focus rails, and explicit
  loading/empty/error states;
- a stateless detail pane for responsive, semantically styled definition lists
  in caller-positioned side or bottom regions;
- width-aware Markdown rendering with adaptive table layouts;
- semantic themes, messages, empty states, and key hints;
- one canonical fullscreen design storybook plus focused component examples.

The enforceable visual contract lives in [`DESIGN.md`](DESIGN.md).

Run the storybook examples from `nori-rs/`:

```console
cargo run -p nori-tui-components --example nori_storybook
```

The canonical storybook contains Picker, Markdown, Primitives, States, Detail
pane, and the production Overlay menu. Use `1-6` to change page, `d` to switch
picker density, and `Tab`/`Shift-Tab` to compare configurable Detail-pane and
overlay-menu states. Picker
search owns every printable key while active, so page, quit, density, mode, and
state shortcuts resume after search deactivates. `/`, `f`, and Ctrl-F all
activate picker search; the visible subtitle and footer mention only `/` to
keep the hint concise. While the overlay page is active, use arrows or `j`/`k`
to move, `Enter` to activate, and `1-5` or `r`/`s`/`i`/`a` to invoke displayed
shortcuts. Overlay shortcuts take precedence over numbered page navigation;
the left arrow returns to Detail pane and the right arrow wraps to Picker. On
full-screen overlay examples, selected rows use the explicit symmetric-rail
treatment; the Narrow example demonstrates the copy-safe compact pointer used
by default. The Picker page also opts into full-screen rails: they replace the
single-select pointer, while toggle and multi-select markers continue to show
checked state. The overlay cases include Dense and Dense Zebra presentations.
Shared pickers and overlay menus are now deployed across Nori CLI bottom-pane
and full-screen surfaces; the storybook remains their visual acceptance
reference. On the Detail pane page, `Tab`/`Shift-Tab` compares compact columns,
zebra bands, normal density, responsive stacking, fixed label width, and an
omitted heading. Each story reports its required height at the current pane
width.
The focused examples remain useful while developing one component:

```console
cargo run -p nori-tui-components --example status_card_storybook
cargo run -p nori-tui-components --example picker_storybook
cargo run -p nori-tui-components --example markdown_storybook
cargo run -p nori-tui-components --example component_storybook
```

`status_card_storybook` is an interactive production-design specimen. Use it
to compare derived, ANSI-fallback, and unshaded surfaces; plain or colon labels;
green accent placement; compact or normal density; and summary or full status
content before changing the CLI status card.

Press `q` or `Esc` to leave an example. In picker examples, active search owns
printable keys and the first Escape, so deactivate search before quitting.
