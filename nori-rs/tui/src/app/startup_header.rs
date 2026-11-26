//! The welcome header while the default agent session starts.
//!
//! At interactive startup the welcome card does not go straight into the
//! scrollback. It sits at the top of the inline viewport, above the chat
//! widget, so its title can show Jiro's face moving in place while the
//! composer below already takes input. When the face settles the card is
//! written to the history in the same draw that shrinks the viewport, so the
//! rows on screen do not move.
//!
//! - Before the session is ready, any other history freezes the face with its
//!   eyes open and writes the card first, so nothing is ever shown above it
//!   and a failed start never winks.
//! - Once the session is ready, history that arrives during the wink and
//!   retype is shown under the card in the viewport and written with it, so
//!   the configuration rows appear at once without cutting the finale short.
//!   History that would not fit settles the face at once instead.
//! - The full-screen view settles the face and writes the card, because it
//!   draws from the transcript. An overlay only pauses the face: it is not
//!   drawn meanwhile, and it carries on when the overlay closes.

use std::sync::Arc;
use std::time::Instant;

use color_eyre::eyre::Result;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use super::App;
use crate::history_cell::HistoryCell;
use crate::insert_history::wrap_history_lines_for_width;
use crate::nori::session_header::StartupFace;
use crate::render::renderable::Renderable;
use crate::tui;

pub(crate) struct StartupHeader {
    card: Arc<dyn HistoryCell>,
    face: StartupFace,
    ready: bool,
    /// History written after readiness, shown under the card until it settles.
    held: Vec<Arc<dyn HistoryCell>>,
}

impl StartupHeader {
    pub(crate) fn new(card: Arc<dyn HistoryCell>, face: StartupFace) -> Self {
        Self {
            card,
            face,
            ready: false,
            held: Vec::new(),
        }
    }

    /// The card and the held history as the scrollback would show them,
    /// wrapped to `width`.
    fn lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = self.card.display_lines(width);
        for cell in &self.held {
            let display = cell.display_lines(width);
            if display.is_empty() {
                continue;
            }
            if !cell.is_stream_continuation() {
                lines.push(Line::from(""));
            }
            lines.extend(display);
        }
        wrap_history_lines_for_width(&lines, width)
    }
}

impl App {
    /// The default session is ready: wink and retype.
    pub(super) fn startup_header_ready(&mut self, tui: &tui::Tui) {
        if let Some(header) = &mut self.startup_header {
            header.face.ready(Instant::now());
            header.ready = true;
            tui.frame_requester().schedule_frame();
        }
    }

    /// Show `cell` under the welcome header while it finishes. Returns the
    /// cell when it belongs in the history right away, after the header.
    pub(super) fn hold_under_startup_header(
        &mut self,
        tui: &mut tui::Tui,
        cell: Arc<dyn HistoryCell>,
    ) -> Option<Arc<dyn HistoryCell>> {
        let Some(header) = &mut self.startup_header else {
            return Some(cell);
        };
        if header.ready {
            let size = tui.terminal.last_known_screen_size;
            header.held.push(cell);
            let rows = header.lines(size.width).len();
            let chat = usize::from(self.chat_widget.desired_height(size.width));
            if rows + chat <= usize::from(size.height) {
                tui.frame_requester().schedule_frame();
                return None;
            }
            let cell = header.held.pop();
            self.commit_startup_header(tui);
            return cell;
        }
        self.commit_startup_header(tui);
        Some(cell)
    }

    /// Settle the face and write the card, then any held history.
    pub(super) fn commit_startup_header(&mut self, tui: &mut tui::Tui) {
        let Some(header) = self.startup_header.take() else {
            return;
        };
        header.face.freeze();
        self.append_history_cell(tui, header.card);
        for cell in header.held {
            self.append_history_cell(tui, cell);
        }
    }

    /// Advance the face before an inline draw. Writes the card once the face
    /// settles or the header no longer fits above the chat widget, and
    /// returns the header's height otherwise.
    pub(super) fn advance_startup_header(&mut self, tui: &mut tui::Tui) -> Result<u16> {
        let Some(header) = &self.startup_header else {
            return Ok(0);
        };
        let next = header.face.tick(Instant::now());
        let size = tui.terminal.size()?;
        let rows = header.lines(size.width).len();
        let chat = usize::from(self.chat_widget.desired_height(size.width));
        if header.face.is_finished() || rows + chat > usize::from(size.height) {
            self.commit_startup_header(tui);
            return Ok(0);
        }
        if let Some(next) = next {
            tui.frame_requester().schedule_frame_in(next);
        }
        Ok(u16::try_from(rows).unwrap_or(u16::MAX))
    }

    /// Draw the header into the top `area` of the viewport.
    pub(super) fn render_startup_header(&self, area: Rect, buf: &mut Buffer) {
        if let Some(header) = &self.startup_header {
            Paragraph::new(header.lines(area.width)).render(area, buf);
        }
    }
}

/// Split the inline viewport into the header's `rows` on top and the chat
/// widget below.
pub(super) fn split_viewport(area: Rect, rows: u16) -> (Rect, Rect) {
    let rows = rows.min(area.height);
    (
        Rect {
            height: rows,
            ..area
        },
        Rect {
            y: area.y + rows,
            height: area.height - rows,
            ..area
        },
    )
}
