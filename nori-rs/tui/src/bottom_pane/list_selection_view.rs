use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use itertools::Itertools as _;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::app_event_sender::AppEventSender;
use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::render::Insets;
use crate::render::RectExt as _;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::Renderable;
use crate::style::user_message_style;

use super::CancellationEvent;
use super::bottom_pane_view::BottomPaneView;
use super::popup_consts::MAX_POPUP_ROWS;
use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::measure_rows_height;
use super::selection_popup_common::render_rows;

/// One selectable item in the generic selection list.
pub(crate) type SelectionAction = Box<dyn Fn(&AppEventSender) + Send + Sync>;

#[derive(Default)]
pub(crate) struct SelectionItem {
    pub name: String,
    pub display_shortcut: Option<KeyBinding>,
    pub description: Option<String>,
    pub selected_description: Option<String>,
    pub is_current: bool,
    pub actions: Vec<SelectionAction>,
    pub dismiss_on_select: bool,
    pub search_value: Option<String>,
}

pub(crate) struct SelectionViewParams {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub footer_hint: Option<Line<'static>>,
    pub items: Vec<SelectionItem>,
    pub is_searchable: bool,
    pub search_placeholder: Option<String>,
    pub header: Box<dyn Renderable>,
    pub initial_selected_idx: Option<usize>,
    /// Optional callback fired when the picker is dismissed without selection
    /// (e.g. via Escape or Ctrl-C).
    pub on_dismiss: Option<SelectionAction>,
    /// When true, j/k navigate and `/` toggles search mode.
    /// When false (default), typing goes directly to search if `is_searchable`.
    pub vim_mode: bool,
}

impl Default for SelectionViewParams {
    fn default() -> Self {
        Self {
            title: None,
            subtitle: None,
            footer_hint: None,
            items: Vec::new(),
            is_searchable: false,
            search_placeholder: None,
            header: Box::new(()),
            initial_selected_idx: None,
            on_dismiss: None,
            vim_mode: false,
        }
    }
}

pub(crate) struct ListSelectionView {
    footer_hint: Option<Line<'static>>,
    items: Vec<SelectionItem>,
    state: ScrollState,
    complete: bool,
    app_event_tx: AppEventSender,
    is_searchable: bool,
    search_query: String,
    search_placeholder: Option<String>,
    filtered_indices: Vec<usize>,
    last_selected_actual_idx: Option<usize>,
    header: Box<dyn Renderable>,
    initial_selected_idx: Option<usize>,
    on_dismiss: Option<SelectionAction>,
    vim_mode: bool,
    search_active: bool,
}

impl ListSelectionView {
    pub fn new(params: SelectionViewParams, app_event_tx: AppEventSender) -> Self {
        let mut header = params.header;
        if params.title.is_some() || params.subtitle.is_some() {
            let title = params.title.map(|title| Line::from(title.bold()));
            let subtitle = params.subtitle.map(|subtitle| Line::from(subtitle.dim()));
            header = Box::new(ColumnRenderable::with([
                header,
                Box::new(title),
                Box::new(subtitle),
            ]));
        }
        let mut s = Self {
            footer_hint: params.footer_hint,
            items: params.items,
            state: ScrollState::new(),
            complete: false,
            app_event_tx,
            is_searchable: params.is_searchable,
            search_query: String::new(),
            search_placeholder: if params.is_searchable {
                Some(
                    params
                        .search_placeholder
                        .unwrap_or_else(|| "Type to filter".to_string()),
                )
            } else {
                None
            },
            filtered_indices: Vec::new(),
            last_selected_actual_idx: None,
            header,
            initial_selected_idx: params.initial_selected_idx,
            on_dismiss: params.on_dismiss,
            vim_mode: params.vim_mode,
            search_active: false,
        };
        s.apply_filter();
        s
    }

