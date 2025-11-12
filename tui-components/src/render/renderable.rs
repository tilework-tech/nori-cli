//! The core [`Renderable`] trait and composable layout containers.
//!
//! This module provides a trait-based abstraction for rendering TUI components
//! with height awareness, enabling dynamic layouts where components can calculate
//! their space requirements before rendering.

use std::sync::Arc;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::WidgetRef;

use crate::render::Insets;
use crate::render::RectExt as _;

/// A trait for types that can render themselves to a terminal buffer and report their desired height.
///
/// Unlike Ratatui's widget system where widgets are consumed on render, `Renderable` types
/// can be reused and queried for their height requirements before rendering. This enables
/// dynamic layouts where components can adapt to available space.
///
/// # Examples
///
/// ## Implementing Renderable
///
/// ```rust
/// use tui_components::render::Renderable;
/// use ratatui::buffer::Buffer;
/// use ratatui::layout::Rect;
/// use ratatui::widgets::{Paragraph, WidgetRef};
///
/// struct MyComponent {
///     lines: Vec<String>,
/// }
///
/// impl Renderable for MyComponent {
///     fn render(&self, area: Rect, buf: &mut Buffer) {
///         let text = self.lines.join("\n");
///         Paragraph::new(text).render_ref(area, buf);
///     }
///
///     fn desired_height(&self, _width: u16) -> u16 {
///         self.lines.len() as u16
///     }
/// }
/// ```
pub trait Renderable {
    /// Renders this component into the provided buffer area.
    ///
    /// # Arguments
    ///
    /// * `area` - The rectangular area to render into
    /// * `buf` - The buffer to render into
    fn render(&self, area: Rect, buf: &mut Buffer);

    /// Returns the desired height for this component given the available width.
    ///
    /// This allows parent layouts to allocate appropriate space before rendering.
    ///
    /// # Arguments
    ///
    /// * `width` - The available width in columns
    ///
    /// # Returns
    ///
    /// The desired height in rows
    fn desired_height(&self, width: u16) -> u16;
}

impl<R: Renderable + 'static> From<R> for Box<dyn Renderable> {
    fn from(value: R) -> Self {
        Box::new(value)
    }
}

impl Renderable for () {
    fn render(&self, _area: Rect, _buf: &mut Buffer) {}
    fn desired_height(&self, _width: u16) -> u16 {
        0
    }
}

impl Renderable for &str {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_ref(area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

impl Renderable for String {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_ref(area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

impl<'a> Renderable for Span<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_ref(area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

impl<'a> Renderable for Line<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        WidgetRef::render_ref(self, area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        1
    }
}

impl<'a> Renderable for Paragraph<'a> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_ref(area, buf);
    }
    fn desired_height(&self, _width: u16) -> u16 {
        // Note: Cannot accurately calculate wrapped line count without rendering
        // This is a limitation of the Paragraph API. For accurate height calculation,
        // use the text::Text type directly or implement custom wrapping logic.
        1
    }
}

impl<R: Renderable> Renderable for Option<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if let Some(renderable) = self {
            renderable.render(area, buf);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        if let Some(renderable) = self {
            renderable.desired_height(width)
        } else {
            0
        }
    }
}

impl<R: Renderable> Renderable for Arc<R> {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.as_ref().render(area, buf);
    }
    fn desired_height(&self, width: u16) -> u16 {
        self.as_ref().desired_height(width)
    }
}

/// A container that renders children vertically in a column layout.
///
/// Children are stacked top-to-bottom, each receiving their desired height.
/// If the total desired height exceeds the available area, children at the
/// bottom will be clipped.
///
/// # Examples
///
/// ```rust
/// use tui_components::render::{ColumnRenderable, Renderable};
/// use ratatui::buffer::Buffer;
/// use ratatui::layout::Rect;
///
/// let mut column = ColumnRenderable::new();
/// column.push("Header");
/// column.push("Body");
/// column.push("Footer");
///
/// let height = column.desired_height(80); // Calculate required height
/// let mut buf = Buffer::empty(Rect::new(0, 0, 80, height));
/// column.render(Rect::new(0, 0, 80, height), &mut buf);
/// ```
pub struct ColumnRenderable {
    children: Vec<Box<dyn Renderable>>,
}

impl Renderable for ColumnRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut y = area.y;
        for child in &self.children {
            let child_area = Rect::new(area.x, y, area.width, child.desired_height(area.width))
                .intersection(area);
            if !child_area.is_empty() {
                child.render(child_area, buf);
            }
            y += child_area.height;
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.children
            .iter()
            .map(|child| child.desired_height(width))
            .sum()
    }
}

impl ColumnRenderable {
    /// Creates a new empty column.
    pub fn new() -> Self {
        Self::with(vec![])
    }

    /// Creates a column with the provided children.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tui_components::render::{ColumnRenderable, Renderable};
    ///
    /// let column = ColumnRenderable::with(vec![
    ///     Box::new("Line 1") as Box<dyn Renderable>,
    ///     Box::new("Line 2") as Box<dyn Renderable>,
    /// ]);
    /// ```
    pub fn with(children: impl IntoIterator<Item = Box<dyn Renderable>>) -> Self {
        Self {
            children: children.into_iter().collect(),
        }
    }

