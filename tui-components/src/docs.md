# Noridoc: tui-components/src

Path: @/tui-components/src

### Overview

Core widget implementations for the tui-components library: TextArea for multiline text input, Shimmer for loading animations, Footer for keyboard shortcuts, LiveWrap for streaming text wrapping, and supporting utilities for scrolling, paste detection, and rendering.

### How it fits into the larger codebase

- Used by @/src/ui.rs and @/src/app.rs for terminal UI rendering in nori-cli
- TextArea widget instantiated in @/src/app.rs via `create_textarea()` factory function
- Shimmer widget rendered in @/src/ui.rs during streaming operations
- Provides `WidgetRef` and `StatefulWidgetRef` implementations compatible with ratatui's rendering pipeline
- Wrapping utilities (@/tui-components/src/wrapping.rs) consumed by TextArea for text layout calculations

### Core Implementation

**TextArea Widget** (@/tui-components/src/textarea.rs):
- Entry point: `TextArea::new(TextAreaConfig)` creates widget instance with styling configuration
- Configuration via builder pattern: `.with_background_style()`, `.with_padding()`, `.with_prefix()`, `.with_border_style()`, `.with_placeholder()`
- Text editing API: `.insert_str()`, `.set_text()`, `.text()`, `.set_cursor()`, `.is_empty()`
- Keyboard handling: `.handle_key(KeyEvent)` processes input events for cursor movement and editing
- Rendering pipeline: `StatefulWidgetRef::render_ref()` → fill background → render border → calculate inner area → render prefix → render text → render cursor
- **Styling layers** (applied in order):
  1. Background fill: Fills entire allocated area with `background_style` color
  2. Border: Optional ratatui Block widget with `border_style` (reduces content area)
  3. Prefix symbol: Rendered at vertical center on left edge, consumes `prefix.len()` columns
  4. Padding: Creates inner margin (top/bottom in rows, left/right in columns)
  5. Text content: Rendered within inner area after accounting for all layout constraints
  6. Cursor: Positioned relative to inner area, adjusted for padding and prefix offset
- **Layout calculation invariant**: Text wrapping width = `available_width - prefix_width - padding_left - padding_right`
- **Height calculation**: Exposed via `.config()` getter for external height calculation (used by @/src/ui.rs)

**Shimmer Widget** (@/tui-components/src/shimmer.rs):
- Time-based gradient animation for loading states
- Created via `Shimmer::new(text)` or `Shimmer::with_palette(text, ColorPalette)`
- Renders using `WidgetRef::render_ref()` with automatic time tracking via `Instant::now()`
- Used by @/src/ui.rs when `use_codex_components = true`

**Footer Component** (@/tui-components/src/footer.rs):
- Configurable footer with keyboard shortcuts and context display
- Builder API for adding hints, sections, and context information

**LiveWrap** (@/tui-components/src/live_wrap.rs):
- Incremental text wrapping for streaming content
- Maintains wrap state across partial text updates

**Supporting Modules**:
- @/tui-components/src/wrapping.rs: Word wrapping with `wrap_ranges_trim()` function
- @/tui-components/src/scroll_state.rs: Scroll state utilities
- @/tui-components/src/paste_burst.rs: Timing-based paste detection
- @/tui-components/src/render/: Composable rendering utilities with Column, Row, Inset layouts

### Things to Know

**TextArea Rendering Sequence** (@/tui-components/src/textarea.rs:468-564):
- Background fill happens FIRST across entire area before any other rendering
- Border rendering reduces content_area for all subsequent calculations
- Prefix width is calculated once and affects both inner_area and cursor_area calculations
- Inner area calculation: `content_area - (prefix_width + padding_left + padding_right, padding_top + padding_bottom)`
- Prefix rendering uses vertical centering: `prefix_y = content_area.y + (content_area.height / 2)`
- Cursor positioning uses separate cursor_area rect that accounts for padding and prefix offset
- Text wrapping uses inner_area.width to ensure wrapped lines fit within padded region

**Styling Configuration Default Values** (@/tui-components/src/textarea.rs:67-83):
- All new styling fields default to no-op values for backward compatibility
- `background_style: Style::default()` (transparent, no background color)
- `padding_top/bottom/left/right: 0` (no padding)
- `prefix: None` (no prefix symbol)
- `border_style: None` (no border)
- Only pre-existing defaults: cursor style (white bg, black fg), placeholder style (dark gray fg)

**TextArea Configuration Access Pattern**:
- Configuration is immutable after widget creation (no runtime style changes)
- External code accesses config via `.config()` getter for layout calculations
- Used by @/src/ui.rs `calculate_textarea_height()` to account for padding/prefix when computing wrap width
- Ensures external layout calculations match internal rendering constraints

**Wrap Cache Invalidation** (@/tui-components/src/textarea.rs:347-360):
- `wrap_cache: RefCell<Option<WrapCache>>` stores cached wrapped line ranges keyed by width
- Cache invalidated on text changes: `.insert_str()`, `.replace_range()`, `.set_text()`
- Cache hit when `cached.width == width`, avoiding expensive re-wrapping
- Wrapping uses `crate::wrapping::wrap_ranges_trim()` from @/tui-components/src/wrapping.rs

**Border vs Padding Interaction**:
- Border renders first using ratatui's Block widget, which returns inner area
- Padding applied AFTER border, within the inner area from Block
- If both border and padding are set, total space consumed = border (2 rows/cols) + padding values
- This matches expected behavior where padding creates margin inside border

**Prefix Symbol Rendering**:
- Prefix is single-width character or string (commonly "›", ">", "•", "▸")
- Always rendered at vertical center regardless of text content or scroll position
- Consumes horizontal space on left side (text area width reduced by `prefix.len()`)
- Styled independently via `prefix_style` (allows color/modifier different from text)

**Height Calculation Pattern** (@/src/ui.rs:11-46):
- External code must account for padding and prefix when calculating textarea height
- Formula: `content_width = available_width - prefix_width - padding_left - padding_right`
- This content_width used to determine how many wrapped lines text will occupy
- Final height: `wrapped_lines + padding_top + padding_bottom`
- Critical for dynamic layout in @/src/ui.rs where textarea height changes based on content

**Cursor Positioning with Padding** (@/tui-components/src/textarea.rs:544-563):
- Cursor area calculation separate from inner area to ensure proper bounds checking
- Cursor positioned within inner area but final coordinates checked against outer area bounds
- Prevents cursor from rendering outside allocated widget area when near boundaries
- Cursor style applied directly to buffer cell at calculated position

Created and maintained by Nori.