    pub(crate) fn update_item(
        &mut self,
        stable_id: &str,
        name: String,
        description: Option<String>,
        search_value: String,
    ) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| {
            item.search_value
                .as_deref()
                .map(search_value_id)
                .is_some_and(|id| id == stable_id)
        }) else {
            return false;
        };

        item.name = name;
        item.description = description;
        item.search_value = Some(search_value);
        self.apply_filter();
        true
    }

    pub(crate) fn remove_item(&mut self, stable_id: &str) -> bool {
        let Some(index) = self.items.iter().position(|item| {
            item.search_value
                .as_deref()
                .map(search_value_id)
                .is_some_and(|id| id == stable_id)
        }) else {
            return false;
        };

        self.items.remove(index);
        self.apply_filter();
        true
    }

    fn visible_len(&self) -> usize {
        self.filtered_indices.len()
    }

    fn max_visible_rows(len: usize) -> usize {
        MAX_POPUP_ROWS.min(len.max(1))
    }

    fn apply_filter(&mut self) {
        let previously_selected = self
            .state
            .selected_idx
            .and_then(|visible_idx| self.filtered_indices.get(visible_idx).copied())
            .or_else(|| self.initial_selected_idx.take())
            .or_else(|| {
                (!self.is_searchable)
                    .then(|| self.items.iter().position(|item| item.is_current))
                    .flatten()
            });

        if self.is_searchable && !self.search_query.is_empty() {
            let query_lower = self.search_query.to_lowercase();
            self.filtered_indices = self
                .items
                .iter()
                .positions(|item| {
                    item.search_value
                        .as_ref()
                        .is_some_and(|v| v.to_lowercase().contains(&query_lower))
                })
                .collect();
        } else {
            self.filtered_indices = (0..self.items.len()).collect();
        }

        let len = self.filtered_indices.len();
        self.state.selected_idx = self
            .state
            .selected_idx
            .and_then(|visible_idx| {
                self.filtered_indices
                    .get(visible_idx)
                    .and_then(|idx| self.filtered_indices.iter().position(|cur| cur == idx))
            })
            .or_else(|| {
                previously_selected.and_then(|actual_idx| {
                    self.filtered_indices
                        .iter()
                        .position(|idx| *idx == actual_idx)
                })
            })
            .or_else(|| (len > 0).then_some(0));

        let visible = Self::max_visible_rows(len);
        self.state.clamp_selection(len);
        self.state.ensure_visible(len, visible);
    }

    fn build_rows(&self) -> Vec<GenericDisplayRow> {
        self.filtered_indices
            .iter()
            .enumerate()
            .filter_map(|(visible_idx, actual_idx)| {
                self.items.get(*actual_idx).map(|item| {
                    let is_selected = self.state.selected_idx == Some(visible_idx);
                    let prefix = if is_selected { '›' } else { ' ' };
                    let name = item.name.as_str();
                    let name_with_marker = if item.is_current {
                        format!("{name} (current)")
                    } else {
                        item.name.clone()
                    };
                    let n = visible_idx + 1;
                    let show_numbers =
                        !self.is_searchable || (self.vim_mode && !self.search_active);
                    let display_name = if show_numbers {
                        format!("{prefix} {n}. {name_with_marker}")
                    } else {
                        format!("{prefix} {name_with_marker}")
                    };
                    let description = is_selected
                        .then(|| item.selected_description.clone())
                        .flatten()
                        .or_else(|| item.description.clone());
                    GenericDisplayRow {
                        name: display_name,
                        display_shortcut: item.display_shortcut,
                        match_indices: None,
                        description,
                    }
                })
            })
            .collect()
    }

    fn move_up(&mut self) {
        let len = self.visible_len();
        self.state.move_up_wrap(len);
        let visible = Self::max_visible_rows(len);
        self.state.ensure_visible(len, visible);
    }

    fn move_down(&mut self) {
        let len = self.visible_len();
        self.state.move_down_wrap(len);
        let visible = Self::max_visible_rows(len);
        self.state.ensure_visible(len, visible);
    }

    fn accept(&mut self) {
        if let Some(idx) = self.state.selected_idx
            && let Some(actual_idx) = self.filtered_indices.get(idx)
            && let Some(item) = self.items.get(*actual_idx)
        {
            self.last_selected_actual_idx = Some(*actual_idx);
            for act in &item.actions {
                act(&self.app_event_tx);
            }
            if item.dismiss_on_select {
                self.complete = true;
            }
        } else {
            self.complete = true;
        }
    }

    #[cfg(test)]
    pub(crate) fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.apply_filter();
    }

    pub(crate) fn take_last_selected_index(&mut self) -> Option<usize> {
        self.last_selected_actual_idx.take()
    }

    /// Whether the search input row should be shown.
    /// In vim mode, only show when search is active. In non-vim mode, always show if searchable.
    fn show_search_row(&self) -> bool {
        self.is_searchable && (!self.vim_mode || self.search_active)
    }

    /// Compute the effective footer hint based on current state.
    fn effective_footer_hint(&self) -> Option<Line<'static>> {
        // If a static footer was provided, use it.
        if self.footer_hint.is_some() {
            return self.footer_hint.clone();
        }
        // For searchable views, generate a context-sensitive hint.
        if !self.is_searchable {
            return None;
        }
        if self.vim_mode && self.search_active {
            Some(Line::from(vec![
                "type to filter, ".into(),
                key_hint::plain(KeyCode::Enter).into(),
                " confirm, ".into(),
                key_hint::plain(KeyCode::Esc).into(),
                " cancel search".into(),
            ]))
        } else if self.vim_mode {
            Some(Line::from(vec![
                "↑/k ↓/j navigate, ".into(),
                "/ ".into(),
                "search, ".into(),
                key_hint::plain(KeyCode::Enter).into(),
                " confirm, ".into(),
                key_hint::plain(KeyCode::Esc).into(),
                " go back".into(),
            ]))
        } else {
            Some(Line::from(vec![
                "↑/↓ navigate, type to filter, ".into(),
                key_hint::plain(KeyCode::Enter).into(),
                " confirm, ".into(),
                key_hint::plain(KeyCode::Esc).into(),
                " go back".into(),
            ]))
        }
    }

    fn rows_width(total_width: u16) -> u16 {
        total_width.saturating_sub(2)
    }
}

