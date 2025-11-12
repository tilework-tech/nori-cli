# codex-tui-components

Reusable TUI components built on Ratatui, extracted from the Codex project.

## Features

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
codex-tui-components = "0.1.0"
```

For syntax highlighting support:

```toml
[dependencies]
codex-tui-components = { version = "0.1.0", features = ["syntax-highlighting"] }
```

## Quick Start

```rust
use codex_tui_components::shimmer::Shimmer;
use ratatui::widgets::WidgetRef;

// Create an animated shimmer effect
let shimmer = Shimmer::new("Processing...");
// Render in your terminal loop with WidgetRef::render_ref()
```

## Components

### Animation & Visual Effects

#### Shimmer
Animated text effect with customizable color palettes for loading states. Creates a wave-like shimmer effect that sweeps across text.

#### KeyHint
Display keyboard shortcuts with platform-aware formatting (Cmd vs Ctrl, etc.). Provides helper functions for common key combinations.

### Layout & Rendering

#### Renderable Trait
Composable rendering abstraction that extends Ratatui's widget system. Unlike widgets which are consumed on render, Renderables can calculate their desired height before rendering, enabling dynamic layouts.

#### ColumnRenderable
Stack children vertically with automatic height calculation.

#### RowRenderable
Place children horizontally with specified widths.

#### InsetRenderable
Add padding around child components.

#### Line Utilities
Helper functions for manipulating Ratatui Line and Span types.

### Input & State Management

#### ScrollState
Generic scroll and selection state for vertical lists with wrap-around navigation and automatic scroll adjustment.

#### PasteBurst
Timing-based detection of paste operations vs typed input. Enables special handling for multi-line pastes and prevents flickering.

### Optional Features

#### Syntax Highlighting (feature: `syntax-highlighting`)
Bash syntax highlighting using tree-sitter. Converts bash scripts into styled Ratatui lines with appropriate dimming for comments, operators, and strings.

## Examples

Examples are coming soon. In the meantime, comprehensive usage examples are included in the rustdoc for each component.

## Documentation

Generate local documentation with:

```bash
cargo doc --all-features --open
```

All public APIs include comprehensive rustdoc with examples.

## License

Licensed under MIT OR Apache-2.0.
