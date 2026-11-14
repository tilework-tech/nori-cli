//! The session list: every session this TUI holds, in navigation order.
//!
//! Rows show a status glyph, the title, the agent and a status word, with a
//! one-line preview below. Children sit indented under their parent. Tab
//! toggles between every session and only those waiting on the user, and
//! typing filters by title or agent. Rows are replaced while the list is open
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
    /// The session's own status, which decides its glyph.
    pub(crate) status: SessionStatus,
    /// The word shown in the status column. For a parent this follows its
    /// whole family.
    pub(crate) status_word: String,
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
            PickerColumn::fixed(STATUS, "Status", 13),
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
        SessionStatus::NeedsYou => "●",
        SessionStatus::Working => "⠋",
        SessionStatus::Ready => "○",
        SessionStatus::Failed => "✕",
        SessionStatus::Closed => "·",
        SessionStatus::Starting => "…",
    }
}

fn tone(status: SessionStatus) -> DetailTone {
    match status {
        SessionStatus::NeedsYou => DetailTone::Warning,
        SessionStatus::Failed => DetailTone::Error,
        SessionStatus::Closed | SessionStatus::Starting => DetailTone::Muted,
        SessionStatus::Working | SessionStatus::Ready => DetailTone::Default,
    }
}

fn items(rows: &[SessionRow]) -> Vec<PickerItem<SessionGeneration>> {
    rows.iter()
        .map(|row| {
            let mut item = PickerItem::new(
                row.generation,
                TITLE,
                format!("{} {}", glyph(row.status), row.title),
            )
            .cell(AGENT, row.agent.clone())
            .cell(STATUS, row.status_word.clone())
            .cell_detail_tone(STATUS, tone(row.family_status))
            .search_text(format!("{} {}", row.title, row.agent))
            .depth(row.depth)
            .current(row.on_screen);
            if let Some(provider) = row.provider {
                item = item.cell_tone(AGENT, provider);
            }
            if row.status == SessionStatus::Closed {
                item = item.cell_detail_tone(TITLE, DetailTone::Muted);
            }
            if row.family_status == SessionStatus::NeedsYou {
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
        Picker::new(&self.state)
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
                    Some(KeyHint::new("tab", WAITING_FILTER_LABEL)),
                    Some(KeyHint::new("esc", "close")),
                ]
                .into_iter()
                .flatten(),
            )
            .render(list_area, buf);
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