    /// Adds a child to the bottom of the column.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tui_components::render::ColumnRenderable;
    ///
    /// let mut column = ColumnRenderable::new();
    /// column.push("First item");
    /// column.push("Second item");
    /// ```
    pub fn push(&mut self, child: impl Into<Box<dyn Renderable>>) {
        self.children.push(child.into());
    }
}

impl Default for ColumnRenderable {
    fn default() -> Self {
        Self::new()
    }
}

/// A container that renders children horizontally in a row layout.
///
/// Each child is assigned a specific width and rendered left-to-right.
/// Children that would extend beyond the available area are clipped.
///
/// # Examples
///
/// ```rust
/// use tui_components::render::{RowRenderable, Renderable};
/// use ratatui::buffer::Buffer;
/// use ratatui::layout::Rect;
///
/// let mut row = RowRenderable::new();
/// row.push(10, "Left");   // 10 columns wide
/// row.push(20, "Middle"); // 20 columns wide
/// row.push(10, "Right");  // 10 columns wide
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 80, 1));
/// row.render(Rect::new(0, 0, 80, 1), &mut buf);
/// ```
pub struct RowRenderable {
    children: Vec<(u16, Box<dyn Renderable>)>,
}

impl Renderable for RowRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut x = area.x;
        for (width, child) in &self.children {
            let available_width = area.width.saturating_sub(x - area.x);
            let child_area = Rect::new(x, area.y, (*width).min(available_width), area.height);
            if child_area.is_empty() {
                break;
            }
            child.render(child_area, buf);
            x = x.saturating_add(*width);
        }
    }

    fn desired_height(&self, width: u16) -> u16 {
        let mut max_height = 0;
        let mut width_remaining = width;
        for (child_width, child) in &self.children {
            let w = (*child_width).min(width_remaining);
            if w == 0 {
                break;
            }
            let height = child.desired_height(w);
            if height > max_height {
                max_height = height;
            }
            width_remaining = width_remaining.saturating_sub(w);
        }
        max_height
    }
}

impl RowRenderable {
    /// Creates a new empty row.
    pub fn new() -> Self {
        Self { children: vec![] }
    }

    /// Adds a child to the right side of the row with the specified width.
    ///
    /// # Arguments
    ///
    /// * `width` - The width in columns to allocate for this child
    /// * `child` - The renderable child to add
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tui_components::render::RowRenderable;
    ///
    /// let mut row = RowRenderable::new();
    /// row.push(15, "Label:");
    /// row.push(30, "Value content");
    /// ```
    pub fn push(&mut self, width: u16, child: impl Into<Box<dyn Renderable>>) {
        self.children.push((width, child.into()));
    }
}

impl Default for RowRenderable {
    fn default() -> Self {
        Self::new()
    }
}

/// A container that renders a child with padding (insets) on all sides.
///
/// The insets reduce the available space for the child and offset its position.
///
/// # Examples
///
/// ```rust
/// use tui_components::render::{InsetRenderable, Insets, Renderable};
/// use ratatui::buffer::Buffer;
/// use ratatui::layout::Rect;
///
/// let insets = Insets::vh(1, 2); // 1 vertical, 2 horizontal
/// let content = InsetRenderable::new("Padded content", insets);
///
/// let mut buf = Buffer::empty(Rect::new(0, 0, 20, 5));
/// content.render(Rect::new(0, 0, 20, 5), &mut buf);
/// ```
pub struct InsetRenderable {
    child: Box<dyn Renderable>,
    insets: Insets,
}

impl Renderable for InsetRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.child.render(area.inset(self.insets), buf);
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.child
            .desired_height(width - self.insets.left - self.insets.right)
            + self.insets.top
            + self.insets.bottom
    }
}

impl InsetRenderable {
    /// Creates a new inset container with the specified child and insets.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tui_components::render::{InsetRenderable, Insets};
    ///
    /// let padded = InsetRenderable::new("Content", Insets::vh(1, 2));
    /// ```
    pub fn new(child: impl Into<Box<dyn Renderable>>, insets: Insets) -> Self {
        Self {
            child: child.into(),
            insets,
        }
    }
}

/// Extension trait providing convenient inset functionality for any Renderable.
///
/// # Examples
///
/// ```rust
/// use tui_components::render::{Insets, RenderableExt};
///
/// let padded = "My content".inset(Insets::vh(1, 2));
/// ```
pub trait RenderableExt {
    /// Wraps this renderable in an [`InsetRenderable`] with the specified insets.
    fn inset(self, insets: Insets) -> Box<dyn Renderable>;
}

impl<R: Into<Box<dyn Renderable>>> RenderableExt for R {
    fn inset(self, insets: Insets) -> Box<dyn Renderable> {
        Box::new(InsetRenderable {
            child: self.into(),
            insets,
        })
    }
}
