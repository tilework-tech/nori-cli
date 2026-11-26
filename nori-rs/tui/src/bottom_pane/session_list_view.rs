//! Interactive session list in navigation order, using the shared two-row layout.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use nori_protocol::SessionStatus;
use nori_tui_components::DetailTone;
use nori_tui_components::KeyHint;
use nori_tui_components::Picker;
use nori_tui_components::PickerAction;
use nori_tui_components::PickerDensity;
use nori_tui_components::PickerItem;
use nori_tui_components::PickerOutcome;
use nori_tui_components::PickerState;
use nori_tui_components::ProviderKind;
use nori_tui_components::session_table;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Widget;

use super::BottomPaneView;
use super::CancellationEvent;
use crate::app_event::AppEvent;
use crate::app_event::SessionGeneration;
use crate::app_event_sender::AppEventSender;
use crate::render::renderable::Renderable;
use crate::session_status::WAITING_FILTER_LABEL;
use crate::session_status::status_summary;

/// One session as the list shows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SessionRow {
    pub(crate) generation: SessionGeneration,
    pub(crate) title: String,
    pub(crate) agent: String,
    pub(crate) provider: Option<ProviderKind>,
    /// The session's own status, which decides what it can do (resume,
    /// close).
    pub(crate) status: SessionStatus,
    /// The word shown in the status column. For a parent this follows its
    /// whole family.
    pub(crate) status_word: String,
    /// The status shown by the glyph and the status tone. For a parent this
    /// follows its whole family.
    pub(crate) family_status: SessionStatus,
    /// The start of the agent's latest reply.
    pub(crate) reply: Option<String>,
    /// The model the agent runs, as the status bar names it.
    pub(crate) model: Option<String>,
    /// Seconds since the session last did anything.
    pub(crate) idle_seconds: Option<i64>,
    /// Context use in the status bar's short form.
    pub(crate) context: Option<String>,
    pub(crate) depth: i32,
    pub(crate) on_screen: bool,
}

pub(crate) struct SessionListView {
    state: PickerState<SessionGeneration>,
    rows: Vec<SessionRow>,
    app_event_tx: AppEventSender,
    complete: bool,
    /// A busy session the user pressed the close key on once. Closing it
    /// loses its work in progress, so it takes a second press.
    close_armed: Option<SessionGeneration>,
}

const TITLE: &str = session_table::SESSION;
const AGENT: &str = session_table::AGENT;
const STATUS: &str = session_table::STATUS;
const CONTEXT: &str = session_table::CONTEXT;

impl SessionListView {
    pub(crate) fn new(rows: Vec<SessionRow>, app_event_tx: AppEventSender) -> Self {
        let columns = session_table::session_columns();
        let mut state = PickerState::new("Sessions", columns, items(&rows))
            .categories([WAITING_FILTER_LABEL])
            .search_placeholder("Title or agent");
        if let Some(index) = rows.iter().position(|row| row.on_screen) {
            state.selected_index = Some(index);
        }
        Self {
            state,
            rows,
            app_event_tx,
            complete: false,
            close_armed: None,
        }
    }

    /// Replace the rows, keeping the selection on the same session.
    /// Show `rows`. Returns whether they differ from the rows shown.
    pub(crate) fn replace_rows(&mut self, rows: Vec<SessionRow>) -> bool {
        if rows == self.rows {
            return false;
        }
        self.state.replace_items(items(&rows));
        self.rows = rows;
        true
    }

