//! Scroll and selection state management for vertical lists.
//!
//! Provides [`ScrollState`] for managing selection and scroll position in
//! list-like UIs with wrap-around navigation and automatic scroll adjustment.

/// Generic scroll/selection state for a vertical list menu.
///
/// Encapsulates the common behavior of a selectable list that supports:
/// - Optional selection (None when list is empty)
/// - Wrap-around navigation on Up/Down
/// - Maintaining a scroll window (`scroll_top`) so the selected row stays visible
///
/// # Examples
///
/// ```rust
/// use codex_tui_components::scroll_state::ScrollState;
///
/// let mut state = ScrollState::new();
/// let list_len = 10;
/// let visible_rows = 5;
///
/// // Initialize selection
/// state.clamp_selection(list_len);
/// assert_eq!(state.selected_idx, Some(0));
///
/// // Navigate down
/// state.move_down_wrap(list_len);
/// state.ensure_visible(list_len, visible_rows);
/// assert_eq!(state.selected_idx, Some(1));
///
/// // Wrap around to bottom
/// state.move_up_wrap(list_len);
/// state.move_up_wrap(list_len);
/// assert_eq!(state.selected_idx, Some(9));
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct ScrollState {
    /// The currently selected index, or None if the list is empty.
    pub selected_idx: Option<usize>,
    /// The index of the first visible row in the scroll window.
    pub scroll_top: usize,
}

impl ScrollState {
    /// Creates a new `ScrollState` with no selection and scroll at top.
    pub fn new() -> Self {
        Self {
            selected_idx: None,
            scroll_top: 0,
        }
    }

    /// Resets selection and scroll position to initial state.
    pub fn reset(&mut self) {
        self.selected_idx = None;
        self.scroll_top = 0;
    }

    /// Clamps selection to be within the [0, len-1] range, or None when empty.
    ///
    /// This ensures the selection index is valid for the current list length.
    /// If the list is empty, selection becomes None.
    ///
    /// # Arguments
    ///
    /// * `len` - The current length of the list
    pub fn clamp_selection(&mut self, len: usize) {
        self.selected_idx = match len {
            0 => None,
            _ => Some(self.selected_idx.unwrap_or(0).min(len - 1)),
        };
        if len == 0 {
            self.scroll_top = 0;
        }
    }

    /// Moves selection up by one, wrapping to the bottom when necessary.
    ///
    /// If at the first item (index 0), wraps to the last item (index len-1).
    /// If the list is empty, selection becomes None.
    ///
    /// # Arguments
    ///
    /// * `len` - The current length of the list
    pub fn move_up_wrap(&mut self, len: usize) {
        if len == 0 {
            self.selected_idx = None;
            self.scroll_top = 0;
            return;
        }
        self.selected_idx = Some(match self.selected_idx {
            Some(idx) if idx > 0 => idx - 1,
            Some(_) => len - 1,
            None => 0,
        });
    }

    /// Moves selection down by one, wrapping to the top when necessary.
    ///
    /// If at the last item (index len-1), wraps to the first item (index 0).
    /// If the list is empty, selection becomes None.
    ///
    /// # Arguments
    ///
    /// * `len` - The current length of the list
    pub fn move_down_wrap(&mut self, len: usize) {
        if len == 0 {
            self.selected_idx = None;
            self.scroll_top = 0;
            return;
        }
        self.selected_idx = Some(match self.selected_idx {
            Some(idx) if idx + 1 < len => idx + 1,
            _ => 0,
        });
    }

    /// Adjusts `scroll_top` so that the current `selected_idx` is visible within
    /// the window of `visible_rows`.
    ///
    /// This should be called after changing selection to ensure the selected item
    /// remains in view. The scroll window will be adjusted up or down as needed.
    ///
    /// # Arguments
    ///
    /// * `len` - The current length of the list
    /// * `visible_rows` - The number of rows visible in the viewport
    pub fn ensure_visible(&mut self, len: usize, visible_rows: usize) {
        if len == 0 || visible_rows == 0 {
            self.scroll_top = 0;
            return;
        }
        if let Some(sel) = self.selected_idx {
            if sel < self.scroll_top {
                self.scroll_top = sel;
            } else {
                let bottom = self.scroll_top + visible_rows - 1;
                if sel > bottom {
                    self.scroll_top = sel + 1 - visible_rows;
                }
            }
        } else {
            self.scroll_top = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ScrollState;

    #[test]
    fn wrap_navigation_and_visibility() {
        let mut s = ScrollState::new();
        let len = 10;
        let vis = 5;

        s.clamp_selection(len);
        assert_eq!(s.selected_idx, Some(0));
        s.ensure_visible(len, vis);
        assert_eq!(s.scroll_top, 0);

        s.move_up_wrap(len);
        s.ensure_visible(len, vis);
        assert_eq!(s.selected_idx, Some(len - 1));
        match s.selected_idx {
            Some(sel) => assert!(s.scroll_top <= sel),
            None => panic!("expected Some(selected_idx) after wrap"),
        }

        s.move_down_wrap(len);
        s.ensure_visible(len, vis);
        assert_eq!(s.selected_idx, Some(0));
        assert_eq!(s.scroll_top, 0);
    }
}