fn search_value_id(search_value: &str) -> &str {
    search_value
        .split_once(' ')
        .map_or(search_value, |(id, _)| id)
}

impl BottomPaneView for ListSelectionView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event {
            KeyEvent {
                code: KeyCode::Up, ..
            } => self.move_up(),
            KeyEvent {
                code: KeyCode::Down,
                ..
            } => self.move_down(),
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } if self.is_searchable && (!self.vim_mode || self.search_active) => {
                self.search_query.pop();
                self.apply_filter();
            }
            KeyEvent {
                code: KeyCode::Esc, ..
            } if self.vim_mode && self.search_active => {
                // Exit search mode without dismissing the popup.
                self.search_active = false;
                self.search_query.clear();
                self.apply_filter();
            }
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                self.on_ctrl_c();
            }
            // Vim mode + searchable + search active: chars go to search query.
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                ..
            } if self.is_searchable
                && self.vim_mode
                && self.search_active
                && !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
            {
                self.search_query.push(c);
                self.apply_filter();
            }
            // Vim mode + searchable + NOT searching: j/k navigate, / starts search, digits select.
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                ..
            } if self.is_searchable
                && self.vim_mode
                && !self.search_active
                && !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
            {
                match c {
                    'k' => self.move_up(),
                    'j' => self.move_down(),
                    '/' => {
                        self.search_active = true;
                    }
                    _ => {
                        if let Some(idx) = c
                            .to_digit(10)
                            .map(|d| d as usize)
                            .and_then(|d| d.checked_sub(1))
                            && idx < self.items.len()
                        {
                            self.state.selected_idx = Some(idx);
                            self.accept();
                        }
                    }
                }
            }
            // Non-vim searchable: chars go directly to search query.
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                ..
            } if self.is_searchable
                && !self.vim_mode
                && !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
            {
                self.search_query.push(c);
                self.apply_filter();
            }
            // Not searchable: j/k navigate, digits select.
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                ..
            } if !self.is_searchable
                && !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
            {
                match c {
                    'k' => self.move_up(),
                    'j' => self.move_down(),
                    _ => {
                        if let Some(idx) = c
                            .to_digit(10)
                            .map(|d| d as usize)
                            .and_then(|d| d.checked_sub(1))
                            && idx < self.items.len()
                        {
                            self.state.selected_idx = Some(idx);
                            self.accept();
                        }
                    }
                }
            }
            KeyEvent {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::NONE,
                ..
            } => self.accept(),
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        if let Some(cb) = self.on_dismiss.take() {
            cb(&self.app_event_tx);
        }
        self.complete = true;
        CancellationEvent::Handled
    }

    fn update_selection_item(
        &mut self,
        stable_id: &str,
        name: String,
        description: Option<String>,
        search_value: String,
    ) -> bool {
        self.update_item(stable_id, name, description, search_value)
    }

    fn remove_selection_item(&mut self, stable_id: &str) -> bool {
        self.remove_item(stable_id)
    }
}

