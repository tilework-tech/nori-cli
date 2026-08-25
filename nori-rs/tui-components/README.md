# Nori TUI components

`nori-tui-components` is the public, domain-free component library used by
Nori terminal applications. Components render inside caller-owned rectangles
from caller-provided data or state; interactive state machines return typed
outcomes. Applications remain responsible for terminal setup, event
collection, update loops, and command dispatch.

The v0 surface contains:

- a centered selectable overlay menu for bounded action sets, with stable
  typed keys, validated explicit shortcuts, disabled and consequence states,
  responsive two-row items, and typed interaction outcomes;
- a configurable searchable picker with responsive columns, tabs, details,
  single/toggle/multi modes, and explicit loading/empty/error states;
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
picker density, and `Tab`/`Shift-Tab` to compare overlay-menu states. Picker
search owns every printable key while active, so page, quit, density, mode, and
state shortcuts resume after search deactivates. `/`, `f`, and Ctrl-F all
activate picker search; the visible subtitle and footer mention only `/` to
keep the hint concise. While the overlay page is active, use arrows or `j`/`k`
to move, `Enter` to activate, and `1-5` or `r`/`s`/`i`/`a` to invoke displayed
shortcuts. Overlay shortcuts take precedence over numbered page navigation;
the left arrow returns to Detail pane and the right arrow wraps to Picker.
The focused examples remain useful while developing one component:

```console
cargo run -p nori-tui-components --example picker_storybook
cargo run -p nori-tui-components --example markdown_storybook
cargo run -p nori-tui-components --example component_storybook
```

Press `q` or `Esc` to leave an example. In picker examples, active search owns
printable keys and the first Escape, so deactivate search before quitting.
