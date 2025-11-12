//! Rendering abstractions and utilities for composable TUI layouts.
//!
//! This module provides the [`Renderable`] trait and related types that enable composable,
//! height-aware rendering in Ratatui applications. Unlike Ratatui's widget system where
//! widgets are consumed on render, Renderables can calculate their desired height before
//! rendering, enabling dynamic layouts.
//!
//! # Core Components
//!
//! ## The Renderable Trait
//!
//! The [`Renderable`] trait is the foundation of this module:
//!
//! ```rust
//! use ratatui::buffer::Buffer;
//! use ratatui::layout::Rect;
//!
//! pub trait Renderable {
//!     fn render(&self, area: Rect, buf: &mut Buffer);
//!     fn desired_height(&self, width: u16) -> u16;
//! }
//! ```
//!
//! ## Layout Containers
//!
//! - [`ColumnRenderable`]: Stacks children vertically
//! - [`RowRenderable`]: Places children horizontally with specified widths
//! - [`InsetRenderable`]: Adds padding around a child
//!
//! ## Utility Types
//!
//! - [`Insets`]: Represents padding on all four sides
//! - [`RectExt`]: Extension trait adding inset functionality to `Rect`
//!
//! # Examples
//!
//! ## Basic Composition
//!
//! ```rust
//! use codex_tui_components::render::{ColumnRenderable, Renderable};
//! use ratatui::buffer::Buffer;
//! use ratatui::layout::Rect;
//!
//! let mut column = ColumnRenderable::new();
//! column.push("Header");
//! column.push("Body content");
//! column.push("Footer");
//!
//! // Calculate total height needed
//! let height = column.desired_height(80);
//!
//! // Render to buffer
//! let mut buf = Buffer::empty(Rect::new(0, 0, 80, height));
//! column.render(Rect::new(0, 0, 80, height), &mut buf);
//! ```
//!
//! ## Using Insets
//!
//! ```rust
//! use codex_tui_components::render::{Insets, RenderableExt};
//! use ratatui::buffer::Buffer;
//! use ratatui::layout::Rect;
//!
//! let content = "Padded content".inset(Insets::vh(1, 2));
//! ```

use ratatui::layout::Rect;

#[cfg(feature = "syntax-highlighting")]
pub mod highlight;
pub mod line_utils;
pub mod renderable;

pub use renderable::{ColumnRenderable, InsetRenderable, Renderable, RenderableExt, RowRenderable};

/// Represents padding or spacing on all four sides of a rectangular area.
///
/// Insets are commonly used to add padding around content, create margins,
/// or define spacing between UI elements.
///
/// # Examples
///
/// ```rust
/// use codex_tui_components::render::Insets;
///
/// // Create insets with 1 unit of vertical padding and 2 units horizontal
/// let insets = Insets::vh(1, 2);
///
/// // Create insets with specific values for each side
/// let insets = Insets::tlbr(1, 2, 1, 2); // top, left, bottom, right
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Insets {
    left: u16,
    top: u16,
    right: u16,
    bottom: u16,
}

impl Insets {
    /// Creates insets with specific values for each side.
    ///
    /// # Arguments
    ///
    /// * `top` - Padding on the top edge
    /// * `left` - Padding on the left edge
    /// * `bottom` - Padding on the bottom edge
    /// * `right` - Padding on the right edge
    ///
    /// # Examples
    ///
    /// ```rust
    /// use codex_tui_components::render::Insets;
    ///
    /// let insets = Insets::tlbr(1, 2, 3, 4);
    /// ```
    pub fn tlbr(top: u16, left: u16, bottom: u16, right: u16) -> Self {
        Self {
            top,
            left,
            bottom,
            right,
        }
    }

    /// Creates insets with symmetric vertical and horizontal padding.
    ///
    /// # Arguments
    ///
    /// * `v` - Vertical padding (applied to top and bottom)
    /// * `h` - Horizontal padding (applied to left and right)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use codex_tui_components::render::Insets;
    ///
    /// // 1 unit vertical, 2 units horizontal
    /// let insets = Insets::vh(1, 2);
    /// ```
    pub fn vh(v: u16, h: u16) -> Self {
        Self {
            top: v,
            left: h,
            bottom: v,
            right: h,
        }
    }
}

/// Extension trait for [`Rect`] that adds inset functionality.
///
/// This trait allows you to easily create a smaller rectangle by applying
/// insets (padding) to an existing rectangle.
///
/// # Examples
///
/// ```rust
/// use codex_tui_components::render::{Insets, RectExt};
/// use ratatui::layout::Rect;
///
/// let outer = Rect::new(0, 0, 20, 10);
/// let inner = outer.inset(Insets::vh(1, 2));
/// // inner is now Rect { x: 2, y: 1, width: 16, height: 8 }
/// ```
pub trait RectExt {
    /// Returns a new rectangle that is inset by the specified amounts.
    ///
    /// The resulting rectangle will be smaller by the sum of the insets
    /// on each axis. All operations use saturating arithmetic to prevent
    /// overflow or underflow.
    fn inset(&self, insets: Insets) -> Rect;
}

impl RectExt for Rect {
    fn inset(&self, insets: Insets) -> Rect {
        let horizontal = insets.left.saturating_add(insets.right);
        let vertical = insets.top.saturating_add(insets.bottom);
        Rect {
            x: self.x.saturating_add(insets.left),
            y: self.y.saturating_add(insets.top),
            width: self.width.saturating_sub(horizontal),
            height: self.height.saturating_sub(vertical),
        }
    }
}
