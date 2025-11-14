# Remaining TUI Component Implementation

This document outlines the components yet to be extracted from `codex-rs/tui` into the `tui-components` shared library. All components listed here are **critical** for stabilizing the tui-components offering.

## Currently Completed Components

The following components have been successfully migrated (see `MIGRATION_PROGRESS.md` for details):

- ✅ **Text Wrapping Utilities** (`wrapping.rs`, `live_wrap.rs`)
- ✅ **TextArea Widget** (`textarea.rs`)
- ✅ **Selection & Popup Infrastructure** (`selection/` module)
- ✅ **Scroll State** (`scroll_state.rs`)
- ✅ **Paste Burst Detection** (`paste_burst.rs`)
- ✅ **Key Hint Utilities** (`key_hint.rs`)
- ✅ **Shimmer Animation** (`shimmer.rs`)
- ✅ **Rendering Abstractions** (`render/` module)

## Phase 5: Footer Component

### 5.1 Footer Component

**Source**: `codex-rs/tui/src/bottom_pane/footer.rs`
**Target**: `tui-components/src/footer.rs`

**Scope**:
- Extract generic footer rendering with configurable hints and modes
- Support multiple display modes (shortcuts, hints, custom messages)
- Platform-aware keyboard hint rendering
- Context indicator support (optional percentage display)
- Dynamic height calculation based on mode

**Key Functionality to Extract**:
- `FooterProps` struct (parameterize mode, hints, context percentage)
- `FooterMode` enum (ShortcutSummary, ShortcutOverlay, custom modes)
- `footer_height()` - Calculate required height
- `render_footer()` - Render footer with hints
- Mode toggle logic (make generic, remove Codex-specific state)

**Configuration Approach**:
- Use `FooterConfig` struct with:
  - Mode selection
  - Hint text builders (closures/functions)
  - Optional context window percentage
  - Style configuration
- Pass application-specific text/icons via config rather than hardcoding

**Codex-Specific Logic to Remove**:
- Remove hardcoded "? for shortcuts" text (make configurable)
- Remove Ctrl+C reminder logic (let consumer handle)
- Remove hardcoded FOOTER_INDENT_COLS (make configurable)
- Abstract away task running state checks

**Tests**:
- Snapshot tests for different footer modes
- Height calculation tests
- Multi-line footer rendering with wrapping
- Context percentage display formatting

### 5.2 ~~Filterable List / Command Popup~~ ✅ ALREADY COMPLETE

**Status**: The existing `SelectionList<T>` widget **already provides all typeahead filtering functionality**.

**What's Already Available** (in `tui-components/src/selection/list.rs`):
- ✅ Generic over item type `T` via `SelectionItem<T>`
- ✅ Filter string management (`search_query`, `set_search_query()`)
- ✅ Dynamic filtering with scroll state sync (`apply_filter()`)
- ✅ Keyboard input handling (typing updates filter automatically)
- ✅ Search placeholder support via config
- ✅ Empty state handling with custom message
- ✅ Selection preservation across filter changes
- ✅ Height calculation for filtered results
- ✅ Backspace to edit filter
- ✅ Optional search via `is_searchable` flag

**How to Use for Command Popup Use Cases**:
```rust
// Enable search in the config
let config = SelectionListConfig::new()
    .with_title("Commands")
    .with_search(Some("Type to filter...".to_string()));

// Provide items with search_value field
let items = commands.iter().map(|cmd| SelectionItem {
    data: cmd.clone(),
    name: format!("/{}", cmd.name),
    description: Some(cmd.description.clone()),
    search_value: Some(cmd.name.clone()), // Used for filtering
    is_current: false,
    display_shortcut: None,
    selected_description: None,
}).collect();

let mut list = SelectionList::new(config, items, Box::new(()));
```

**Current Implementation Details**:
- Filtering uses **substring matching** (case-insensitive) on `search_value` field
- Consumers can pre-process items to support fuzzy matching if needed
- Filter updates trigger automatic scroll state adjustment
- Selection preserved when possible during filter changes

**What's NOT Included** (intentionally Codex-specific):
- Fuzzy matching algorithm (consumers can implement externally)
- Match indices highlighting (visual feature, not core functionality)
- Composite item type handling (consumers use enum in data field)
- Slash-command-specific filter extraction logic

**Note**: The `command_popup.rs` in Codex uses this same foundation (`selection_popup_common.rs`) that was already extracted. The only additions in CommandPopup are:
1. Fuzzy matching (Codex-specific utility from `codex_common`)
2. Handling two item types (builtins + custom prompts)
3. Extracting filter from composer text (app-specific logic)

All three of these are **application-level concerns** that should stay in the consumer (Codex/nori-cli), not in the shared component library.

## Phase 6: Status Indicator

**Source**: `codex-rs/tui/src/status_indicator_widget.rs`
**Target**: `tui-components/src/status_indicator.rs`

**Scope**:
- Extract generic status line widget with timer and text
- Animated shimmer effect integration
- Elapsed time formatting
- Pause/resume timer support
- **Exclude**: Queue state, history management, operation state machine

**Key Functionality to Extract**:
- `StatusIndicator` widget with:
  - Animated header text (with shimmer)
  - Elapsed time display with pause/resume
  - Optional hint text (configurable)
  - Spinner animation integration
- `fmt_elapsed_compact()` - Format duration as compact string
- Timer management (pause/resume/reset)

