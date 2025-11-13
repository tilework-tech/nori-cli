//! # tui-components
//!
//! A collection of reusable TUI components built on top of Ratatui.
//!
//! This library provides high-quality, tested components extracted from the Codex project,
//! designed to be used in any Ratatui-based terminal application.
//!
//! ## Core Concepts
//!
//! ### Renderable Trait
//!
//! The [`Renderable`] trait provides a composable rendering abstraction that extends
//! Ratatui's widget system. Unlike widgets which are consumed on render, Renderables
//! can calculate their desired height before rendering, enabling dynamic layouts.
//!
//! ### Component Organization
//!
//! Components are organized into modules by functionality:
//! - **Animation & Visual Effects**: [`shimmer`], [`key_hint`]
//! - **Layout & Rendering**: [`render`]
//! - **Text Wrapping**: [`wrapping`], [`live_wrap`]
//! - **Input Widgets**: [`textarea`]
//! - **Input & State Management**: [`scroll_state`], [`paste_burst`]
//!
//! ## Examples
//!
//! ```rust,no_run
//! use tui_components::shimmer::Shimmer;
//! use ratatui::widgets::WidgetRef;
//!
//! // Create an animated shimmer effect
//! let shimmer = Shimmer::new("Loading...");
//! // Render with WidgetRef::render_ref()
//! ```
//!
//! See the `examples/` directory for complete, runnable demonstrations of each component.

#![warn(missing_docs)]

// Core rendering abstractions
pub mod render;

// Animation and visual effects
pub mod key_hint;
pub mod shimmer;

// Text handling and utilities
pub mod live_wrap;
pub mod paste_burst;
pub mod scroll_state;
pub mod textarea;
pub mod wrapping;

// Selection and popup components
pub mod selection;

// Re-export commonly used types for convenience
pub use key_hint::KeyBinding;
pub use live_wrap::{Row, RowBuilder, take_prefix_by_width};
pub use render::{ColumnRenderable, InsetRenderable, Renderable, RenderableExt, RowRenderable};
pub use selection::{
    GenericDisplayRow, MAX_POPUP_ROWS, SelectionItem, SelectionList, SelectionListConfig,
    SelectionListEvent, selection_option_row, standard_popup_hint_line,
};
pub use shimmer::Shimmer;
pub use textarea::{TextArea, TextAreaConfig, TextAreaState};
pub use wrapping::{
    RtOptions, prefix_lines, word_wrap_line, word_wrap_lines, word_wrap_lines_borrowed,
};
