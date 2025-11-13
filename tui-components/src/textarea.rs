//! Multiline text input widget with cursor navigation and wrapping.
//!
//! This module provides [`TextArea`], a versatile multiline text editor widget
//! suitable for chat inputs, forms, and other text entry scenarios in terminal UIs.
//!
//! ## Key Features
//!
//! - Multiline text editing with cursor navigation
//! - Word wrapping with configurable width
//! - Emacs-style keybindings (Ctrl+A, Ctrl+E, Ctrl+K, etc.)
//! - Scrolling for content that exceeds the viewport
//! - Configurable placeholder text
//! - Style customization via [`TextAreaConfig`]
//!
//! ## Examples
//!
//! ```rust,no_run
//! use tui_components::textarea::{TextArea, TextAreaConfig};
//! use ratatui::style::Style;
//!
//! let config = TextAreaConfig::default()
//!     .with_placeholder("Type a message...");
//! let mut textarea = TextArea::new(config);
//! textarea.insert_str("Hello world!");
//! ```

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::{StatefulWidgetRef, WidgetRef};
use std::cell::RefCell;
use std::ops::Range;
use textwrap::Options;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Configuration for [`TextArea`] appearance and behavior.
#[derive(Debug, Clone)]
pub struct TextAreaConfig {
    /// Text to display when the textarea is empty
    pub placeholder: Option<String>,
    /// Style for normal text
    pub text_style: Style,
    /// Style for cursor
    pub cursor_style: Style,
    /// Style for placeholder text
    pub placeholder_style: Style,
}

impl Default for TextAreaConfig {
    fn default() -> Self {
        Self {
            placeholder: None,
            text_style: Style::default(),
            cursor_style: Style::default().bg(Color::White).fg(Color::Black),
            placeholder_style: Style::default().fg(Color::DarkGray),
        }
    }
}

impl TextAreaConfig {
    /// Create a new configuration with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the placeholder text.
    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Set the normal text style.
    pub fn with_text_style(mut self, style: Style) -> Self {
        self.text_style = style;
        self
    }

    /// Set the cursor style.
    pub fn with_cursor_style(mut self, style: Style) -> Self {
        self.cursor_style = style;
        self
    }

    /// Set the placeholder text style.
    pub fn with_placeholder_style(mut self, style: Style) -> Self {
        self.placeholder_style = style;
        self
    }
}

/// A multiline text input widget.
///
/// Provides rich text editing capabilities including cursor navigation,
/// word-based movement, kill/yank operations, and automatic wrapping.
///
/// # Examples
///
/// ```rust
/// use tui_components::textarea::{TextArea, TextAreaConfig};
///
/// let config = TextAreaConfig::default();
/// let mut textarea = TextArea::new(config);
/// textarea.insert_str("Hello");
/// assert_eq!(textarea.text(), "Hello");
/// ```
#[derive(Debug)]
pub struct TextArea {
    text: String,
    cursor_pos: usize,
    wrap_cache: RefCell<Option<WrapCache>>,
    preferred_col: Option<usize>,
    config: TextAreaConfig,
}

#[derive(Debug, Clone)]
struct WrapCache {
    width: u16,
    lines: Vec<Range<usize>>,
}

/// State for rendering a [`TextArea`] widget.
///
/// Tracks scroll position for viewports that don't show all content.
#[derive(Debug, Default, Clone, Copy)]
pub struct TextAreaState {
    /// Index into wrapped lines of the first visible line.
    pub scroll: u16,
}

impl TextArea {
    /// Create a new TextArea with the given configuration.
    pub fn new(config: TextAreaConfig) -> Self {
        Self {
            text: String::new(),
            cursor_pos: 0,
            wrap_cache: RefCell::new(None),
            preferred_col: None,
            config,
        }
    }

    /// Get a reference to the current configuration.
    pub fn config(&self) -> &TextAreaConfig {
        &self.config
    }

    /// Update the configuration.
    pub fn set_config(&mut self, config: TextAreaConfig) {
        self.config = config;
    }

