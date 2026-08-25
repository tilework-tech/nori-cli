use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;
use ratatui::widgets::WidgetRef;

use super::popup_consts::MAX_POPUP_ROWS;
use super::scroll_state::ScrollState;
use nori_harness::HistoryEntry;

pub(crate) struct HistorySearchPopup {
    query: String,
    all_entries: Vec<HistoryEntry>,
    filtered_indices: Vec<usize>,
    scroll: ScrollState,
    search_active: bool,
    loading: bool,
}

impl HistorySearchPopup {
    pub(crate) fn new() -> Self {
        Self {
            query: String::new(),
            all_entries: Vec::new(),
            filtered_indices: Vec::new(),
            scroll: ScrollState::new(),
            search_active: false,
            loading: true,
        }
    }

    pub(crate) fn set_entries(&mut self, entries: Vec<HistoryEntry>) {
        self.all_entries = entries;
        self.loading = false;
        self.refilter();
    }

    #[cfg(test)]
    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    #[cfg(test)]
    pub(crate) fn set_query(&mut self, query: String) {
        self.query = query;
        self.refilter();
    }

    pub(crate) fn push_char(&mut self, ch: char) {
        self.query.push(ch);
        self.refilter();
    }

    pub(crate) fn pop_char(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub(crate) fn move_up(&mut self) {
        let len = self.filtered_indices.len();
        self.scroll.move_up_wrap(len);
        self.scroll.ensure_visible(len, len.min(MAX_POPUP_ROWS));
    }

    pub(crate) fn move_down(&mut self) {
        let len = self.filtered_indices.len();
        self.scroll.move_down_wrap(len);
        self.scroll.ensure_visible(len, len.min(MAX_POPUP_ROWS));
    }

    pub(crate) fn selected_text(&self) -> Option<&str> {
        let idx = self.scroll.selected_idx?;
        let entry_idx = *self.filtered_indices.get(idx)?;
        Some(&self.all_entries[entry_idx].text)
    }

    #[cfg(test)]
    pub(crate) fn filtered_count(&self) -> usize {
        self.filtered_indices.len()
    }

    pub(crate) fn is_search_active(&self) -> bool {
        self.search_active
    }

    pub(crate) fn activate_search(&mut self) {
        self.search_active = true;
    }

    pub(crate) fn deactivate_search(&mut self) {
        self.search_active = false;
        self.query.clear();
        self.refilter();
    }

    pub(crate) fn calculate_required_height(&self) -> u16 {
        let result_rows = self
            .filtered_indices
            .len()
            .min(super::popup_consts::MAX_POPUP_ROWS)
            .max(1);
        (1 + result_rows + usize::from(self.search_active)) as u16
    }

    fn refilter(&mut self) {
        let query_lower = self.query.to_lowercase();
        self.filtered_indices = self
            .all_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| query_lower.is_empty() || e.text.to_lowercase().contains(&query_lower))
            .map(|(i, _)| i)
            .collect();
        self.scroll.clamp_selection(self.filtered_indices.len());
    }
}

impl WidgetRef for HistorySearchPopup {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let search_height = u16::from(self.search_active);
        if self.search_active {
            let mut search_spans: Vec<Span> = vec!["search: ".dim()];
            if !self.query.is_empty() {
                search_spans.push(self.query.clone().into());
            }
            Line::from(search_spans).render(
                Rect {
                    x: area.x + 2,
                    y: area.y,
                    width: area.width.saturating_sub(2),
                    height: 1,
                },
                buf,
            );
        }

        let result_area_y = area.y + search_height;
        let result_rows = (area.height as usize)
            .saturating_sub(1 + search_height as usize)
            .min(MAX_POPUP_ROWS);
        let filtered_len = self.filtered_indices.len();

        if filtered_len == 0 {
            if area.height > 1 {
                let msg = if self.loading {
                    "loading..."
                } else {
                    "no matches"
                };
                Line::from(msg.dim().italic()).render(
                    Rect {
                        x: area.x + 2,
                        y: result_area_y,
                        width: area.width.saturating_sub(2),
                        height: 1,
                    },
                    buf,
                );
            }
        } else {
            let visible = result_rows.min(filtered_len);
            let mut start = self.scroll.scroll_top;
            if let Some(sel) = self.scroll.selected_idx {
                if sel < start {
                    start = sel;
                } else if visible > 0 && sel >= start + visible {
                    start = sel + 1 - visible;
                }
            }

            let content_width = area.width.saturating_sub(2) as usize;
            for (row_i, &entry_idx) in self
                .filtered_indices
                .iter()
                .skip(start)
                .take(visible)
                .enumerate()
            {
                let entry = &self.all_entries[entry_idx];
                let is_selected = self.scroll.selected_idx == Some(start + row_i);

                // Truncate text to fit in one line
                let display: String = entry.text.chars().take(content_width).collect();

                let line = if is_selected {
                    Line::from(display.cyan().bold())
                } else {
                    Line::from(display)
                };

                let row_y = result_area_y + row_i as u16;
                if row_y < area.y + area.height {
                    line.render(
                        Rect {
                            x: area.x + 2,
                            y: row_y,
                            width: area.width.saturating_sub(2),
                            height: 1,
                        },
                        buf,
                    );
                }
            }
        }

        // Last row: status line
        let status_y = area.y + area.height - 1;
        if status_y > area.y {
            let hint = if self.search_active {
                "esc stop search  enter select  up/down move  type to filter"
            } else {
                "esc close  enter select  j/k move  f, /, ctrl+f search"
            };
            Line::from(hint.dim()).render(
                Rect {
                    x: area.x + 2,
                    y: status_y,
                    width: area.width.saturating_sub(2),
                    height: 1,
                },
                buf,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn make_entry(text: &str, ts: u64) -> HistoryEntry {
        HistoryEntry {
            session_id: "test-session".to_string(),
            ts,
            text: text.to_string(),
        }
    }

    #[test]
    fn new_popup_has_empty_state() {
        let popup = HistorySearchPopup::new();
        assert_eq!(popup.query(), "");
        assert_eq!(popup.filtered_count(), 0);
        assert!(popup.selected_text().is_none());
        assert!(!popup.is_search_active());
    }

    #[test]
    fn set_entries_shows_all_when_no_query() {
        let mut popup = HistorySearchPopup::new();
        popup.set_entries(vec![
            make_entry("hello world", 1),
            make_entry("foo bar", 2),
            make_entry("baz qux", 3),
        ]);
        assert_eq!(popup.filtered_count(), 3);
    }

    #[test]
    fn set_query_filters_entries_case_insensitive() {
        let mut popup = HistorySearchPopup::new();
        popup.set_entries(vec![
            make_entry("Hello World", 1),
            make_entry("foo bar", 2),
            make_entry("hello again", 3),
        ]);
        popup.set_query("hello".to_string());
        assert_eq!(popup.filtered_count(), 2);
    }

    #[test]
    fn calculate_required_height_accounts_for_entries() {
        let mut popup = HistorySearchPopup::new();
        // Empty result row + status line.
        assert_eq!(popup.calculate_required_height(), 2);

        popup.set_entries(vec![make_entry("a", 1), make_entry("b", 2)]);
        // Two entries + status line while navigating.
        assert_eq!(popup.calculate_required_height(), 3);

        popup.activate_search();
        // Two entries + search line + status line while filtering.
        assert_eq!(popup.calculate_required_height(), 4);
    }

}
