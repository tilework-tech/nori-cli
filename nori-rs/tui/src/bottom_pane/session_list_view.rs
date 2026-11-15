//! The session list: every session this TUI holds, in navigation order.
//!
//! Rows show a status glyph, the title, the agent and a status word, with a
//! one-line preview below. Children sit indented under their parent. Tab
//! toggles between every session and only those waiting on the user, and
//! typing filters by title or agent. Ctrl+D closes the selected session (a
//! busy one takes a second press) and removes a closed one from the list. Rows are replaced while the list is open
//! without moving the selection.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use nori_protocol::SessionStatus;
use nori_tui_components::DetailTone;
use nori_tui_components::KeyHint;
use nori_tui_components::Picker;
use nori_tui_components::PickerAction;
use nori_tui_components::PickerColumn;
use nori_tui_components::PickerColumnWidth;
use nori_tui_components::PickerDensity;
use nori_tui_components::PickerItem;
use nori_tui_components::PickerOutcome;
use nori_tui_components::PickerState;
use nori_tui_components::ProviderKind;
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
    pub(crate) preview: Option<String>,
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

const TITLE: &str = "title";
const AGENT: &str = "agent";
const STATUS: &str = "status";

impl SessionListView {
    pub(crate) fn new(rows: Vec<SessionRow>, app_event_tx: AppEventSender) -> Self {
        let columns = [
            PickerColumn::flexible(TITLE, "Session").width(PickerColumnWidth::Flexible {
                min: 16,
                max: 72,
                weight: 4,
            }),
            PickerColumn::fixed(AGENT, "Agent", 14).hide_below(48),
            PickerColumn::fixed(STATUS, "Status", 16),
        ];
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
    pub(crate) fn replace_rows(&mut self, rows: Vec<SessionRow>) {
        if rows != self.rows {
            self.state.replace_items(items(&rows));
            self.rows = rows;
        }
    }

    fn header(&self) -> Line<'static> {
        let count = self.rows.len();
        let mut text = format!("Sessions {count}");
        if let Some(summary) = status_summary(self.rows.iter().map(|row| row.status)) {
            text.push_str(" · ");
            text.push_str(&summary);
        }
        let hint = if self.state.active_category.is_some() {
            "  tab: all sessions · type to search"
        } else {
            "  tab: waiting only · type to search"
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

fn glyph(status: SessionStatus) -> &'static str {
    match status {
        SessionStatus::WaitingOnYou => "●",
        SessionStatus::Working => "⠋",
        SessionStatus::Ready => "○",
        SessionStatus::Failed => "✕",
        SessionStatus::Closed | SessionStatus::Done => "·",
        SessionStatus::Starting => "…",
    }
}

fn items(rows: &[SessionRow]) -> Vec<PickerItem<SessionGeneration>> {
    rows.iter()
        .map(|row| {
            let mut item = PickerItem::new(
                row.generation,
                TITLE,
                format!("{} {}", glyph(row.family_status), row.title),
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
            if let Some(preview) = &row.preview {
                item = item.description(preview.clone());
            }
            item
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
            (KeyCode::Up, _) | (KeyCode::Char('p'), KeyModifiers::CONTROL) => PickerAction::MoveUp,
            (KeyCode::Down, _) | (KeyCode::Char('n'), KeyModifiers::CONTROL) => {
                PickerAction::MoveDown
            }
            (KeyCode::PageUp, _) => PickerAction::PageUp,
            (KeyCode::PageDown, _) => PickerAction::PageDown,
            (KeyCode::Home, _) => PickerAction::First,
            (KeyCode::End, _) => PickerAction::Last,
            (KeyCode::Tab, _) => PickerAction::NextCategory,
            (KeyCode::BackTab, _) => PickerAction::PreviousCategory,
            (KeyCode::Enter, _) => PickerAction::Submit,
            (KeyCode::Backspace, _) => PickerAction::Backspace,
            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                if !self.state.search_active {
                    self.state.handle(PickerAction::ActivateSearch);
                }
                PickerAction::AppendQuery(c)
            }
            _ => return,
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
        if self.state.query.is_empty() {
            self.complete = true;
        } else {
            self.state.handle(PickerAction::ClearQuery);
            self.state.handle(PickerAction::DeactivateSearch);
        }
        CancellationEvent::Handled
    }

    fn is_session_list(&self) -> bool {
        true
    }

    fn update_session_list(&mut self, rows: &[SessionRow]) -> bool {
        self.replace_rows(rows.to_vec());
        true
    }
}

impl Renderable for SessionListView {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let header_area = Rect::new(
            area.x.saturating_add(2),
            area.y,
            area.width.saturating_sub(2),
            1,
        );
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
            .compact_descriptions(true)
            .show_title(false)
            .show_details(false)
            .footer_hints(
                [
                    Some(KeyHint::new("↑↓", "move")),
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
        let row_height = if self.rows.iter().any(|row| row.preview.is_some()) {
            2
        } else {
            1
        };
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
            preview: None,
            depth: 0,
            on_screen: false,
        }
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
        list.handle_key_event(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
        let screen = render(&list, 80);
        assert!(screen.contains("No matching options"), "{screen}");
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
}
