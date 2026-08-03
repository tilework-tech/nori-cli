# Nori TUI components

`codex-tui-components` is the public, domain-free component library used by
Nori terminal applications. Components render from caller-owned state and
return typed outcomes; applications remain responsible for terminal setup,
event collection, update loops, and command dispatch.

The v0 surface contains:

- a configurable searchable picker with responsive columns, tabs, details,
  single/toggle/multi modes, and explicit loading/empty/error states;
- width-aware Markdown rendering with adaptive table layouts;
- semantic themes, messages, empty states, and key hints;
- three fullscreen examples under `examples/` that serve as an interactive
  TUI storybook.

Run the storybook examples from `nori-rs/`:

```console
cargo run -p codex-tui-components --example picker_storybook
cargo run -p codex-tui-components --example markdown_storybook
cargo run -p codex-tui-components --example component_storybook
```

Press `q` or `Esc` to leave an example.
