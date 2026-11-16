//! What the terminal shows for the active session.
//!
//! The scrollback belongs to one session at a time. Showing a session clears
//! the screen and the terminal scrollback, then reprints that session's
//! finished cells. Resize reflow and session jumps share this path.

use std::sync::Arc;

use color_eyre::eyre::Result;

use super::App;
use super::session_deck::Direction;
use super::session_deck::ParkedSession;
use crate::app_event::AppEvent;
use crate::app_event::SessionGeneration;
use crate::chatwidget::ChatWidget;
use crate::history_cell::HistoryCell;
use crate::pager_overlay::Overlay;
use crate::transcript_reflow::render_transcript_tail;
use crate::tui;
use ratatui::text::Line;

impl App {
    /// Clear the terminal, including its scrollback, and reprint the active
    /// session's finished cells at the current width. The in-progress turn
    /// stays in the viewport, because it lives in the active widget and not
    /// in the transcript.
    pub(super) fn reprint_active_transcript(&mut self, tui: &mut tui::Tui) -> Result<()> {
        let width = tui.terminal.size()?.width;
        let lines = render_transcript_tail(&self.transcript_cells, width);
        tui.clear_pending_history_lines();
        tui.terminal
            .clear_scrollback_and_visible_screen_for_reflow()?;
        let mut area = tui.terminal.viewport_area;
        area.y = 0;
        area.width = width;
        tui.terminal.set_viewport_area(area);
        self.deferred_history_lines.clear();
        self.has_emitted_history_lines = !lines.is_empty();
        if !lines.is_empty() {
            tui.insert_history_lines(lines);
        }
        self.transcript_reflow.mark_reflowed(width);
        Ok(())
    }

    /// Start an empty transcript for a session that replaces the active one
    /// and will replay its own history, such as a resumed session. The
    /// previous session's cells leave the transcript and the terminal.
    pub(super) fn begin_replacing_session_transcript(&mut self, tui: &mut tui::Tui) -> Result<()> {
        self.transcript_cells.clear();
        self.backtrack = crate::app_backtrack::BacktrackState::default();
        self.reprint_active_transcript(tui)
    }

    /// Make `widget` the active session. A session let go of with
    /// `shutdown_current_conversation` under `[features] multi_session` is
    /// parked and the screen starts clean for the new one; otherwise the new
    /// widget takes the old one's place in the navigation order.
    pub(crate) fn install_active_widget(&mut self, widget: ChatWidget) {
        let previous = self.chat_widget.session_generation();
        let next = widget.session_generation();
        let mut old = std::mem::replace(&mut self.chat_widget, widget);
        match self.retiring_transcript.take() {
            Some(transcript) => {
                old.set_foreground(false);
                self.session_deck.park(
                    previous,
                    ParkedSession {
                        widget: Box::new(old),
                        transcript,
                    },
                );
                self.session_deck.add(next);
                self.session_reprint_pending = true;
            }
            None => {
                self.session_deck.replace(previous, next);
                self.remote_control.forget_session(previous);
            }
        }
        self.remote_control
            .select_session(next, self.chat_widget.managed_conversation_id());
    }

    /// Bring session `target` on screen. Its widget becomes active right away,
    /// so keys reach it; the terminal is reprinted on the next draw.
    pub(super) fn show_session(&mut self, target: SessionGeneration) {
        let current = self.chat_widget.session_generation();
        // A session being set up for the screen would lose its place.
        if target == current || self.session_setup_pending() {
            return;
        }
        let Some(parked) = self.session_deck.unpark(target) else {
            return;
        };
        let mut previous = std::mem::replace(&mut self.chat_widget, *parked.widget);
        let previous_transcript = std::mem::replace(&mut self.transcript_cells, parked.transcript);
        previous.set_foreground(false);
        previous.close_session_list();
        if previous.has_harness_session() || previous.managed_conversation_id().is_some() {
            self.session_deck.park(
                current,
                ParkedSession {
                    widget: Box::new(previous),
                    transcript: previous_transcript,
                },
            );
        } else {
            // A blank session that never started has nothing to come back to.
            self.session_deck.remove(current);
        }
        self.backtrack = crate::app_backtrack::BacktrackState::default();
        self.chat_widget.set_foreground(true);
        self.remote_control
            .select_session(target, self.chat_widget.managed_conversation_id());
        let agent = self.chat_widget.config_ref().active_agent.clone();
        self.config.agent = agent.clone();
        self.config.active_agent = agent;
        self.session_reprint_pending = true;
    }

    /// Move one session along the navigation order, past closed sessions.
    pub(super) fn step_session(&mut self, direction: Direction) {
        let current = self.chat_widget.session_generation();
        let closed: Vec<SessionGeneration> = self
            .session_facts()
            .into_iter()
            .filter(|fact| fact.status == nori_protocol::SessionStatus::Closed)
            .map(|fact| fact.generation)
            .collect();
        if let Some(target) = self
            .session_deck
            .next_where(current, direction, |generation| {
                !closed.contains(&generation)
            })
        {
            self.show_session(target);
        }
    }

    /// Reprint the terminal for a session that just came on screen. Runs on
    /// draw, so a burst of jumps reprints only the last target.
    pub(super) fn run_pending_session_reprint(&mut self, tui: &mut tui::Tui) -> Result<()> {
        if !self.session_reprint_pending || self.overlay.is_some() || tui.is_alt_screen_active() {
            return Ok(());
        }
        self.session_reprint_pending = false;
        if self.chat_widget.has_active_agent_stream() {
            self.transcript_reflow.mark_resize_requested_during_stream();
        }
        self.reprint_active_transcript(tui)
    }

