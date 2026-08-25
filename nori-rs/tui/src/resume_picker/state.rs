use super::*;

impl PickerState {
    pub(super) fn new(
        nori_home: PathBuf,
        requester: FrameRequester,
        agent_filter: Option<String>,
        show_all: bool,
        filter_cwd: Option<PathBuf>,
    ) -> Self {
        Self {
            nori_home,
            requester,
            num_scanned_files: 0,
            all_rows: Vec::new(),
            filtered_rows: Vec::new(),
            seen_paths: HashSet::new(),
            selected: 0,
            scroll_top: 0,
            search_active: false,
            query: String::new(),
            view_rows: None,
            agent_filter,
            show_all,
            filter_cwd,
        }
    }

    pub(super) fn request_frame(&self) {
        self.requester.schedule_frame();
    }

    pub(super) async fn handle_key(&mut self, key: KeyEvent) -> Result<Option<ResumeSelection>> {
        let has_control = key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL);
        let has_alt = key
            .modifiers
            .contains(crossterm::event::KeyModifiers::ALT);

        match key.code {
            KeyCode::Esc if self.search_active => {
                self.search_active = false;
                self.set_query(String::new());
                self.request_frame();
            }
            KeyCode::Esc => return Ok(Some(ResumeSelection::StartFresh)),
            KeyCode::Char('c') if has_control => {
                return Ok(Some(ResumeSelection::Exit));
            }
            KeyCode::Enter => {
                if let Some(row) = self.filtered_rows.get(self.selected) {
                    return Ok(Some(ResumeSelection::Resume(row.target.clone())));
                }
            }
            KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.ensure_selected_visible();
                }
                self.request_frame();
            }
            KeyCode::Down => {
                if self.selected + 1 < self.filtered_rows.len() {
                    self.selected += 1;
                    self.ensure_selected_visible();
                }
                self.request_frame();
            }
            KeyCode::PageUp => {
                let step = self.view_rows.unwrap_or(10).max(1);
                if self.selected > 0 {
                    self.selected = self.selected.saturating_sub(step);
                    self.ensure_selected_visible();
                    self.request_frame();
                }
            }
            KeyCode::PageDown => {
                if !self.filtered_rows.is_empty() {
                    let step = self.view_rows.unwrap_or(10).max(1);
                    let max_index = self.filtered_rows.len().saturating_sub(1);
                    self.selected = (self.selected + step).min(max_index);
                    self.ensure_selected_visible();
                    self.request_frame();
                }
            }
            KeyCode::Backspace if self.search_active => {
                let mut new_query = self.query.clone();
                new_query.pop();
                self.set_query(new_query);
            }
            KeyCode::Char('f')
                if !self.search_active
                    && key.modifiers == crossterm::event::KeyModifiers::CONTROL =>
            {
                self.search_active = true;
                self.request_frame();
            }
            KeyCode::Char('f' | '/') if !self.search_active && !has_control && !has_alt => {
                self.search_active = true;
                self.request_frame();
            }
            KeyCode::Char('k') if !self.search_active && !has_control && !has_alt => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.ensure_selected_visible();
                }
                self.request_frame();
            }
            KeyCode::Char('j') if !self.search_active && !has_control && !has_alt => {
                if self.selected + 1 < self.filtered_rows.len() {
                    self.selected += 1;
                    self.ensure_selected_visible();
                }
                self.request_frame();
            }
            KeyCode::Char(c) if self.search_active && !has_control && !has_alt => {
                let mut new_query = self.query.clone();
                new_query.push(c);
                self.set_query(new_query);
            }
            _ => {}
        }
        Ok(None)
    }

    pub(super) async fn load_initial_page(&mut self) -> Result<()> {
        let page = load_transcript_page(
            &self.nori_home,
            self.filter_cwd.as_deref(),
            self.agent_filter.as_deref(),
        )
        .await?;
        self.num_scanned_files = 0;
        self.all_rows.clear();
        self.filtered_rows.clear();
        self.seen_paths.clear();
        self.selected = 0;
        self.ingest_page(page);
        Ok(())
    }

    pub(super) fn ingest_page(&mut self, page: TranscriptPage) {
        self.num_scanned_files = self
            .num_scanned_files
            .saturating_add(page.num_scanned_files);

        let rows = helpers::rows_from_items(page.items, self.nori_home.clone());
        for row in rows {
            let path = self
                .nori_home
                .join("transcripts")
                .join("by-project")
                .join(&row.target.project_id)
                .join("sessions")
                .join(format!("{}.jsonl", row.target.session_id));
            if self.seen_paths.insert(path) {
                self.all_rows.push(row);
            }
        }

        self.apply_filter();
    }

    pub(super) fn apply_filter(&mut self) {
        let base_iter = self
            .all_rows
            .iter()
            .filter(|row| self.row_matches_filter(row));
        if self.query.is_empty() {
            self.filtered_rows = base_iter.cloned().collect();
        } else {
            let q = self.query.to_lowercase();
            self.filtered_rows = base_iter
                .filter(|r| r.preview.to_lowercase().contains(&q))
                .cloned()
                .collect();
        }
        if self.selected >= self.filtered_rows.len() {
            self.selected = self.filtered_rows.len().saturating_sub(1);
        }
        if self.filtered_rows.is_empty() {
            self.scroll_top = 0;
        }
        self.ensure_selected_visible();
        self.request_frame();
    }

    pub(super) fn row_matches_filter(&self, row: &Row) -> bool {
        if self.show_all {
            return true;
        }
        let Some(filter_cwd) = self.filter_cwd.as_ref() else {
            return true;
        };
        let Some(row_cwd) = row.cwd.as_ref() else {
            return false;
        };
        helpers::paths_match(row_cwd, filter_cwd)
    }

    pub(super) fn set_query(&mut self, new_query: String) {
        if self.query == new_query {
            return;
        }
        self.query = new_query;
        self.selected = 0;
        self.apply_filter();
    }

    pub(super) fn ensure_selected_visible(&mut self) {
        if self.filtered_rows.is_empty() {
            self.scroll_top = 0;
            return;
        }
        let capacity = self.view_rows.unwrap_or(self.filtered_rows.len()).max(1);

        if self.selected < self.scroll_top {
            self.scroll_top = self.selected;
        } else {
            let last_visible = self.scroll_top.saturating_add(capacity - 1);
            if self.selected > last_visible {
                self.scroll_top = self.selected.saturating_sub(capacity - 1);
            }
        }

        let max_start = self.filtered_rows.len().saturating_sub(capacity);
        if self.scroll_top > max_start {
            self.scroll_top = max_start;
        }
    }

    pub(super) fn update_view_rows(&mut self, rows: usize) {
        self.view_rows = if rows == 0 { None } else { Some(rows) };
        self.ensure_selected_visible();
    }
}
