//! What the TUI shows about every session it holds: the status line now, and
//! the session list and background-request line built from the same facts.

use nori_protocol::SessionStatus;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::App;
use crate::app_event::SessionGeneration;
use crate::chatwidget::ChatWidget;
use crate::session_status::status_summary;

/// One session as the overview sees it.
pub(crate) struct SessionFacts {
    pub(crate) generation: SessionGeneration,
    pub(crate) status: SessionStatus,
    pub(crate) on_screen: bool,
}

impl App {
    fn widget_for(&self, generation: SessionGeneration) -> Option<&ChatWidget> {
        if generation == self.chat_widget.session_generation() {
            return Some(&self.chat_widget);
        }
        self.session_deck
            .parked(generation)
            .map(|parked| parked.widget.as_ref())
    }

    fn status_of(&self, widget: &ChatWidget) -> SessionStatus {
        widget
            .managed_conversation_id()
            .and_then(|id| self.session_manager.get(&id))
            .map_or(SessionStatus::Starting, |record| record.status)
    }

    /// Every session in navigation order, with its current status.
    pub(crate) fn session_facts(&self) -> Vec<SessionFacts> {
        let active = self.chat_widget.session_generation();
        self.session_deck
            .order()
            .iter()
            .filter_map(|generation| {
                let widget = self.widget_for(*generation)?;
                Some(SessionFacts {
                    generation: *generation,
                    status: self.status_of(widget),
                    on_screen: *generation == active,
                })
            })
            .collect()
    }

    /// Refresh the lines about other sessions on the active widget. Runs
    /// before every draw; with one session, or without
    /// `[features] multi_session`, nothing is shown.
    pub(super) fn refresh_session_overview(&mut self) {
        if !self.config.multi_session {
            return;
        }
        let facts = self.session_facts();
        for fact in &facts {
            self.unseen_finishes
                .observe(fact.generation, fact.status, fact.on_screen);
        }
        let mut lines = Vec::new();
        if facts.len() > 1
            && let Some(summary) = status_summary(facts.iter().map(|fact| fact.status))
        {
            lines.push(Line::from(summary.dim()));
        }
        self.chat_widget.set_session_lines(lines);
    }
}