**Configuration Approach**:
- Use `StatusIndicatorConfig` with:
  - Header text
  - Show/hide hint text
  - Optional custom hint builder
  - Style configuration
  - Spinner frame provider (reuse existing spinner)

**Codex-Specific Logic to Remove**:
- Remove `AppEventSender` dependency (use callbacks)
- Remove `FrameRequester` (let consumer handle frame requests)
- Remove interrupt handling (expose callback hook)
- Remove `Op::Interrupt` Codex protocol dependency
- Remove queue and history state tracking

**Simplified API**:
```rust
pub struct StatusIndicator {
    header: String,
    hint_text: Option<String>,
    elapsed: Duration,
    is_paused: bool,
    config: StatusIndicatorConfig,
}

impl StatusIndicator {
    pub fn new(header: impl Into<String>) -> Self;
    pub fn with_config(config: StatusIndicatorConfig) -> Self;
    pub fn set_header(&mut self, header: impl Into<String>);
    pub fn set_hint(&mut self, hint: Option<String>);
    pub fn elapsed(&self) -> Duration;
    pub fn pause(&mut self);
    pub fn resume(&mut self);
    pub fn reset(&mut self);
}

impl WidgetRef for StatusIndicator { ... }
```

**Tests**:
- Elapsed time formatting (0s, 59s, 1m 00s, 1h 00m 00s)
- Timer pause/resume/reset logic
- Header and hint text rendering
- Shimmer animation integration
- Dynamic width handling

## Implementation Order

1. **Footer Component** (3-4 hours)
   - Most straightforward extraction
   - Builds on existing `key_hint` and rendering utilities
   - No complex state management

2. **Status Indicator** (2-3 hours)
   - Clean separation from Codex-specific event handling
   - Reuses existing `shimmer` and timer utilities
   - Straightforward API design

## Exit Criteria

- Both remaining components (Footer, Status Indicator) have comprehensive rustdoc
- Snapshot tests for visual rendering (>5 tests per component)
- Unit tests for behavior and state management
- `cargo test -p tui-components` passes with all new tests
- `cargo clippy --all -- -D warnings` passes
- README.md updated with new component descriptions
- No Codex-specific dependencies remain

## Future Migration List

The following components are **not critical** for the initial tui-components stabilization but should be migrated in future phases:

### 7.1 Resume Picker
- **Source**: `codex-rs/tui/src/resume_picker.rs`
- **Complexity**: High (45KB file, complex state machine)
- **Dependencies**: File system, session management
- **Use Case**: Session selection and management UI

### 7.2 Onboarding Screen
- **Source**: `codex-rs/tui/src/onboarding/`
- **Complexity**: Medium
- **Dependencies**: Configuration, first-run detection
- **Use Case**: First-time user experience

### 7.3 Status Module (Full)
- **Source**: `codex-rs/tui/src/status/`
- **Complexity**: High (stateful operation tracking)
- **Dependencies**: Codex protocol, operation lifecycle
- **Use Case**: Full agent operation status with history and queue
- **Note**: Basic status indicator (Phase 6) covers immediate needs

### 7.4 History Cell
- **Source**: `codex-rs/tui/src/history_cell.rs`
- **Complexity**: Very High (79KB file, markdown rendering, interactions)
- **Dependencies**: Markdown rendering, syntax highlighting, collapsing
- **Use Case**: Conversation history display with rich formatting

### 7.5 Diff Renderer
- **Source**: `codex-rs/tui/src/diff_render.rs`
- **Complexity**: High (24KB file, syntax-aware diffing)
- **Dependencies**: Syntax highlighting, diff algorithms
- **Use Case**: Side-by-side or unified diff display with syntax

### 7.6 Pager Overlay
- **Source**: `codex-rs/tui/src/pager_overlay.rs`
- **Complexity**: High (30KB file, scrollable content viewer)
- **Dependencies**: Scrolling, syntax highlighting, search
- **Use Case**: Full-screen content viewer with navigation

### 7.7 Markdown Rendering
- **Source**: `codex-rs/tui/src/markdown_render.rs`, `markdown_stream.rs`
- **Complexity**: Very High (streaming, styling, code blocks)
- **Dependencies**: Markdown parsing, syntax highlighting
- **Use Case**: Rich text rendering for documentation and messages

### 7.8 Chat Composer (Full)
- **Source**: `codex-rs/tui/src/bottom_pane/chat_composer.rs`
- **Complexity**: Very High (131KB file, complex editing)
- **Dependencies**: TextArea, history, approval flow, file attachments
- **Use Case**: Full-featured message composition
- **Note**: Basic `TextArea` (already migrated) covers simple input needs

## Estimated Timeline

**Remaining Critical Work (Phase 5)**: 5-7 hours
- Footer: 3-4 hours
- Status Indicator: 2-3 hours
- ~~Filterable List: Already complete via SelectionList~~

**Future Work (Phase 7)**: 40-60 hours
- Resume Picker: 6-8 hours
- Onboarding: 3-4 hours
- Status Module: 8-10 hours
- History Cell: 12-16 hours
- Diff Renderer: 6-8 hours
- Pager Overlay: 5-7 hours
- Markdown Rendering: 8-12 hours
- Chat Composer: 10-15 hours (if needed beyond TextArea)

## Notes

- All work continues in the `migrate-tui-components` worktree
- Codex source files remain read-only
- Each component must pass independent tests before integration
- Prioritize API flexibility over Codex-specific convenience
- Document migration decisions in `MIGRATION_PROGRESS.md`