    fn header(&self) -> Line<'static> {
        let count = self.rows.len();
        let mut text = format!("Sessions {count}");
        if let Some(summary) = status_summary(self.rows.iter().map(|row| row.status)) {
            text.push_str(" · ");
            text.push_str(&summary);
        }
        let hint = if self.state.active_category.is_some() {
            "  tab: all sessions · / search"
        } else {
            "  tab: waiting only · / search"
        };
        Line::from(vec![text.bold(), hint.dim()])
    }

    fn selected_row(&self) -> Option<&SessionRow> {
        let generation = self.state.selected_item()?.key;
        self.rows.iter().find(|row| row.generation == generation)
    }

    /// Close the selected session, or drop it from the list once closed. A
    /// busy session needs the key twice.
    fn close_selected(&mut self) {
        let Some(row) = self.selected_row() else {
            // With nothing to close, Ctrl+D leaves as it does from the composer.
            if self.rows.is_empty() {
                self.app_event_tx.send(AppEvent::BeginExit);
            }
            return;
        };
        let generation = row.generation;
        if generation < 0 {
            return;
        }
        match row.status {
            SessionStatus::Closed | SessionStatus::Done => {
                self.app_event_tx
                    .send(AppEvent::RemoveSession { generation });
            }
            SessionStatus::Working | SessionStatus::WaitingOnYou | SessionStatus::Starting
                if self.close_armed != Some(generation) =>
            {
                self.close_armed = Some(generation);
            }
            SessionStatus::Working
            | SessionStatus::WaitingOnYou
            | SessionStatus::Starting
            | SessionStatus::Ready
            | SessionStatus::Failed => {
                self.close_armed = None;
                self.app_event_tx
                    .send(AppEvent::CloseSession { generation });
            }
        }
    }

    fn close_hint(&self) -> Option<KeyHint<'static>> {
        let row = self.selected_row().filter(|row| row.generation >= 0)?;
        Some(match row.status {
            SessionStatus::Closed | SessionStatus::Done => KeyHint::new("ctrl+d", "remove"),
            _ if self.close_armed == Some(row.generation) => {
                KeyHint::new("ctrl+d", "again to close")
            }
            SessionStatus::Working
            | SessionStatus::WaitingOnYou
            | SessionStatus::Starting
            | SessionStatus::Ready
            | SessionStatus::Failed => KeyHint::new("ctrl+d", "close"),
        })
    }

    fn apply(&mut self, action: PickerAction) {
        match self.state.handle(action) {
            PickerOutcome::Selected(generation) => {
                self.app_event_tx.send(AppEvent::ShowSession { generation });
                self.complete = true;
            }
            PickerOutcome::Cancelled => self.complete = true,
            PickerOutcome::Unchanged
            | PickerOutcome::SelectionChanged(_)
            | PickerOutcome::Toggled { .. }
            | PickerOutcome::Submitted(_)
            | PickerOutcome::SearchModeChanged(_)
            | PickerOutcome::QueryChanged(_)
            | PickerOutcome::CategoryChanged(_) => {}
        }
    }
}

fn items(rows: &[SessionRow]) -> Vec<PickerItem<SessionGeneration>> {
    rows.iter()
        .map(|row| {
            let mut item = PickerItem::new(
                row.generation,
                TITLE,
                format!(
                    "{} {}",
                    crate::session_status::status_glyph(row.family_status),
                    row.title
                ),
            )
            .cell(AGENT, row.agent.clone())
            .cell(STATUS, row.status_word.clone())
            .cell_detail_tone(
                STATUS,
                crate::session_status::status_tone(row.family_status),
            )
            .search_text(format!("{} {}", row.title, row.agent))
            .depth(row.depth)
            .current(row.on_screen);
            if let Some(provider) = row.provider {
                item = item.cell_tone(AGENT, provider);
            }
            if row.status.is_closed() {
                item = item.cell_detail_tone(TITLE, DetailTone::Muted);
            }
            if row.family_status == SessionStatus::WaitingOnYou {
                item = item.category(WAITING_FILTER_LABEL);
            }
            if let Some(context) = &row.context {
                item = item.cell(CONTEXT, context.clone());
            }
            if let Some(model) = &row.model {
                item = item.second_row_cell(AGENT, model.clone());
            }
            if let Some(idle) = row.idle_seconds {
                item = item.second_row_cell(STATUS, session_table::ago(idle));
            }
            // The reply lines up with the title, after the status glyph.
            let reply = row.reply.as_deref().unwrap_or(session_table::NO_REPLY);
            item.second_row_cell(TITLE, format!("  {reply}"))
        })
        .collect()
}

impl BottomPaneView for SessionListView {
    fn handle_key_event(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        if key.code == KeyCode::Char('d') && key.modifiers == KeyModifiers::CONTROL {
            self.close_selected();
            return;
        }
        self.close_armed = None;
        if key.code == KeyCode::Char('r') && key.modifiers == KeyModifiers::CONTROL {
            if let Some(row) = self.selected_row()
                && row.status == SessionStatus::Failed
            {
                self.app_event_tx.send(AppEvent::ResumeFailedSession {
                    generation: row.generation,
                });
                self.complete = true;
            }
            return;
        }
        let action = match (key.code, key.modifiers) {
            (KeyCode::Char('p'), KeyModifiers::CONTROL) => PickerAction::MoveUp,
            (KeyCode::Char('n'), KeyModifiers::CONTROL) => PickerAction::MoveDown,
            (KeyCode::Tab, _) => PickerAction::NextCategory,
            (KeyCode::BackTab, _) => PickerAction::PreviousCategory,
            _ => match super::picker_keys::navigation_action(&key, self.state.search_active) {
                Some(action) => action,
                None => return,
            },
        };
        self.apply(action);
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }

