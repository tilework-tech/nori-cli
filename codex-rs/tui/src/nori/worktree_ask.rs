//! Worktree ask popup for the "ask" auto-worktree mode.
//!
//! Shows a simple two-option popup at TUI startup asking the user whether
//! to create a git worktree for this session.

use crate::render::Insets;
use crate::render::renderable::ColumnRenderable;
use crate::render::renderable::Renderable;
use crate::render::renderable::RenderableExt as _;
use crate::selection_list::selection_option_row;
use crate::tui::FrameRequester;
use crate::tui::Tui;
use crate::tui::TuiEvent;
use color_eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::prelude::Widget;
use ratatui::style::Stylize as _;
use ratatui::text::Line;
use ratatui::widgets::Clear;
use ratatui::widgets::WidgetRef;
use tokio_stream::StreamExt;

/// Run the worktree ask popup. Returns `true` if the user chose to create a worktree.
pub(crate) async fn run_worktree_ask_popup(tui: &mut Tui) -> Result<bool> {
    let mut screen = WorktreeAskScreen::new(tui.frame_requester());
    tui.draw(u16::MAX, |frame| {
        frame.render_widget_ref(&screen, frame.area());
    })?;

    let events = tui.event_stream();
    tokio::pin!(events);

    while !screen.is_done() {
        if let Some(event) = events.next().await {
            match event {
                TuiEvent::Key(key_event) => screen.handle_key(key_event),
                TuiEvent::Paste(_) => {}
                TuiEvent::Draw => {
                    tui.draw(u16::MAX, |frame| {
                        frame.render_widget_ref(&screen, frame.area());
                    })?;
                }
            }
        } else {
            break;
        }
    }

    Ok(screen.selection == Some(WorktreeSelection::Yes))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorktreeSelection {
    Yes,
    No,
}

struct WorktreeAskScreen {
    request_frame: FrameRequester,
    highlighted: WorktreeSelection,
    selection: Option<WorktreeSelection>,
}

impl WorktreeAskScreen {
    fn new(request_frame: FrameRequester) -> Self {
        Self {
            request_frame,
            highlighted: WorktreeSelection::Yes,
            selection: None,
        }
    }

    fn handle_key(&mut self, key_event: KeyEvent) {
        if key_event.kind == KeyEventKind::Release {
            return;
        }
        if key_event.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key_event.code, KeyCode::Char('c') | KeyCode::Char('d'))
        {
            self.select(WorktreeSelection::No);
            return;
        }
        match key_event.code {
            KeyCode::Up | KeyCode::Char('k') => self.set_highlight(self.highlighted.other()),
            KeyCode::Down | KeyCode::Char('j') => self.set_highlight(self.highlighted.other()),
            KeyCode::Char('1') | KeyCode::Char('y') => self.select(WorktreeSelection::Yes),
            KeyCode::Char('2') | KeyCode::Char('n') => self.select(WorktreeSelection::No),
            KeyCode::Enter => self.select(self.highlighted),
            KeyCode::Esc => self.select(WorktreeSelection::No),
            _ => {}
        }
    }

    fn set_highlight(&mut self, highlight: WorktreeSelection) {
        if self.highlighted != highlight {
            self.highlighted = highlight;
            self.request_frame.schedule_frame();
        }
    }

    fn select(&mut self, selection: WorktreeSelection) {
        self.highlighted = selection;
        self.selection = Some(selection);
        self.request_frame.schedule_frame();
    }

    fn is_done(&self) -> bool {
        self.selection.is_some()
    }
}

impl WorktreeSelection {
    fn other(self) -> Self {
        match self {
            Self::Yes => Self::No,
            Self::No => Self::Yes,
        }
    }
}

impl WidgetRef for &WorktreeAskScreen {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        let mut column = ColumnRenderable::new();

        column.push("");
        column.push(Line::from(
            "  Create a git worktree for this session?".bold(),
        ));
        column.push("  Each session gets an isolated branch and working directory.");
        column.push("");
        column.push(selection_option_row(
            0,
            "Yes, create a worktree".to_string(),
            self.highlighted == WorktreeSelection::Yes,
        ));
        column.push(selection_option_row(
            1,
            "No, continue without a worktree".to_string(),
            self.highlighted == WorktreeSelection::No,
        ));
        column.push("");
        column.push(
            Line::from(vec![
                "Press ".dim(),
                "enter".dim().bold(),
                " to confirm, ".dim(),
                "esc".dim().bold(),
                " to skip".dim(),
            ])
            .inset(Insets::tlbr(0, 2, 0, 0)),
        );
        column.render(area, buf);
    }
}
