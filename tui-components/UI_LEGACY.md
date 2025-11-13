# UI Legacy Components

This document outlines the legacy UI components extracted from `src/ui.rs` that could be encapsulated into reusable `tui-components` library components.

## Overview

The current `src/ui.rs` contains several distinct rendering functionalities that are tightly coupled to the application model.

## Component List

### Core Chat Components

#### 1. InputArea
**Purpose**: Dynamic textarea input with borders and height calculation.

**Features**:
- Renders `TextArea` with calculated dynamic height based on content
- Includes border styling (`Borders::ALL`)
- Handles text wrapping and line counting
- Minimum height: 3 lines, Maximum height: 10 lines
- Adds 2 lines for border height

**Dependencies**: `tui_textarea::TextArea`, `unicode_width::UnicodeWidthStr`

**Current Location**: `src/ui.rs:calculate_textarea_height()` and `render_chat()` input section

#### 2. AgentInfo
**Purpose**: Display selected agent information with debug indicators.

**Features**:
- Shows selected agent name in cyan color
- Includes "[DEBUG]" indicator when debug mode is active
- Falls back to "No agent selected" when no agent is chosen
- Single line display

**Current Location**: `src/ui.rs:render_chat()` agent info section (lines 89-104)

#### 3. InstructionsBar
**Purpose**: Display status messages, errors, or user instructions.

**Features**:
- Shows error messages in yellow when present
- Displays default instructions in gray: "/switch-model: agents | /exit: quit"
- Single line with conditional styling
- Handles dynamic message content

**Current Location**: `src/ui.rs:render_chat()` instructions section (lines 126-140)

#### 4. AutocompleteDropdown
**Purpose**: Command suggestions dropdown with selection highlighting.

**Features**:
- Renders filtered command list with borders
- Shows commands prefixed with "/"
- Highlighted selection with ">" symbol
- "Commands" title in border
- Dynamic height based on command count (clamped 1-6)
- Handles empty state (no rendering)

**Current Location**: `src/ui.rs:render_autocomplete_in_layout()`

### Modal/Fullscreen Components

#### 5. AgentSelectionModal
**Purpose**: Fullscreen agent router interface.

**Layout**:
- Title: "Agent Router - Select an Agent" (cyan, bold)
- Agent list with availability indicators
- Navigation instructions: "↑/↓: navigate | Enter: select | Esc: close"

**Features**:
- Shows agent availability ("[Not Installed]" for unavailable agents)
- Gray styling for unavailable agents
- Highlighted selection with ">> " symbol and dark gray background
- Uses `ListState` for selection management

**Current Location**: `src/ui.rs:render_agent_selection_fullscreen()`

#### 6. InstallPromptModal
**Purpose**: Fullscreen backend installation prompt.

**Layout**:
- Title: "Backend Not Installed" (yellow, bold)
- Dynamic message based on install command availability
- Options list with selection highlighting
- Navigation instructions: "↑/↓: navigate | Enter: confirm | Esc: cancel"

**Features**:
- Conditional options: shows "Run Installation" only when install command exists
- Always shows "Open Installation Page" and "Cancel"
- Selection highlighting with ">> " prefix and dark gray background
- Word-wrapped message text

**Current Location**: `src/ui.rs:render_install_prompt_fullscreen()`

### Utility Components

#### 7. LoadingIndicator
**Purpose**: Unified loading animation component.

**Features**:
- Uses `Shimmer` component when `use_codex_components` is true
- Falls back to legacy spinner animation (10-frame character cycle)
- Shows "{agent} processing..." text
- Single line display

**Current Location**: `src/ui.rs:render_chat()` loading section (lines 106-124)

### Layout Components

#### 8. ChatLayout
**Purpose**: Main chat interface layout orchestrator.

**Features**:
- Vertical layout with dynamic constraints
- Handles autocomplete visibility (adjusts layout when visible)
- Stacks: InputArea, AgentInfo/LoadingIndicator, InstructionsBar
- Calculates textarea height dynamically
- Manages layout chunks and rendering order

**Current Location**: `src/ui.rs:render_chat()` layout logic (lines 51-148)

## Implementation Notes

### Renderable Trait Compliance
All components could implement the `tui_components::render::Renderable` trait:
- `render(&self, area: Rect, buf: &mut Buffer)`
- `desired_height(&self, width: u16) -> u16`

### State Management
Components requiring application state could accept it via constructor parameters or associated data structures, not direct model access.

### Styling Consistency
Maintain existing color schemes:
- Cyan: Agent info, modal titles
- Yellow: Error messages, install prompt title
- Gray: Instructions, unavailable items
- Dark Gray: Selection backgrounds

### Testing
Each component should include:
- Unit tests for rendering logic
- Snapshot tests for visual output
- Integration tests for layout behavior

## Migration Strategy

1. Create new component modules in `tui-components/src/`
2. Implement `Renderable` trait for each component
3. Update `src/ui.rs` to use new components
4. Remove legacy rendering functions
5. Update tests and documentation

## Dependencies to Consider

- `ratatui` widgets (Paragraph, List, Block, etc.)
- `tui_textarea` for InputArea
- `unicode_width` for text width calculations
- Existing `tui_components` infrastructure (Shimmer, Renderable trait)
