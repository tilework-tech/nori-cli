//! Incremental text wrapping for streaming content.
//!
//! This module provides [`RowBuilder`], which incrementally wraps text fragments into
//! display rows of a fixed width. It's designed for streaming scenarios where text arrives
//! in chunks and needs to be wrapped on-the-fly.
//!
//! ## Key Features
//!
//! - Incremental wrapping: process text fragments as they arrive
//! - Unicode-aware width calculation
//! - Explicit line break tracking
//! - Dynamic width changes with automatic rewrapping
//! - Fragmentation invariance: results don't depend on how input is chunked
//!
//! ## Examples
//!
//! ```rust
//! use tui_components::live_wrap::RowBuilder;
//!
//! let mut builder = RowBuilder::new(40);
//! builder.push_fragment("Hello world, this is streaming text");
//! let rows = builder.display_rows();
//! ```

use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

/// A single visual row produced by [`RowBuilder`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The text content of this row.
    pub text: String,
    /// True if this row ends with an explicit line break (newline),
    /// as opposed to a hard wrap due to width constraints.
    pub explicit_break: bool,
}

impl Row {
    /// Calculate the display width of this row in terminal columns.
    pub fn width(&self) -> usize {
        self.text.width()
    }
}

/// Incrementally wraps input text into visual rows of at most `width` columns.
///
/// This builder processes text fragments one at a time, wrapping them into rows
/// that fit within the specified width. It handles newlines explicitly and can
/// rewrap all content if the width changes.
///
/// # Examples
///
/// ```rust
/// use tui_components::live_wrap::RowBuilder;
///
/// let mut builder = RowBuilder::new(20);
/// builder.push_fragment("Hello ");
/// builder.push_fragment("world!\n");
/// builder.push_fragment("More text");
///
/// let rows = builder.display_rows();
/// assert_eq!(rows[0].text, "Hello world!");
/// assert!(rows[0].explicit_break);
/// ```
pub struct RowBuilder {
    target_width: usize,
    /// Buffer for the current logical line (until a '\n' is seen).
    current_line: String,
    /// Output rows built so far for the current logical line and previous ones.
    rows: Vec<Row>,
}

impl RowBuilder {
    /// Create a new `RowBuilder` with the specified width.
    ///
    /// Width must be at least 1. Smaller values will be clamped to 1.
    pub fn new(target_width: usize) -> Self {
        Self {
            target_width: target_width.max(1),
            current_line: String::new(),
            rows: Vec::new(),
        }
    }

    /// Get the current target width.
    pub fn width(&self) -> usize {
        self.target_width
    }

    /// Change the target width and rewrap all existing content.
    ///
    /// This reconstructs the full text from all rows, then rewraps it at the new width.
    /// Width must be at least 1. Smaller values will be clamped to 1.
    pub fn set_width(&mut self, width: usize) {
        self.target_width = width.max(1);
        // Rewrap everything we have.
        let mut all = String::new();
        for row in self.rows.drain(..) {
            all.push_str(&row.text);
            if row.explicit_break {
                all.push('\n');
            }
        }
        all.push_str(&self.current_line);
        self.current_line.clear();
        self.push_fragment(&all);
    }

    /// Push a text fragment, which may contain newlines.
    ///
    /// The fragment is incrementally wrapped into rows. Any complete rows are
    /// added to the internal buffer. Partial content remains in `current_line`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tui_components::live_wrap::RowBuilder;
    ///
    /// let mut builder = RowBuilder::new(10);
    /// builder.push_fragment("short");
    /// builder.push_fragment(" line\n");
    /// builder.push_fragment("next");
    /// ```
    pub fn push_fragment(&mut self, fragment: &str) {
        if fragment.is_empty() {
            return;
        }
        let mut start = 0usize;
        for (i, ch) in fragment.char_indices() {
            if ch == '\n' {
                // Flush anything pending before the newline.
                if start < i {
                    self.current_line.push_str(&fragment[start..i]);
                }
                self.flush_current_line(true);
                start = i + ch.len_utf8();
            }
        }
        if start < fragment.len() {
            self.current_line.push_str(&fragment[start..]);
            self.wrap_current_line();
        }
    }

    /// Mark the end of the current logical line.
    ///
    /// Equivalent to pushing a newline character. Any buffered content is flushed
    /// as a row with `explicit_break = true`.
    pub fn end_line(&mut self) {
        self.flush_current_line(true);
    }

    /// Drain and return all produced rows, clearing the internal buffer.
    ///
    /// This does NOT include any partial content in `current_line`.
    /// Use [`display_rows`](Self::display_rows) if you want to include partial content.
    pub fn drain_rows(&mut self) -> Vec<Row> {
        std::mem::take(&mut self.rows)
    }

