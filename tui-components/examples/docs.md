# Noridoc: TUI Components Tests

Path: @/tui-components/tests

### Overview

- Snapshot tests for all TUI components using the `insta` crate
- Interactive examples demonstrating component behavior, co-located with tests
- Visual regression testing with text-based snapshots

### How it fits into the larger codebase

- Tests live in dedicated `tests/` directory, separate from `src/` implementation code
- Each `*_snapshots.rs` file contains both snapshot tests AND a runnable interactive example
- Interactive examples are registered in `@/tui-components/Cargo.toml` as `[[example]]` entries
- Snapshots are stored in `tests/snapshots/` subdirectory, managed by `insta` crate
- Examples use the same ratatui/crossterm APIs as production code, serving as integration tests
- Components being tested are imported from `@/tui-components/src/`

### Core Implementation

**Dual-Purpose Test Files**
- Each `*_snapshots.rs` file has two distinct sections separated by `#[cfg(test)]` guards:
  - Interactive example code (no guard, always compiled)
  - Test code (behind `#[cfg(test)]`, only compiled during testing)
- Examples include: `textarea`, `key_hint`, `shimmer`, `selection`

**Interactive Example Pattern**
- Entry point: `main()` function follows standard pattern:
  1. `color_eyre::install()?` - Error handling setup
  2. `ratatui::init()` - Terminal initialization
  3. `app.run(&mut terminal)` - Event loop
  4. `ratatui::restore()` - Terminal cleanup
- `App` struct: Holds multiple widget configurations to demonstrate different features side-by-side
- `App::new()`: Constructs example configurations (basic, styled, with options, etc.)
- `App::run()`: Event loop handling keyboard input (Esc/Ctrl+C to exit, distributes input to widgets)
- `App::draw()`: Renders all widget configurations with labels using ratatui Layout system
- All widgets receive input simultaneously - no focus management needed for examples

**Snapshot Testing Pattern**
- Helper functions like `render_to_string()` or `render_list_to_string()` convert widgets to text
- Each test renders a widget configuration and compares against saved snapshot
- Snapshots capture exact terminal output including Unicode, colors, borders
- Uses `assert_snapshot!()` macro from `insta` crate

**Selection Component Example** (`selection_snapshots.rs`)
- `ExampleData`: Simple struct demonstrating generic data type for `SelectionList<T>`
- Four `SelectionList` configurations demonstrated:
  1. Basic selection with title and footer
  2. Selection with search enabled
  3. Selection with subtitle
  4. Long list (12 items) demonstrating scrolling behavior (MAX_POPUP_ROWS=8)
- Status line shows last selection event from any list
- Snapshot tests cover: basic render, with search, with subtitle, long list scrolling

**TextArea Example** (`textarea_snapshots.rs`)
- Four TextArea configurations: default with placeholder, custom styling, pre-filled content, narrow width for wrapping
- All textareas receive same input to compare behaviors

**Shimmer Example** (`shimmer_snapshots.rs`)
- Demonstrates animation patterns: fixed 30 FPS vs on-keypress animation
- Multiple color palettes: basic, blue, green
- Unicode text handling

**KeyHint Example** (`key_hint_snapshots.rs`)
- Demonstrates platform-aware keyboard shortcut display
- Shows plain keys, control keys, alt keys, shift keys, arrow keys

### Things to Know

**Co-location Strategy**
- Tests and examples are intentionally co-located in same file
- Avoids duplication of setup code and widget configuration logic
- Examples serve as both documentation AND integration tests
- All examples are runnable via `cargo run --example <name>`

**No Focus Management in Examples**
- Interactive examples distribute keyboard input to ALL widgets simultaneously
- Simplifies example code - no need for focus tracking state
- Works well for demonstration purposes where seeing all variants respond is useful
- Production usage would typically have focus management

**Snapshot File Organization**
- Snapshots stored in `tests/snapshots/` directory
- Named pattern: `<test_module_name>__<test_function_name>.snap`
- `insta` crate handles snapshot creation/comparison automatically
- Review snapshots with `cargo insta review` after changes

**Testing Anti-Pattern Avoided**
- Tests verify actual widget rendering output, not mocked behavior
- Each test exercises real component code path through `render()` methods
- Snapshot approach catches visual regressions that unit tests might miss

**MAX_POPUP_ROWS Constant**
- Selection component limits visible items to 8 rows (MAX_POPUP_ROWS)
- Long list example (12 items) demonstrates scrolling behavior
- Scrolling automatically tracks current selection

**Event Handling**
- Examples use crossterm's `Event::Key` for keyboard input
- Standard exit pattern: `Esc` or `Ctrl+C` to quit
- Components return event enums (e.g., `SelectionListEvent::Selected`, `SelectionListEvent::Cancelled`)
- Selection example tracks and displays last event in status line

Created and maintained by Nori.