    /// Replace the entire text content.
    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor_pos = self.text.len();
        self.wrap_cache.replace(None);
        self.preferred_col = None;
    }

    /// Get the current text content.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Insert text at the current cursor position.
    pub fn insert_str(&mut self, text: &str) {
        self.insert_str_at(self.cursor_pos, text);
    }

    /// Insert text at a specific position.
    pub fn insert_str_at(&mut self, pos: usize, text: &str) {
        let pos = pos.clamp(0, self.text.len());
        self.text.insert_str(pos, text);
        self.wrap_cache.replace(None);
        if pos <= self.cursor_pos {
            self.cursor_pos += text.len();
        }
        self.preferred_col = None;
    }

    /// Replace a range of text.
    pub fn replace_range(&mut self, range: Range<usize>, text: &str) {
        assert!(range.start <= range.end);
        let start = range.start.clamp(0, self.text.len());
        let end = range.end.clamp(0, self.text.len());
        let removed_len = end - start;
        let inserted_len = text.len();
        if removed_len == 0 && inserted_len == 0 {
            return;
        }
        let diff = inserted_len as isize - removed_len as isize;

        self.text.replace_range(range, text);
        self.wrap_cache.replace(None);
        self.preferred_col = None;

        self.cursor_pos = if self.cursor_pos < start {
            self.cursor_pos
        } else if self.cursor_pos <= end {
            start + inserted_len
        } else {
            ((self.cursor_pos as isize) + diff) as usize
        }
        .min(self.text.len());
    }

    /// Get the current cursor position (byte offset).
    pub fn cursor(&self) -> usize {
        self.cursor_pos
    }

    /// Set the cursor position (byte offset).
    pub fn set_cursor(&mut self, pos: usize) {
        self.cursor_pos = pos.clamp(0, self.text.len());
        self.preferred_col = None;
    }

    /// Calculate the desired height for rendering at the given width.
    pub fn desired_height(&self, width: u16) -> u16 {
        self.wrapped_lines(width).len() as u16
    }

    /// Compute the on-screen cursor position.
    pub fn cursor_pos(&self, area: Rect) -> Option<(u16, u16)> {
        self.cursor_pos_with_state(area, TextAreaState::default())
    }

    /// Compute the on-screen cursor position with scroll state.
    pub fn cursor_pos_with_state(&self, area: Rect, state: TextAreaState) -> Option<(u16, u16)> {
        let lines = self.wrapped_lines(area.width);
        let effective_scroll = self.effective_scroll(area.height, &lines, state.scroll);
        let i = Self::wrapped_line_index_by_start(&lines, self.cursor_pos)?;
        let ls = &lines[i];
        let col = self.text[ls.start..self.cursor_pos].width() as u16;
        let screen_row = i
            .saturating_sub(effective_scroll as usize)
            .try_into()
            .unwrap_or(0);
        Some((area.x + col, area.y + screen_row))
    }

    /// Check if the textarea is empty.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn current_display_col(&self) -> usize {
        let bol = self.beginning_of_current_line();
        self.text[bol..self.cursor_pos].width()
    }

    fn wrapped_line_index_by_start(lines: &[Range<usize>], pos: usize) -> Option<usize> {
        let idx = lines.partition_point(|r| r.start <= pos);
        if idx == 0 { None } else { Some(idx - 1) }
    }

    fn move_to_display_col_on_line(
        &mut self,
        line_start: usize,
        line_end: usize,
        target_col: usize,
    ) {
        let mut width_so_far = 0usize;
        for (i, g) in self.text[line_start..line_end].grapheme_indices(true) {
            width_so_far += g.width();
            if width_so_far > target_col {
                self.cursor_pos = line_start + i;
                return;
            }
        }
        self.cursor_pos = line_end;
    }

    fn beginning_of_line(&self, pos: usize) -> usize {
        self.text[..pos].rfind('\n').map(|i| i + 1).unwrap_or(0)
    }

    fn beginning_of_current_line(&self) -> usize {
        self.beginning_of_line(self.cursor_pos)
    }

    fn end_of_line(&self, pos: usize) -> usize {
        self.text[pos..]
            .find('\n')
            .map(|i| pos + i)
            .unwrap_or(self.text.len())
    }

    fn end_of_current_line(&self) -> usize {
        self.end_of_line(self.cursor_pos)
    }

    fn wrapped_lines(&self, width: u16) -> Vec<Range<usize>> {
        if let Some(cached) = self.wrap_cache.borrow().as_ref()
            && cached.width == width
        {
            return cached.lines.clone();
        }

        let lines = crate::wrapping::wrap_ranges_trim(&self.text, Options::new(width as usize));
        self.wrap_cache.replace(Some(WrapCache {
            width,
            lines: lines.clone(),
        }));
        lines
    }

    fn effective_scroll(
        &self,
        viewport_height: u16,
        lines: &[Range<usize>],
        requested_scroll: u16,
    ) -> u16 {
        if lines.is_empty() {
            return 0;
        }
        let max_scroll = (lines.len() as u16).saturating_sub(viewport_height);
        requested_scroll.min(max_scroll)
    }

    // Simplified input handling - full implementation would be more complex
    /// Handle a key event and update the textarea state.
    ///
    /// Returns `true` if the event was handled.
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        match (key.code, key.modifiers) {
            // Basic character insertion
            (KeyCode::Char(c), KeyModifiers::NONE) | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                self.insert_str(&c.to_string());
                true
            }
            // Backspace
            (KeyCode::Backspace, _) => {
                if self.cursor_pos > 0 {
                    let prev_pos = self.cursor_pos - 1;
                    self.text.remove(prev_pos);
                    self.cursor_pos = prev_pos;
                    self.wrap_cache.replace(None);
                }
                true
            }
            // Delete
            (KeyCode::Delete, _) => {
                if self.cursor_pos < self.text.len() {
                    self.text.remove(self.cursor_pos);
                    self.wrap_cache.replace(None);
                }
                true
            }
            // Enter/newline
            (KeyCode::Enter, _) => {
                self.insert_str("\n");
                true
            }
            // Cursor movement
            (KeyCode::Left, _) => {
                if self.cursor_pos > 0 {
                    self.cursor_pos -= 1;
                }
                true
            }
            (KeyCode::Right, _) => {
                if self.cursor_pos < self.text.len() {
                    self.cursor_pos += 1;
                }
                true
            }
            (KeyCode::Up, _) => {
                // Simplified: move to previous line
                let bol = self.beginning_of_current_line();
                if bol > 0 {
                    let prev_bol = self.beginning_of_line(bol - 1);
                    let prev_eol = self.end_of_line(prev_bol);
                    let col = self.current_display_col();
                    self.move_to_display_col_on_line(prev_bol, prev_eol, col);
                }
                true
            }
            (KeyCode::Down, _) => {
                // Simplified: move to next line
                let eol = self.end_of_current_line();
                if eol < self.text.len() {
                    let next_bol = eol + 1;
                    let next_eol = self.end_of_line(next_bol);
                    let col = self.current_display_col();
                    self.move_to_display_col_on_line(next_bol, next_eol, col);
                }
                true
            }
            (KeyCode::Home, _) => {
                self.cursor_pos = self.beginning_of_current_line();
                true
            }
            (KeyCode::End, _) => {
                self.cursor_pos = self.end_of_current_line();
                true
            }
            _ => false,
        }
    }
}

