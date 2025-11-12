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
pub mod paste_burst;
pub mod scroll_state;

// TODO: Extract these in future iterations
// pub mod wrapping;
// pub mod textarea;
// pub mod selection_list;

// Re-export commonly used types for convenience
pub use key_hint::KeyBinding;
pub use render::{ColumnRenderable, InsetRenderable, Renderable, RenderableExt, RowRenderable};
pub use shimmer::Shimmer;