    /// File history sent by a session's widget in that session's transcript.
    /// Returns the event when it belongs to the active session, for the
    /// caller to show.
    pub(super) fn route_session_history(
        &mut self,
        generation: SessionGeneration,
        event: AppEvent,
    ) -> Option<AppEvent> {
        let Some(parked) = self.session_deck.parked_mut(generation) else {
            return Some(event);
        };
        match event {
            AppEvent::InsertHistoryCell(cell) => {
                let cell: Arc<dyn HistoryCell> = cell.into();
                parked.transcript.push(cell);
            }
            AppEvent::ConsolidateAgentMessage { source, cwd } => {
                crate::transcript_reflow::consolidate_agent_message_cells(
                    &mut parked.transcript,
                    source,
                    &cwd,
                );
            }
            AppEvent::AgentConnecting { display_name } => {
                parked.widget.show_connecting_status(&display_name);
            }
            AppEvent::AgentSpawnFailed { agent_name, error } => {
                parked.widget.on_agent_spawn_failed(&agent_name, &error);
            }
            other => return Some(other),
        }
        None
    }

    /// Hand a session event to the parked widget it belongs to. Returns the
    /// event when no parked session owns it.
    pub(super) fn route_parked_session_event(
        &mut self,
        generation: SessionGeneration,
        event: nori_protocol::SessionEvent,
    ) -> Option<nori_protocol::SessionEvent> {
        match self.session_deck.parked_mut(generation) {
            Some(parked) => {
                parked.widget.handle_session_event(generation, event);
                None
            }
            None => Some(event),
        }
    }

    /// Advance streaming in every session, so parked sessions keep filing
    /// their streamed text while another session is on screen.
    pub(super) fn tick_all_sessions(&mut self) {
        self.chat_widget.on_commit_tick();
        for (_, parked) in self.session_deck.parked_sessions_mut() {
            parked.widget.on_commit_tick();
        }
    }

    /// Add a cell to the active session's transcript and print it.
    pub(super) fn insert_active_history_cell(
        &mut self,
        tui: &mut tui::Tui,
        cell: Box<dyn HistoryCell>,
    ) {
        let cell: Arc<dyn HistoryCell> = cell.into();
        if let Some(Overlay::Transcript(t)) = &mut self.overlay {
            t.insert_cell(cell.clone());
            tui.frame_requester().schedule_frame();
        }
        self.transcript_cells.push(cell.clone());
        let mut display = cell.display_lines(tui.terminal.last_known_screen_size.width);
        if display.is_empty() {
            return;
        }
        // Only insert a separating blank line for new cells that are not part
        // of an ongoing stream. Streaming continuations should not accrue
        // extra blank lines between chunks.
        if !cell.is_stream_continuation() {
            if self.has_emitted_history_lines {
                display.insert(0, Line::from(""));
            } else {
                self.has_emitted_history_lines = true;
            }
        }
        if self.overlay.is_some() {
            self.deferred_history_lines.extend(display);
        } else {
            tui.insert_history_lines(display);
        }
    }

    /// Replace the active session's just-finished stream cells with one
    /// Markdown cell.
    pub(super) fn consolidate_active_agent_message(
        &mut self,
        tui: &mut tui::Tui,
        source: String,
        cwd: std::path::PathBuf,
    ) {
        let consolidation = crate::transcript_reflow::consolidate_agent_message_cells(
            &mut self.transcript_cells,
            source,
            &cwd,
        );
        if let Some((range, replacement)) = consolidation
            && let Some(Overlay::Transcript(transcript)) = &mut self.overlay
        {
            transcript.replace_cells(range, replacement);
            tui.frame_requester().schedule_frame();
        }
        if self.transcript_reflow.take_stream_finish_reflow_needed() {
            self.transcript_reflow.schedule_immediate();
            tui.frame_requester().schedule_frame();
        }
    }

    /// Whether any session, on screen or parked, has a stream in progress.
    pub(super) fn any_session_streaming(&mut self) -> bool {
        self.chat_widget.has_active_agent_stream()
            || self
                .session_deck
                .parked_sessions_mut()
                .any(|(_, parked)| parked.widget.has_active_agent_stream())
    }

    /// Shut down every parked session in parallel before the TUI exits. Each
    /// close is bounded by the session manager's shutdown limit.
    pub(super) async fn close_parked_sessions(&mut self) {
        let grace = if self.cloud_mode {
            std::time::Duration::from_secs(1)
        } else {
            std::time::Duration::ZERO
        };
        let mut closing = tokio::task::JoinSet::new();
        for (_, parked) in self.session_deck.parked_sessions_mut() {
            if let Some(id) = parked.widget.managed_conversation_id() {
                let manager = self.session_manager.clone();
                closing.spawn(async move { manager.close(&id, grace).await });
            }
        }
        while let Some(result) = closing.join_next().await {
            if let Ok(Err(error)) = result {
                tracing::warn!(%error, "a parked session did not shut down cleanly");
            }
        }
    }

    /// Whether a session is being set up for the session on screen. Session
    /// moves wait until it is installed, so it cannot take another session's
    /// place.
    pub(super) fn session_setup_pending(&self) -> bool {
        self.pending_session_activation.is_some()
            || self.candidate_agent.is_some()
            || self.primary_agent_preparation.is_some()
    }
}
