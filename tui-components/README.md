# tui-components

Reusable TUI components built on Ratatui.

## Features

- **Text Input** - Multiline TextArea widget with cursor management and wrapping
- **Selection Lists** - Generic selection widget with keyboard navigation and filtering
- **Text Wrapping** - Word wrapping utilities with Ratatui integration
- **Live Wrapping** - Incremental text wrapping for streaming content
- **Footer Component** - Configurable footer with keyboard shortcuts, hints, and context display
- **Animation Effects** - Shimmer animations for loading states
- **Keyboard Hints** - Platform-aware keyboard shortcut display
- **Composable Rendering** - Renderable trait with Column, Row, and Inset layouts
- **Scroll Management** - Scroll state utilities with wrap-around navigation
- **Paste Detection** - Timing-based paste detection to distinguish from typing
- **Text Utilities** - Line manipulation and text rendering helpers
- **Optional Syntax Highlighting** - Bash syntax highlighting with tree-sitter (feature-gated)

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
tui-components = "0.1.0"
```

For syntax highlighting support:

```toml
[dependencies]
tui-components = { version = "0.1.0", features = ["syntax-highlighting"] }
```

## Quick Start

```rust
use tui_components::shimmer::Shimmer;
use ratatui::widgets::WidgetRef;

// Create an animated shimmer effect
let shimmer = Shimmer::new("Processing...");
// Render in your terminal loop with WidgetRef::render_ref()
```

## Components

### Text Handling & Wrapping

#### Wrapping Module (`wrapping`)
Core text wrapping utilities with Ratatui integration:
- **`word_wrap_line`** - Wrap a single `Line` with style preservation
- **`word_wrap_lines`** - Wrap multiple lines with indent support
- **`word_wrap_lines_borrowed`** - Borrowed variant for zero-copy wrapping
- **`prefix_lines`** - Add prefixes to lines (indentation, bullets, etc.)
- **`RtOptions`** - Ratatui-specific wrapping configuration

Handles Unicode correctly, preserves ANSI styling across wraps, and supports custom break behavior.

#### Live Wrap Module (`live_wrap`)
Incremental text wrapping for streaming content:
- **`RowBuilder`** - Builds wrapped rows incrementally as text arrives
- **`Row`** - Single visual row with explicit break tracking
- **`take_prefix_by_width`** - Unicode-aware width-based text slicing

Designed for streaming scenarios where text arrives in chunks. Maintains fragmentation invariance (results don't depend on input chunking) and handles dynamic width changes with automatic rewrapping.

### Input Widgets

#### TextArea (`textarea`)
Full-featured multiline text input widget:
- Text insertion, deletion, and cursor navigation
- Emacs-style keybindings (arrows, Home/End, Backspace/Delete, Enter)
- Word wrapping with configurable width
- Scrolling support for content exceeding viewport
- Unicode-aware text handling
- Configurable placeholder text and styling via `TextAreaConfig`
- Implements `WidgetRef` and `StatefulWidgetRef` traits

Perfect for chat input, form fields, and any multiline text editing needs.

### Selection & Popups

#### Selection Module (`selection`)
Generic selection list components for building menus, dropdowns, and pickers:

- **`SelectionList<T>`** - Generic selection widget with:
  - Keyboard navigation (up/down with wrapping)
  - Optional search filtering
  - Number key shortcuts (when search disabled)
  - Event-based API via `SelectionListEvent` enum
  - Full `Renderable` trait implementation
  - Configuration via `SelectionListConfig`

- **`selection_option_row`** - Single row rendering with selection marker
- **`GenericDisplayRow`** - Common row structure for list items
- **`render_rows`** - Shared rendering with scrolling, wrapping, and alignment
- **`measure_rows_height`** - Dynamic height calculation
- **`standard_popup_hint_line`** - Standard footer hints for popups

Use cases: command palettes, file pickers, option menus, agent selection dialogs.

### Animation & Visual Effects

#### Shimmer (`shimmer`)
Animated text effect with customizable color palettes for loading states. Creates a wave-like shimmer effect that sweeps across text. Useful for "Working..." indicators and processing states.

#### KeyHint (`key_hint`)
Platform-aware keyboard shortcut display:
- Automatically uses Cmd on macOS, Ctrl on other platforms
- Helper functions for common key combinations
- `KeyBinding` type for structured key representation
- Proper formatting with modifiers (Ctrl+C, Alt+Enter, etc.)

### Footer Component

#### Footer
Configurable footer widget supporting multiple display modes: shortcut summaries, detailed shortcut overlays, custom messages, and context indicators. Features platform-aware keyboard hint rendering and dynamic height calculation.

### Layout & Rendering

#### Renderable Trait (`render`)
Composable rendering abstraction that extends Ratatui's widget system. Unlike widgets which are consumed on render, Renderables can calculate their desired height before rendering, enabling dynamic layouts.

- **`ColumnRenderable`** - Stack children vertically with automatic height calculation
- **`RowRenderable`** - Place children horizontally with specified widths
- **`InsetRenderable`** - Add padding around child components
- **Line Utilities** - Helper functions for manipulating Ratatui `Line` and `Span` types

### State Management

#### ScrollState (`scroll_state`)
Generic scroll and selection state for vertical lists:
- Wrap-around navigation (top to bottom, bottom to top)
- Automatic scroll adjustment to keep selection visible
- Viewport tracking
- Works seamlessly with `SelectionList` and custom list widgets

#### PasteBurst (`paste_burst`)
Timing-based detection of paste operations vs typed input:
- Distinguishes rapid input (paste) from typing
- Configurable threshold and burst window
- Enables special handling for multi-line pastes
- Prevents UI flickering during large pastes

### Optional Features

#### Syntax Highlighting (feature: `syntax-highlighting`)
Bash syntax highlighting using tree-sitter. Converts bash scripts into styled Ratatui lines with appropriate dimming for comments, operators, and strings. Enable with the `syntax-highlighting` feature flag.

## Examples

Interactive examples are available for several components. Each example demonstrates multiple configurations side-by-side:

### TextArea Example

```bash
cargo run --example textarea
```

Displays four TextArea configurations simultaneously:
- Default with placeholder text
- Custom styling with colored text
- Pre-filled multiline content
- Narrow width demonstrating text wrapping

### Selection Example

```bash
cargo run --example selection
```

Demonstrates the SelectionList widget with four configurations:
- Basic selection with title and footer
- Selection with search filtering enabled
- Selection with subtitle
- Long list (12 items) showing scrolling behavior

### KeyHint Example

```bash
cargo run --example key_hint
```

Shows platform-aware keyboard shortcut formatting for various key combinations.

### Shimmer Example

```bash
cargo run --example shimmer
```

Animated text effects with different color palettes and animation modes.

---

All examples receive keyboard input simultaneously for easy comparison. Press `Esc` or `Ctrl+C` to exit. For additional usage examples, see the rustdoc.

## Documentation

Generate local documentation with:

```bash
cargo doc --all-features --open
```

All public APIs include comprehensive rustdoc with examples.

## License

Licensed under Apache-2.0.