impl Renderable for ListSelectionView {
    fn desired_height(&self, width: u16) -> u16 {
        // Measure wrapped height for up to MAX_POPUP_ROWS items at the given width.
        // Build the same display rows used by the renderer so wrapping math matches.
        let rows = self.build_rows();
        let rows_width = Self::rows_width(width);
        let rows_height = measure_rows_height(&rows, &self.state, MAX_POPUP_ROWS, rows_width);

        // Subtract 4 for the padding on the left and right of the header.
        let mut height = self.header.desired_height(width.saturating_sub(4));
        height = height.saturating_add(rows_height + 3);
        if self.show_search_row() {
            height = height.saturating_add(1);
        }
        if self.effective_footer_hint().is_some() {
            height = height.saturating_add(1);
        }
        height
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let effective_hint = self.effective_footer_hint();
        let [content_area, footer_area] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(if effective_hint.is_some() { 1 } else { 0 }),
        ])
        .areas(area);

        Block::default()
            .style(user_message_style())
            .render(content_area, buf);

        let header_height = self
            .header
            // Subtract 4 for the padding on the left and right of the header.
            .desired_height(content_area.width.saturating_sub(4));
        let rows = self.build_rows();
        let rows_width = Self::rows_width(content_area.width);
        let rows_height = measure_rows_height(&rows, &self.state, MAX_POPUP_ROWS, rows_width);
        let [header_area, _, search_area, list_area] = Layout::vertical([
            Constraint::Max(header_height),
            Constraint::Max(1),
            Constraint::Length(if self.show_search_row() { 1 } else { 0 }),
            Constraint::Length(rows_height),
        ])
        .areas(content_area.inset(Insets::vh(1, 2)));

        if header_area.height < header_height {
            let [header_area, elision_area] =
                Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(header_area);
            self.header.render(header_area, buf);
            Paragraph::new(vec![
                Line::from(format!("[… {header_height} lines] ctrl + a view all")).dim(),
            ])
            .render(elision_area, buf);
        } else {
            self.header.render(header_area, buf);
        }

        if self.show_search_row() {
            Line::from(self.search_query.clone()).render(search_area, buf);
            let query_span: Span<'static> = if self.search_query.is_empty() {
                self.search_placeholder
                    .as_ref()
                    .map(|placeholder| placeholder.clone().dim())
                    .unwrap_or_else(|| "".into())
            } else {
                self.search_query.clone().into()
            };
            Line::from(query_span).render(search_area, buf);
        }

        if list_area.height > 0 {
            let render_area = Rect {
                x: list_area.x.saturating_sub(2),
                y: list_area.y,
                width: rows_width.max(1),
                height: list_area.height,
            };
            render_rows(
                render_area,
                buf,
                &rows,
                &self.state,
                render_area.height as usize,
                "no matches",
            );
        }

        if let Some(hint) = effective_hint {
            let hint_area = Rect {
                x: footer_area.x + 2,
                y: footer_area.y,
                width: footer_area.width.saturating_sub(2),
                height: footer_area.height,
            };
            hint.dim().render(hint_area, buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_event::AppEvent;
    use crate::bottom_pane::popup_consts::standard_popup_hint_line;
    use insta::assert_snapshot;
    use ratatui::layout::Rect;
    use tokio::sync::mpsc::unbounded_channel;

    fn make_selection_view(subtitle: Option<&str>) -> ListSelectionView {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let items = vec![
            SelectionItem {
                name: "Read Only".to_string(),
                description: Some("Nori can read files".to_string()),
                is_current: true,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Full Access".to_string(),
                description: Some("Nori can edit files".to_string()),
                is_current: false,
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        ListSelectionView::new(
            SelectionViewParams {
                title: Some("Select Approval Mode".to_string()),
                subtitle: subtitle.map(str::to_string),
                footer_hint: Some(standard_popup_hint_line()),
                items,
                ..Default::default()
            },
            tx,
        )
    }

    fn render_lines(view: &ListSelectionView) -> String {
        render_lines_with_width(view, 48)
    }

    fn render_lines_with_width(view: &ListSelectionView, width: u16) -> String {
        let height = view.desired_height(width);
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        view.render(area, &mut buf);

        let lines: Vec<String> = (0..area.height)
            .map(|row| {
                let mut line = String::new();
                for col in 0..area.width {
                    let symbol = buf[(area.x + col, area.y + row)].symbol();
                    if symbol.is_empty() {
                        line.push(' ');
                    } else {
                        line.push_str(symbol);
                    }
                }
                line
            })
            .collect();
        lines.join("\n")
    }

    #[test]
    fn renders_blank_line_between_title_and_items_without_subtitle() {
        let view = make_selection_view(None);
        assert_snapshot!(
            "list_selection_spacing_without_subtitle",
            render_lines(&view)
        );
    }

    #[test]
    fn renders_blank_line_between_subtitle_and_items() {
        let view = make_selection_view(Some("Switch between Nori approval presets"));
        assert_snapshot!("list_selection_spacing_with_subtitle", render_lines(&view));
    }

    #[test]
    fn renders_search_query_line_when_enabled() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let items = vec![SelectionItem {
            name: "Read Only".to_string(),
            description: Some("Nori can read files".to_string()),
            is_current: false,
            dismiss_on_select: true,
            ..Default::default()
        }];
        let mut view = ListSelectionView::new(
            SelectionViewParams {
                title: Some("Select Approval Mode".to_string()),
                footer_hint: Some(standard_popup_hint_line()),
                items,
                is_searchable: true,
                search_placeholder: Some("Type to search branches".to_string()),
                ..Default::default()
            },
            tx,
        );
        view.set_search_query("filters".to_string());

        let lines = render_lines(&view);
        assert!(
            lines.contains("filters"),
            "expected search query line to include rendered query, got {lines:?}"
        );
    }

    #[test]
    fn update_item_refreshes_row_and_search_index() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let mut view = ListSelectionView::new(
            SelectionViewParams {
                items: vec![SelectionItem {
                    name: "Apr 27, 2026 15:44".to_string(),
                    search_value: Some("session-1".to_string()),
                    ..Default::default()
                }],
                is_searchable: true,
                ..Default::default()
            },
            tx,
        );

        let updated = view.update_item(
            "session-1",
            "Apr 27, 2026 15:44 · 2 turns".to_string(),
            Some("\"first prompt\"".to_string()),
            "session-1 first prompt".to_string(),
        );

        assert!(updated);
        assert_eq!(view.items[0].name, "Apr 27, 2026 15:44 · 2 turns");
        assert_eq!(
            view.items[0].description.as_deref(),
            Some("\"first prompt\"")
        );

        view.set_search_query("first prompt".to_string());
        assert_eq!(view.filtered_indices, vec![0]);
    }

    #[test]
    fn width_changes_do_not_hide_rows() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let items = vec![
            SelectionItem {
                name: "gpt-5.1-codex".to_string(),
                description: Some(
                    "Optimized for Nori. Balance of reasoning quality and coding ability."
                        .to_string(),
                ),
                is_current: true,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "gpt-5.1-codex-mini".to_string(),
                description: Some(
                    "Optimized for Nori. Cheaper, faster, but less capable.".to_string(),
                ),
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "gpt-4.1-codex".to_string(),
                description: Some(
                    "Legacy model. Use when you need compatibility with older automations."
                        .to_string(),
                ),
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        let view = ListSelectionView::new(
            SelectionViewParams {
                title: Some("Select Model and Effort".to_string()),
                items,
                ..Default::default()
            },
            tx,
        );
        let mut missing: Vec<u16> = Vec::new();
        for width in 60..=90 {
            let rendered = render_lines_with_width(&view, width);
            if !rendered.contains("3.") {
                missing.push(width);
            }
        }
        assert!(
            missing.is_empty(),
            "third option missing at widths {missing:?}"
        );
    }

    #[test]
    fn on_dismiss_callback_fires_on_ctrl_c() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let dismissed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dismissed_clone = dismissed.clone();
        let items = vec![SelectionItem {
            name: "Option A".to_string(),
            dismiss_on_select: true,
            ..Default::default()
        }];
        let mut view = ListSelectionView::new(
            SelectionViewParams {
                title: Some("Test".to_string()),
                items,
                on_dismiss: Some(Box::new(move |_tx| {
                    dismissed_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                })),
                ..Default::default()
            },
            tx,
        );

        // Dismiss via Ctrl-C
        view.on_ctrl_c();

        assert!(
            dismissed.load(std::sync::atomic::Ordering::SeqCst),
            "on_dismiss callback should fire when picker is dismissed via Ctrl-C"
        );
        assert!(view.is_complete(), "view should be complete after dismiss");
    }

    #[test]
    fn on_dismiss_callback_not_fired_on_accept() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let dismissed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let dismissed_clone = dismissed.clone();
        let items = vec![SelectionItem {
            name: "Option A".to_string(),
            dismiss_on_select: true,
            ..Default::default()
        }];
        let mut view = ListSelectionView::new(
            SelectionViewParams {
                title: Some("Test".to_string()),
                items,
                on_dismiss: Some(Box::new(move |_tx| {
                    dismissed_clone.store(true, std::sync::atomic::Ordering::SeqCst);
                })),
                ..Default::default()
            },
            tx,
        );

        // Accept via Enter (should NOT fire on_dismiss)
        view.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(
            !dismissed.load(std::sync::atomic::Ordering::SeqCst),
            "on_dismiss callback should NOT fire when an item is selected"
        );
    }

    #[test]
    fn narrow_width_keeps_all_rows_visible() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let desc = "x".repeat(10);
        let items: Vec<SelectionItem> = (1..=3)
            .map(|idx| SelectionItem {
                name: format!("Item {idx}"),
                description: Some(desc.clone()),
                dismiss_on_select: true,
                ..Default::default()
            })
            .collect();
        let view = ListSelectionView::new(
            SelectionViewParams {
                title: Some("Debug".to_string()),
                items,
                ..Default::default()
            },
            tx,
        );
        let rendered = render_lines_with_width(&view, 24);
        assert!(
            rendered.contains("3."),
            "third option missing for width 24:\n{rendered}"
        );
    }

    #[test]
    fn snapshot_model_picker_width_80() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let items = vec![
            SelectionItem {
                name: "gpt-5.1-codex".to_string(),
                description: Some(
                    "Optimized for Nori. Balance of reasoning quality and coding ability."
                        .to_string(),
                ),
                is_current: true,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "gpt-5.1-codex-mini".to_string(),
                description: Some(
                    "Optimized for Nori. Cheaper, faster, but less capable.".to_string(),
                ),
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "gpt-4.1-codex".to_string(),
                description: Some(
                    "Legacy model. Use when you need compatibility with older automations."
                        .to_string(),
                ),
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        let view = ListSelectionView::new(
            SelectionViewParams {
                title: Some("Select Model and Effort".to_string()),
                items,
                ..Default::default()
            },
            tx,
        );
        assert_snapshot!(
            "list_selection_model_picker_width_80",
            render_lines_with_width(&view, 80)
        );
    }

    #[test]
    fn snapshot_narrow_width_preserves_third_option() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let desc = "x".repeat(10);
        let items: Vec<SelectionItem> = (1..=3)
            .map(|idx| SelectionItem {
                name: format!("Item {idx}"),
                description: Some(desc.clone()),
                dismiss_on_select: true,
                ..Default::default()
            })
            .collect();
        let view = ListSelectionView::new(
            SelectionViewParams {
                title: Some("Debug".to_string()),
                items,
                ..Default::default()
            },
            tx,
        );
        assert_snapshot!(
            "list_selection_narrow_width_preserves_rows",
            render_lines_with_width(&view, 24)
        );
    }

    #[test]
    fn test_jk_navigation_when_not_searchable() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let items = vec![
            SelectionItem {
                name: "Item 1".to_string(),
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Item 2".to_string(),
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Item 3".to_string(),
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        let mut view = ListSelectionView::new(
            SelectionViewParams {
                title: Some("Test".to_string()),
                items,
                is_searchable: false, // explicitly false
                ..Default::default()
            },
            tx,
        );

        // Initial selection should be at index 0
        assert_eq!(view.state.selected_idx, Some(0));

        // Press 'j' to move down
        view.handle_key_event(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(
            view.state.selected_idx,
            Some(1),
            "j should move selection down"
        );

        // Press 'j' again to move to index 2
        view.handle_key_event(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(
            view.state.selected_idx,
            Some(2),
            "j should move selection down again"
        );

        // Press 'k' to move up
        view.handle_key_event(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(
            view.state.selected_idx,
            Some(1),
            "k should move selection up"
        );

        // Press 'k' again to go back to index 0
        view.handle_key_event(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(
            view.state.selected_idx,
            Some(0),
            "k should move selection up again"
        );

        // Press 'k' at index 0 should wrap to last item
        view.handle_key_event(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(
            view.state.selected_idx,
            Some(2),
            "k at first item should wrap to last"
        );
    }

    #[test]
    fn test_jk_goes_to_search_when_searchable() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let items = vec![
            SelectionItem {
                name: "Item 1".to_string(),
                search_value: Some("junk".to_string()), // contains 'j' so it won't be filtered
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Item 2".to_string(),
                search_value: Some("kite".to_string()), // contains 'k' so it won't be filtered
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        let mut view = ListSelectionView::new(
            SelectionViewParams {
                title: Some("Test".to_string()),
                items,
                is_searchable: true, // searchable mode
                ..Default::default()
            },
            tx,
        );

        // Initial selection should be at index 0
        assert_eq!(view.state.selected_idx, Some(0));

        // Press 'j' - should go to search query, not navigate
        view.handle_key_event(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(view.search_query, "j", "j should be added to search query");
        // After filtering for 'j', only "junk" matches, so selection stays at 0
        assert_eq!(
            view.state.selected_idx,
            Some(0),
            "selection should stay at first match"
        );

        // Clear search and try 'k'
        view.search_query.clear();
        view.handle_key_event(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(view.search_query, "k", "k should be added to search query");
    }

    /// Helper to build a searchable list with vim_mode set.
    fn make_vim_searchable_view() -> ListSelectionView {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let items = vec![
            SelectionItem {
                name: "alpha".to_string(),
                search_value: Some("alpha".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "beta".to_string(),
                search_value: Some("beta".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "gamma".to_string(),
                search_value: Some("gamma".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        ListSelectionView::new(
            SelectionViewParams {
                title: Some("Test".to_string()),
                items,
                is_searchable: true,
                vim_mode: true,
                ..Default::default()
            },
            tx,
        )
    }

    /// Helper to build a searchable list WITHOUT vim_mode.
    fn make_nonvim_searchable_view() -> ListSelectionView {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let items = vec![
            SelectionItem {
                name: "alpha".to_string(),
                search_value: Some("alpha".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "beta".to_string(),
                search_value: Some("beta".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "gamma".to_string(),
                search_value: Some("gamma".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        ListSelectionView::new(
            SelectionViewParams {
                title: Some("Test".to_string()),
                items,
                is_searchable: true,
                vim_mode: false,
                ..Default::default()
            },
            tx,
        )
    }

    #[test]
    fn vim_searchable_jk_navigates_not_searches() {
        let mut view = make_vim_searchable_view();
        assert_eq!(view.state.selected_idx, Some(0));

        // 'j' should move down, NOT add 'j' to search query
        view.handle_key_event(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(
            view.state.selected_idx,
            Some(1),
            "j should move selection down"
        );
        assert!(
            view.search_query.is_empty(),
            "j should not go to search query"
        );

        // 'k' should move up, NOT add 'k' to search query
        view.handle_key_event(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
        assert_eq!(
            view.state.selected_idx,
            Some(0),
            "k should move selection up"
        );
        assert!(
            view.search_query.is_empty(),
            "k should not go to search query"
        );
    }

    #[test]
    fn vim_searchable_slash_activates_search_then_chars_filter() {
        let mut view = make_vim_searchable_view();

        // Press '/' to activate search
        view.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(view.search_active, "/ should activate search mode");

        // Now typing 'a' should filter
        view.handle_key_event(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(
            view.search_query, "a",
            "char should append to search query in search mode"
        );
        // 'a' matches 'alpha', 'beta', 'gamma' — all contain 'a'
        assert_eq!(view.filtered_indices.len(), 3);

        // Type 'l' to narrow further — 'al' matches 'alpha' only
        view.handle_key_event(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
        assert_eq!(view.search_query, "al");
        assert_eq!(view.filtered_indices.len(), 1);
    }

    #[test]
    fn vim_searchable_esc_in_search_exits_search_not_dismiss() {
        let mut view = make_vim_searchable_view();

        // Activate search and type something
        view.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        view.handle_key_event(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(view.search_query, "b");
        assert!(view.search_active);

        // Esc should exit search mode but NOT dismiss the popup
        view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!view.search_active, "Esc should deactivate search mode");
        assert!(
            view.search_query.is_empty(),
            "Esc should clear search query"
        );
        assert!(
            !view.is_complete(),
            "Esc in search mode should NOT dismiss the popup"
        );
        // All items should be visible again
        assert_eq!(view.filtered_indices.len(), 3);
    }

    #[test]
    fn vim_searchable_esc_outside_search_dismisses() {
        let mut view = make_vim_searchable_view();

        // Esc when NOT in search mode should dismiss
        view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(
            view.is_complete(),
            "Esc outside search mode should dismiss the popup"
        );
    }

    #[test]
    fn vim_searchable_digits_direct_select_when_not_searching() {
        let mut view = make_vim_searchable_view();

        // Press '2' — should select item at index 1 and accept
        view.handle_key_event(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE));
        assert!(view.is_complete(), "digit should accept selection");
        assert_eq!(view.last_selected_actual_idx, Some(1));
    }

    #[test]
    fn vim_searchable_backspace_in_search_pops_char() {
        let mut view = make_vim_searchable_view();

        // Activate search and type
        view.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        view.handle_key_event(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
        view.handle_key_event(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        assert_eq!(view.search_query, "be");

        // Backspace should remove last char
        view.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(view.search_query, "b");
    }

    #[test]
    fn nonvim_searchable_chars_go_to_search_immediately() {
        let mut view = make_nonvim_searchable_view();

        // Typing 'j' should go to search query, not navigate
        view.handle_key_event(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(
            view.search_query, "j",
            "chars should go to search in non-vim mode"
        );
    }

    #[test]
    fn nonvim_searchable_esc_dismisses() {
        let mut view = make_nonvim_searchable_view();

        // Esc should dismiss immediately (no search_active state)
        view.handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(view.is_complete(), "Esc should dismiss in non-vim mode");
    }

    #[test]
    fn narrow_terminal_no_single_char_description_lines() {
        // Reproduce the bug: on a narrow terminal, descriptions that are pushed
        // to a high desc_col wrap with huge indent, producing one-char-per-line.
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let items = vec![
            SelectionItem {
                name: "Vertical Footer (on)".to_string(),
                description: Some(
                    "Stack footer segments vertically instead of horizontally".to_string(),
                ),
                is_current: true,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Terminal Notifications (on)".to_string(),
                description: Some("Send OSC 9 escape sequences to notify the terminal".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        let view = ListSelectionView::new(
            SelectionViewParams {
                title: Some("Configuration".to_string()),
                items,
                ..Default::default()
            },
            tx,
        );
        let rendered = render_lines_with_width(&view, 30);

        // The rendered output should NOT have lines that are just whitespace + a
        // single visible character. That pattern is the telltale sign of the
        // one-char-per-line wrapping bug.
        for (line_num, line) in rendered.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.len() == 1 && trimmed != "›" {
                panic!(
                    "line {} has single-char content '{trimmed}', \
                     indicating broken description wrapping:\n{rendered}",
                    line_num + 1
                );
            }
        }
    }

    #[test]
    fn snapshot_very_narrow_config_popup() {
        let (tx_raw, _rx) = unbounded_channel::<AppEvent>();
        let tx = AppEventSender::new(tx_raw);
        let items = vec![
            SelectionItem {
                name: "Vertical Footer (on)".to_string(),
                description: Some(
                    "Stack footer segments vertically instead of horizontally".to_string(),
                ),
                is_current: true,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "OS Notifications (off)".to_string(),
                description: Some("Send native desktop notifications on events".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        let view = ListSelectionView::new(
            SelectionViewParams {
                title: Some("Configuration".to_string()),
                items,
                ..Default::default()
            },
            tx,
        );
        assert_snapshot!(
            "list_selection_very_narrow_config",
            render_lines_with_width(&view, 30)
        );
    }
}
