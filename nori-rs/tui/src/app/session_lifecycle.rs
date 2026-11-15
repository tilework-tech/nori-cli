//! Closing sessions, the session cap, and loop iterations, with
//! `[features] multi_session`.
//!
//! A closed session stays in the session list, marked closed, until the user
//! removes it, so it is clear what happened to it. Loop iterations replace
//! the session they came from in its place in the list, on screen or not.

use color_eyre::eyre::Result;
use nori_protocol::SessionStatus;

use super::App;
use super::PendingSessionActivation;
use super::session_deck::Direction;
use super::session_deck::ParkedSession;
use crate::app_event::SessionGeneration;
use crate::chatwidget::ChatWidget;
use crate::tui;

impl App {
    /// With several sessions, refuse to start another one at the cap and say
    /// how to make room. Returns true when the start was refused.
    pub(super) fn refuse_at_session_cap(&mut self) -> bool {
        if !self.config.multi_session || self.session_manager.free_slots() > 0 {
            return false;
        }
        let limit = self.session_manager.max_sessions();
        self.chat_widget.add_error_message(format!(
            "{limit} sessions are open, the most `[tui] max_sessions` allows. Close one first: \
             select it in the session list (Left or /sessions) and press ctrl+d, or /close the \
             session on screen."
        ));
        true
    }

    fn session_status(&self, generation: SessionGeneration) -> Option<SessionStatus> {
        self.session_facts()
            .into_iter()
            .find(|fact| fact.generation == generation)
            .map(|fact| fact.status)
    }

    /// The nearest session after `from` that is still open.
    fn next_open_session(&self, from: SessionGeneration) -> Option<SessionGeneration> {
        let closed: Vec<SessionGeneration> = self
            .session_facts()
            .into_iter()
            .filter(|fact| fact.status == SessionStatus::Closed)
            .map(|fact| fact.generation)
            .collect();
        self.session_deck
            .next_where(from, Direction::Next, |generation| {
                !closed.contains(&generation)
            })
    }

    /// Shut down session `generation`'s agent process. The session stays
    /// listed as closed; if it was on screen, the next open session comes on
    /// screen. The last open session is not closed this way.
    pub(super) fn close_session(&mut self, generation: SessionGeneration) {
        let active = self.chat_widget.session_generation();
        let next = self.next_open_session(generation);
        if generation == active && next.is_none() {
            self.chat_widget.add_error_message(
                "This is the only open session. Use /quit to leave Nori.".to_string(),
            );
            return;
        }
        if self.session_status(generation) == Some(SessionStatus::Closed) {
            return;
        }
        let grace = if self.cloud_mode {
            std::time::Duration::from_secs(1)
        } else {
            std::time::Duration::ZERO
        };
        let widget: Option<&mut ChatWidget> = if generation == active {
            Some(&mut self.chat_widget)
        } else {
            self.session_deck
                .parked_mut(generation)
                .map(|parked| parked.widget.as_mut())
        };
        let Some(widget) = widget else {
            return;
        };
        widget.mark_closed_by_user();
        if let Some(id) = widget.managed_conversation_id() {
            let manager = self.session_manager.clone();
            tokio::spawn(async move {
                if let Err(error) = manager.close(&id, grace).await {
                    tracing::warn!(%error, "a session did not shut down cleanly");
                }
            });
        }
        if generation == active
            && let Some(next) = next
        {
            self.show_session(next);
        }
    }

    /// Drop a closed session from the deck and the session list.
    pub(super) fn remove_session(&mut self, generation: SessionGeneration) {
        if generation == self.chat_widget.session_generation()
            || self.session_status(generation) != Some(SessionStatus::Closed)
        {
            return;
        }
        self.session_deck.remove(generation);
    }

    /// A session released through `/close`. With another session open, that
    /// one comes on screen; otherwise the session picker opens, as it does
    /// with one session.
    pub(super) fn on_session_closed(
        &mut self,
        tui: &mut tui::Tui,
        generation: SessionGeneration,
    ) -> Result<bool> {
        let active = self.chat_widget.session_generation();
        if self.config.multi_session {
            if generation != active {
                return Ok(true);
            }
            if let Some(next) = self.next_open_session(generation) {
                self.show_session(next);
                tui.frame_requester().schedule_frame();
                return Ok(true);
            }
            self.retiring_transcript = Some(std::mem::take(&mut self.transcript_cells));
            self.session_reprint_pending = true;
        } else if generation != active {
            return Ok(true);
        }
        self.cancel_primary_agent_preparation();
        self.discard_candidate();
        self.pending_session_activation = None;
        if let Some(agent) = self.prepared_agent.take() {
            tokio::spawn(agent.shutdown());
        }
        self.prepared_agent_initial_context = None;
        let init = self.chat_widget_init(tui.frame_requester(), None, Vec::new(), None, true, None);
        self.install_active_widget(ChatWidget::new(init));
        self.configure_new_chat_widget();
        self.deferred_spawn_pending = true;
        self.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Picker {
            fallback_to_spawn: false,
        });
        tui.frame_requester().schedule_frame();
        Ok(true)
    }

    /// Run the next loop iteration of session `generation` in a fresh
    /// session that takes its place. The previous iteration's session shuts
    /// down. An iteration of a session in the background stays there.
    pub(super) fn run_loop_iteration(
        &mut self,
        tui: &mut tui::Tui,
        generation: SessionGeneration,
        prompt: String,
        remaining: i32,
        total: i32,
    ) {
        let iteration = total - remaining;
        tracing::info!("Loop iteration {iteration}/{total} (remaining: {remaining})");
        let note = format!("Loop iteration {iteration} of {total}");
        if generation != self.chat_widget.session_generation() {
            let Some(parked) = self.session_deck.parked(generation) else {
                return;
            };
            parked.widget.shutdown_harness_session();
            let agent = parked.widget.config_ref().active_agent.clone();
            let mut init = self.chat_widget_init(
                tui.frame_requester(),
                Some(prompt),
                Vec::new(),
                None,
                false,
                None,
            );
            init.config = self.config_for_agent(&agent);
            let mut widget = ChatWidget::new(init);
            widget.set_foreground(false);
            widget.set_loop_state(remaining, total);
            widget.add_info_message(note, None);
            let next = widget.session_generation();
            let transcript = self
                .session_deck
                .replace(generation, next)
                .map(|old| old.transcript)
                .unwrap_or_default();
            self.session_deck.park(
                next,
                ParkedSession {
                    widget: Box::new(widget),
                    transcript,
                },
            );
            return;
        }

        self.chat_widget.shutdown_harness_session();
        let init = self.chat_widget_init(
            tui.frame_requester(),
            Some(prompt),
            Vec::new(),
            None,
            true,
            None,
        );
        self.install_active_widget(ChatWidget::new(init));
        self.configure_new_chat_widget();
        self.chat_widget.set_loop_state(remaining, total);
        self.pending_session_activation = Some(PendingSessionActivation::New);
        self.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Idle);
        self.chat_widget.add_info_message(note, None);
    }
}