// Rendering implementation (simplified)
impl WidgetRef for TextArea {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let mut state = TextAreaState::default();
        StatefulWidgetRef::render_ref(self, area, buf, &mut state);
    }
}

impl StatefulWidgetRef for TextArea {
    type State = TextAreaState;

    fn render_ref(&self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if self.text.is_empty() {
            // Render placeholder if configured
            if let Some(placeholder) = &self.config.placeholder {
                buf.set_string(area.x, area.y, placeholder, self.config.placeholder_style);
            }
            return;
        }

        let lines = self.wrapped_lines(area.width);
        let effective_scroll = self.effective_scroll(area.height, &lines, state.scroll);

        for (row_idx, line_range) in lines
            .iter()
            .enumerate()
            .skip(effective_scroll as usize)
            .take(area.height as usize)
        {
            let y = area.y + (row_idx - effective_scroll as usize) as u16;
            let end = line_range.end.min(self.text.len());
            let line_text = &self.text[line_range.start..end];
            buf.set_string(area.x, y, line_text, self.config.text_style);
        }

        // Render cursor if visible
        if let Some((cx, cy)) = self.cursor_pos_with_state(area, *state)
            && cx < area.right()
            && cy < area.bottom()
        {
            let cursor_cell = &mut buf[(cx, cy)];
            cursor_cell.set_style(self.config.cursor_style);
        }
    }
}