    fn on_escape(&mut self) -> CancellationEvent {
        if self.state.search_active {
            self.apply(PickerAction::DeactivateSearch);
        } else {
            self.complete = true;
        }
        CancellationEvent::Handled
    }

    fn is_session_list(&self) -> bool {
        true
    }

    fn update_session_list(&mut self, rows: &[SessionRow]) -> bool {
        self.replace_rows(rows.to_vec())
    }
}

impl Renderable for SessionListView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        // The header starts at the list's left edge, with the tabs, the
        // row marker and the footer below it.
        let header_area = Rect::new(area.x, area.y, area.width, 1);
        self.header().render(header_area, buf);
        let list_area = Rect::new(
            area.x,
            area.y.saturating_add(1),
            area.width,
            area.height.saturating_sub(1),
        );
        let picker = Picker::new(&self.state)
            .theme(crate::style::component_theme())
            .density(PickerDensity::Compact)
            .horizontal_margin(0)
            .selection_keeps_tones(true)
            .show_title(false)
            .show_details(false)
            .footer_hints(
                [
                    Some(KeyHint::new("↑↓/j/k", "move")),
                    Some(KeyHint::new("enter", "open")),
                    self.selected_row()
                        .filter(|row| row.status == SessionStatus::Failed)
                        .map(|_| KeyHint::new("ctrl+r", "resume")),
                    self.close_hint(),
                    Some(if self.state.active_category.is_some() {
                        KeyHint::new("tab", "all sessions")
                    } else {
                        KeyHint::new("tab", WAITING_FILTER_LABEL)
                    }),
                    Some(KeyHint::new("esc", "close")),
                ]
                .into_iter()
                .flatten(),
            );
        let picker = if self.state.active_category.is_some() && self.state.query.is_empty() {
            picker.empty_state(
                "No sessions waiting on you",
                "Press tab to show all sessions.",
            )
        } else {
            picker
        };
        picker.render(list_area, buf);
    }

    fn desired_height(&self, _width: u16) -> u16 {
        let rows = self.state.visible_indices().len().clamp(1, 8) as u16;
        let row_height = 2;
        // Header, category tabs and their gap, column headings, and the key
        // hints with their gap.
        let chrome = 1 + 2 + 1 + 1 + 2 + 1 + u16::from(self.state.search_active);
        rows.saturating_mul(row_height)
            .saturating_add(chrome)
            .min(24)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_snapshot;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use tokio::sync::mpsc::unbounded_channel;

    fn row(generation: SessionGeneration, title: &str, status: SessionStatus) -> SessionRow {
        SessionRow {
            generation,
            title: title.to_string(),
            agent: "Mock ACP".to_string(),
            provider: None,
            status,
            status_word: crate::session_status::status_label(status).to_string(),
            family_status: status,
            reply: None,
            model: None,
            idle_seconds: None,
            context: None,
            depth: 0,
            on_screen: false,
        }
    }

    fn rich_rows() -> Vec<SessionRow> {
        let mut parent = row(1, "Greeting in three words", SessionStatus::Working);
        parent.agent = "claude-code".to_string();
        parent.provider = Some(ProviderKind::Claude);
        parent.family_status = SessionStatus::Working;
        parent.status_word = "working".to_string();
        parent.reply = Some(
            "Hi there, friend! Anything else you would like me to help with today?".to_string(),
        );
        parent.model = Some("opus 5.5".to_string());
        parent.idle_seconds = Some(125);
        parent.context = Some("8% / 475k".to_string());
        parent.on_screen = true;
        let mut child = row(
            2,
            "inspect the parser recovery path",
            SessionStatus::Working,
        );
        child.agent = "codex".to_string();
        child.provider = Some(ProviderKind::Codex);
        child.reply = Some("Reading src/parser.rs".to_string());
        child.model = Some("gpt-5.5".to_string());
        child.idle_seconds = Some(4);
        child.context = Some("31% / 272k".to_string());
        child.depth = 1;
        let mut fresh = row(3, "say hi in 3 words", SessionStatus::Starting);
        fresh.agent = "gemini".to_string();
        fresh.idle_seconds = Some(0);
        vec![parent, child, fresh]
    }

    fn view(rows: Vec<SessionRow>) -> SessionListView {
        let (tx, _rx) = unbounded_channel();
        SessionListView::new(rows, AppEventSender::new(tx))
    }

    fn render(view: &SessionListView, width: u16) -> String {
        let height = view.desired_height(width);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| view.render(frame.area(), frame.buffer_mut()))
            .expect("draw");
        terminal.backend().to_string()
    }

    #[test]
    fn the_list_asks_for_a_redraw_only_when_its_rows_change() {
        let mut view = view(rich_rows());
        assert!(
            !view.update_session_list(&rich_rows()),
            "unchanged rows need no redraw"
        );
        let mut changed = rich_rows();
        changed[2].status = SessionStatus::Ready;
        assert!(view.update_session_list(&changed), "a changed row redraws");
    }

    #[test]
    fn ctrl_d_in_an_empty_list_leaves() {
        let (tx, mut rx) = unbounded_channel();
        let mut view = SessionListView::new(Vec::new(), AppEventSender::new(tx));
        view.handle_key_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(matches!(rx.try_recv(), Ok(AppEvent::BeginExit)));
    }

    #[test]
    fn a_parent_shows_the_glyph_of_its_family_status() {
        let mut parent = row(1, "parent session", SessionStatus::Ready);
        parent.family_status = SessionStatus::WaitingOnYou;
        parent.status_word = "waiting on you".to_string();
        let mut child = row(2, "child session", SessionStatus::WaitingOnYou);
        child.depth = 1;
        let screen = render(&view(vec![parent, child]), 80);
        let parent_line = screen
            .lines()
            .find(|line| line.contains("parent session"))
            .expect("parent row");
        assert!(parent_line.contains("● parent session"), "{screen}");
    }

    #[test]
    fn titles_sharing_a_long_prefix_stay_distinct_with_one_cut() {
        let mut parent = row(
            1,
            "shared prefix for the parser review: parent side",
            SessionStatus::Ready,
        );
        parent.on_screen = true;
        let mut child = row(
            2,
            "shared prefix for the parser review: child side mock:request-permission:z",
            SessionStatus::WaitingOnYou,
        );
        child.depth = 1;
        for width in [120, 80] {
            let screen = render(&view(vec![parent.clone(), child.clone()]), width);
            let parent_line = screen
                .lines()
                .find(|line| line.contains("parent side"))
                .unwrap_or_else(|| panic!("parent end is shown at {width}: {screen}"));
            let child_line = screen
                .lines()
                .find(|line| line.contains("permission:z"))
                .unwrap_or_else(|| panic!("child end is shown at {width}: {screen}"));
            for line in [parent_line, child_line] {
                assert!(line.contains("shared"), "start kept at {width}: {line}");
            }
            assert_eq!(
                child_line.matches('…').count(),
                1,
                "the long child name is cut once at {width}: {child_line}"
            );
            assert!(parent_line.matches('…').count() <= 1, "{parent_line}");
            assert!(child_line.contains("└─"), "{child_line}");
            assert!(parent_line.contains("(current)"), "{parent_line}");
        }
    }

    #[test]
    fn status_words_are_colored_by_what_the_session_needs() {
        use ratatui::style::Color;
        let rows = vec![
            row(1, "asks", SessionStatus::WaitingOnYou),
            row(2, "busy", SessionStatus::Working),
            row(3, "done", SessionStatus::Ready),
            row(4, "broke", SessionStatus::Failed),
            SessionRow {
                on_screen: true,
                ..row(5, "selected", SessionStatus::Closed)
            },
        ];
        let titles = [
            ("waiting on you", "asks"),
            ("working", "busy"),
            ("ready", "done"),
            ("failed", "broke"),
        ];
        let list = view(rows);
        let height = list.desired_height(80);
        let mut terminal = Terminal::new(TestBackend::new(80, height)).expect("terminal");
        terminal
            .draw(|frame| list.render(frame.area(), frame.buffer_mut()))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        let color_of = |word: &str| {
            let title = titles
                .iter()
                .find(|(status, _)| *status == word)
                .map(|(_, title)| *title)
                .expect("row title");
            (0..buffer.area.height)
                .find_map(|y| {
                    let line: String = (0..buffer.area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect();
                    if !line.contains(title) {
                        return None;
                    }
                    let column = line.rfind(word)?;
                    let x = line[..column].chars().count() as u16;
                    Some(buffer[(x, y)].fg)
                })
                .unwrap_or_else(|| panic!("{word} is shown"))
        };
        assert_eq!(color_of("waiting on you"), Color::Yellow);
        assert_eq!(color_of("working"), Color::Cyan);
        assert_eq!(color_of("ready"), Color::Green);
        assert_eq!(color_of("failed"), Color::Red);
    }

    #[test]
    fn after_tab_the_footer_offers_every_session_again() {
        let mut waiting = row(1, "waiting session", SessionStatus::WaitingOnYou);
        waiting.on_screen = true;
        let mut list = view(vec![waiting, row(2, "ready session", SessionStatus::Ready)]);
        list.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let screen = render(&list, 80);
        assert!(screen.contains("tab all sessions"), "{screen}");
        assert!(!screen.contains("tab waiting on you"), "{screen}");
        assert_snapshot!(screen);
    }

    #[test]
    fn waiting_filter_explains_when_nobody_needs_attention() {
        let mut list = view(vec![row(1, "ready session", SessionStatus::Ready)]);
        list.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        let screen = render(&list, 80);
        assert!(screen.contains("No sessions waiting on you"), "{screen}");
        assert_snapshot!(screen);
    }

    #[test]
    fn waiting_filter_updates_when_the_last_request_is_answered() {
        let mut list = view(vec![row(1, "session", SessionStatus::WaitingOnYou)]);
        list.handle_key_event(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        list.replace_rows(vec![row(1, "session", SessionStatus::Ready)]);
        let screen = render(&list, 80);
        assert!(screen.contains("No sessions waiting on you"), "{screen}");
        list.handle_key_event(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        list.handle_key_event(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        let screen = render(&list, 80);
        assert!(screen.contains("No matching options"), "{screen}");
    }

    /// The title on the row the marker points at.
    fn selected_title(list: &SessionListView) -> String {
        let screen = render(list, 80);
        screen
            .lines()
            .find_map(|line| {
                let line = line.trim_start_matches('"');
                line.strip_prefix("▏› ").map(|rest| {
                    rest.split("  ")
                        .next()
                        .unwrap_or_default()
                        .trim()
                        .to_string()
                })
            })
            .unwrap_or_else(|| panic!("a selected row in {screen}"))
    }

    fn press(list: &mut SessionListView, code: KeyCode) {
        list.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
    }

    #[test]
    fn j_and_k_move_through_the_list_and_letters_do_not_filter() {
        let mut list = view(rich_rows());
        assert!(selected_title(&list).contains("Greeting in three words"));
        press(&mut list, KeyCode::Char('j'));
        assert!(
            selected_title(&list).contains("inspect the parser"),
            "j moves down"
        );
        press(&mut list, KeyCode::Char('k'));
        assert!(
            selected_title(&list).contains("Greeting in three words"),
            "k moves up"
        );
        press(&mut list, KeyCode::Char('x'));
        let screen = render(&list, 80);
        for title in [
            "Greeting in three words",
            "inspect the parser",
            "say hi in 3 words",
        ] {
            assert!(screen.contains(title), "a letter does not filter: {screen}");
        }
    }

    #[test]
    fn slash_searches_and_esc_leaves_search_before_closing() {
        let mut list = view(rich_rows());
        press(&mut list, KeyCode::Char('/'));
        for character in "say".chars() {
            press(&mut list, KeyCode::Char(character));
        }
        let screen = render(&list, 80);
        assert!(screen.contains("say hi in 3 words"), "{screen}");
        assert!(
            !screen.contains("inspect the parser"),
            "typing filters while searching: {screen}"
        );

        list.on_escape();
        assert!(!list.is_complete(), "the first esc only leaves search");
        let screen = render(&list, 80);
        assert!(
            screen.contains("inspect the parser"),
            "the filter is gone: {screen}"
        );
        press(&mut list, KeyCode::Char('j'));
        assert!(
            selected_title(&list).contains("inspect the parser"),
            "j moves again"
        );

        list.on_escape();
        assert!(list.is_complete(), "the second esc closes the list");
    }

    #[test]
    fn the_hints_name_j_k_and_slash() {
        let screen = render(&view(rich_rows()), 120);
        assert!(screen.contains("↑↓/j/k move"), "{screen}");
        assert!(screen.contains("/ search"), "{screen}");
        assert!(!screen.contains("type to search"), "{screen}");
    }

    #[test]
    fn a_busy_session_needs_a_second_close_press() {
        let (tx, mut rx) = unbounded_channel();
        let mut busy = row(1, "busy session", SessionStatus::Working);
        busy.on_screen = false;
        let mut current = row(2, "current session", SessionStatus::Ready);
        current.on_screen = true;
        let mut list = SessionListView::new(vec![busy, current], AppEventSender::new(tx));
        let mut closes = || {
            std::iter::from_fn(|| rx.try_recv().ok())
                .filter_map(|event| match event {
                    AppEvent::CloseSession { generation } => Some(generation),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        // The list opens on the session on screen; Up selects the busy one.
        list.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        list.handle_key_event(ctrl_d);
        let after_first = closes();
        let screen = render(&list, 80);
        list.handle_key_event(ctrl_d);
        let after_second = closes();
        assert!(screen.contains("ctrl+d again to close"), "{screen}");
        assert_eq!((after_first, after_second), (Vec::new(), vec![1]));
    }

    #[test]
    fn every_session_takes_two_rows_at_each_layout_width() {
        let list = view(rich_rows());
        for width in [58, 80, 87, 120] {
            assert_snapshot!(format!("two_row_sessions_{width}"), render(&list, width));
        }
    }

    #[test]
    fn a_session_without_a_reply_says_so() {
        let screen = render(&view(rich_rows()), 80);
        let fresh = screen
            .lines()
            .position(|line| line.contains("say hi in 3 words"))
            .expect("fresh row");
        let below = screen.lines().nth(fresh + 1).unwrap_or_default();
        assert!(below.contains("no reply yet"), "{screen}");
        assert!(below.contains("0s ago"), "{screen}");
    }

    #[test]
    fn the_header_carries_the_only_status_summary() {
        let screen = render(&view(rich_rows()), 120);
        assert_eq!(screen.matches("2 working").count(), 1, "{screen}");
        assert!(
            screen
                .lines()
                .any(|line| line.contains("Sessions 3 · 2 working")),
            "{screen}"
        );
    }

    #[test]
    fn the_header_lines_up_with_the_rows_below_it() {
        let screen = render(&view(rich_rows()), 80);
        let indent = |needle: &str| {
            let line = screen
                .lines()
                .find(|line| line.contains(needle))
                .unwrap_or_else(|| panic!("{needle} in {screen}"));
            let line = line.trim_start_matches('"');
            line.len() - line.trim_start().len()
        };
        assert_eq!(
            indent("Sessions 3"),
            indent("All  waiting on you"),
            "{screen}"
        );
        assert_eq!(indent("Sessions 3"), indent("› "), "{screen}");
    }

    fn render_buffer(view: &SessionListView, width: u16) -> ratatui::buffer::Buffer {
        let height = view.desired_height(width);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal
            .draw(|frame| view.render(frame.area(), frame.buffer_mut()))
            .expect("draw");
        terminal.backend().buffer().clone()
    }

    /// The style of `text` on the first list row that shows it.
    fn style_at(buffer: &ratatui::buffer::Buffer, text: &str) -> ratatui::style::Style {
        (0..buffer.area.height)
            .find_map(|y| {
                let line: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                if line.contains("Sessions ") {
                    return None;
                }
                let at = line.find(text)?;
                let x = unicode_width::UnicodeWidthStr::width(&line[..at]) as u16;
                Some(buffer[(x, y)].style())
            })
            .unwrap_or_else(|| panic!("{text} is shown"))
    }

    #[test]
    fn second_rows_are_faint_and_the_selected_row_keeps_its_colours() {
        use ratatui::style::Color;
        use ratatui::style::Modifier;
        let buffer = render_buffer(&view(rich_rows()), 120);
        for faint in ["no reply yet", "Hi there", "opus 5.5", "2m ago"] {
            let style = style_at(&buffer, faint);
            assert!(
                style.add_modifier.contains(Modifier::DIM) || style.fg == Some(Color::DarkGray),
                "{faint} is faint: {style:?}"
            );
        }
        // The session on screen is selected when the list opens.
        assert_eq!(style_at(&buffer, "working").fg, Some(Color::Cyan));
        assert_eq!(style_at(&buffer, "claude-code").fg, Some(Color::Yellow));
    }
}
