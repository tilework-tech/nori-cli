use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;

use super::popup_consts::MAX_POPUP_ROWS;
use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::measure_rows_height;
use super::selection_popup_common::render_rows;
use crate::render::Insets;
use crate::render::RectExt;
use codex_common::fuzzy_match::fuzzy_match;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SkillPickerItem {
    pub(crate) display_name: String,
    pub(crate) insert_text: String,
    pub(crate) description: String,
}

pub(crate) struct SkillPopup {
    filter: String,
    items: Vec<SkillPickerItem>,
    state: ScrollState,
}

impl SkillPopup {
    pub(crate) fn new(items: Vec<SkillPickerItem>) -> Self {
        Self {
            filter: String::new(),
            items,
            state: ScrollState::new(),
        }
    }

    pub(crate) fn set_items(&mut self, items: Vec<SkillPickerItem>) {
        self.items = items;
        self.clamp_selection();
    }

    pub(crate) fn on_query_change(&mut self, query: String) {
        self.filter = query;
        self.clamp_selection();
    }

    pub(crate) fn has_matches(&self) -> bool {
        !self.filtered().is_empty()
    }

    pub(crate) fn calculate_required_height(&self, width: u16) -> u16 {
        let rows = self.rows_from_matches(self.filtered());
        measure_rows_height(&rows, &self.state, MAX_POPUP_ROWS, width.saturating_sub(2))
    }

    pub(crate) fn move_up(&mut self) {
        let len = self.filtered().len();
        self.state.move_up_wrap(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    pub(crate) fn move_down(&mut self) {
        let len = self.filtered().len();
        self.state.move_down_wrap(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    pub(crate) fn selected_item(&self) -> Option<SkillPickerItem> {
        let matches = self.filtered();
        self.state
            .selected_idx
            .and_then(|idx| matches.get(idx).map(|(item, _, _)| item.clone()))
    }

    fn clamp_selection(&mut self) {
        let matches_len = self.filtered().len();
        self.state.clamp_selection(matches_len);
        self.state
            .ensure_visible(matches_len, MAX_POPUP_ROWS.min(matches_len));
    }

    fn filtered(&self) -> Vec<(SkillPickerItem, Option<Vec<usize>>, i32)> {
        let filter = self.filter.trim();
        let mut out = Vec::new();

        for item in &self.items {
            if filter.is_empty() {
                out.push((item.clone(), None, 0));
            } else if let Some((indices, score)) = fuzzy_match(&item.display_name, filter) {
                out.push((item.clone(), Some(indices), score));
            }
        }

        out.sort_by(|a, b| {
            a.2.cmp(&b.2)
                .then_with(|| a.0.display_name.cmp(&b.0.display_name))
                .then_with(|| a.0.insert_text.cmp(&b.0.insert_text))
        });
        out
    }

    fn rows_from_matches(
        &self,
        matches: Vec<(SkillPickerItem, Option<Vec<usize>>, i32)>,
    ) -> Vec<GenericDisplayRow> {
        matches
            .into_iter()
            .map(|(item, indices, _)| GenericDisplayRow {
                name: item.display_name,
                match_indices: indices,
                display_shortcut: None,
                description: Some(item.description),
                styled_description: None,
                disabled: false,
                is_header: false,
                two_line: false,
            })
            .collect()
    }
}

impl WidgetRef for SkillPopup {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let rows = self.rows_from_matches(self.filtered());
        render_rows(
            area.inset(Insets::tlbr(0, 2, 0, 0)),
            buf,
            &rows,
            &self.state,
            MAX_POPUP_ROWS,
            "no skills",
        );
    }
}