    /// Return a reference to produced rows (non-draining).
    ///
    /// Does NOT include any partial content in `current_line`.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Get all rows suitable for display, including the current partial line.
    ///
    /// If there is any buffered content in `current_line`, it is appended as a row
    /// with `explicit_break = false`.
    pub fn display_rows(&self) -> Vec<Row> {
        let mut out = self.rows.clone();
        if !self.current_line.is_empty() {
            out.push(Row {
                text: self.current_line.clone(),
                explicit_break: false,
            });
        }
        out
    }

    /// Drain the oldest rows that exceed `max_keep` display rows.
    ///
    /// This is useful for implementing scrolling buffers. Returns the drained rows
    /// in order. The count includes the current partial line if any.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use tui_components::live_wrap::RowBuilder;
    ///
    /// let mut builder = RowBuilder::new(10);
    /// for i in 0..10 {
    ///     builder.push_fragment(&format!("line {}\n", i));
    /// }
    /// // Keep only the last 5 rows
    /// let old_rows = builder.drain_commit_ready(5);
    /// assert_eq!(old_rows.len(), 5);
    /// ```
    pub fn drain_commit_ready(&mut self, max_keep: usize) -> Vec<Row> {
        let display_count = self.rows.len() + if self.current_line.is_empty() { 0 } else { 1 };
        if display_count <= max_keep {
            return Vec::new();
        }
        let to_commit = display_count - max_keep;
        let commit_count = to_commit.min(self.rows.len());
        let mut drained = Vec::with_capacity(commit_count);
        for _ in 0..commit_count {
            drained.push(self.rows.remove(0));
        }
        drained
    }

    fn flush_current_line(&mut self, explicit_break: bool) {
        // Wrap any remaining content in the current line and then finalize with explicit_break.
        self.wrap_current_line();
        // If the current line ended exactly on a width boundary and is non-empty, represent
        // the explicit break as an empty explicit row so that fragmentation invariance holds.
        if explicit_break {
            if self.current_line.is_empty() {
                // We ended on a boundary previously; add an empty explicit row.
                self.rows.push(Row {
                    text: String::new(),
                    explicit_break: true,
                });
            } else {
                // There is leftover content that did not wrap yet; push it now with the explicit flag.
                let mut s = String::new();
                std::mem::swap(&mut s, &mut self.current_line);
                self.rows.push(Row {
                    text: s,
                    explicit_break: true,
                });
            }
        }
        // Reset current line buffer for next logical line.
        self.current_line.clear();
    }

    fn wrap_current_line(&mut self) {
        // While the current_line exceeds width, cut a prefix.
        loop {
            if self.current_line.is_empty() {
                break;
            }
            let (prefix, suffix, taken) =
                take_prefix_by_width(&self.current_line, self.target_width);
            if taken == 0 {
                // Avoid infinite loop on pathological inputs; take one scalar and continue.
                if let Some((i, ch)) = self.current_line.char_indices().next() {
                    let len = i + ch.len_utf8();
                    let p = self.current_line[..len].to_string();
                    self.rows.push(Row {
                        text: p,
                        explicit_break: false,
                    });
                    self.current_line = self.current_line[len..].to_string();
                    continue;
                }
                break;
            }
            if suffix.is_empty() {
                // Fits entirely; keep in buffer (do not push yet) so we can append more later.
                break;
            } else {
                // Emit wrapped prefix as a non-explicit row and continue with the remainder.
                self.rows.push(Row {
                    text: prefix,
                    explicit_break: false,
                });
                self.current_line = suffix.to_string();
            }
        }
    }
}

/// Take a prefix of `text` whose visible width is at most `max_cols`.
///
/// Returns `(prefix, suffix, prefix_width)` where:
/// - `prefix`: the longest substring that fits within `max_cols`
/// - `suffix`: the remaining text after the prefix
/// - `prefix_width`: the actual display width of the prefix in columns
///
/// # Examples
///
/// ```rust
/// use tui_components::live_wrap::take_prefix_by_width;
///
/// let (prefix, suffix, width) = take_prefix_by_width("hello world", 5);
/// assert_eq!(prefix, "hello");
/// assert_eq!(suffix, " world");
/// assert_eq!(width, 5);
/// ```
pub fn take_prefix_by_width(text: &str, max_cols: usize) -> (String, &str, usize) {
    if max_cols == 0 || text.is_empty() {
        return (String::new(), text, 0);
    }
    let mut cols = 0usize;
    let mut end_idx = 0usize;
    for (i, ch) in text.char_indices() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if cols.saturating_add(ch_width) > max_cols {
            break;
        }
        cols += ch_width;
        end_idx = i + ch.len_utf8();
        if cols == max_cols {
            break;
        }
    }
    let prefix = text[..end_idx].to_string();
    let suffix = &text[end_idx..];
    (prefix, suffix, cols)
}
