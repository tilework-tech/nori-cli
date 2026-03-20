use std::any::TypeId;
use std::sync::Arc;

use crate::app::App;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::SessionInfoCell;
use crate::history_cell::UserHistoryCell;
use crate::pager_overlay::Overlay;
use crate::tui;
use crate::tui::TuiEvent;
use codex_core::protocol::ConversationPathResponseEvent;
use codex_protocol::ConversationId;
use color_eyre::eyre::Result;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;

/// Aggregates all backtrack-related state used by the App.
#[derive(Default)]
pub(crate) struct BacktrackState {
    /// True when Esc has primed backtrack mode in the main view.
    pub(crate) primed: bool,
    /// Session id of the base conversation to fork from.
    pub(crate) base_id: Option<ConversationId>,
    /// Index in the transcript of the last user message.
    pub(crate) nth_user_message: usize,
    /// True when the transcript overlay is showing a backtrack preview.
    pub(crate) overlay_preview_active: bool,
    /// Pending fork request: (base_id, nth_user_message, prefill).
    pub(crate) pending: Option<(ConversationId, usize, String)>,
}

impl App {
    /// Route overlay events when transcript overlay is active.
    /// - If backtrack preview is active: Esc steps selection; Enter confirms.
    /// - Otherwise: Esc begins preview; all other events forward to overlay.
    ///   interactions (Esc to step target, Enter to confirm) and overlay lifecycle.
    pub(crate) async fn handle_backtrack_overlay_event(
        &mut self,
        tui: &mut tui::Tui,
        event: TuiEvent,
    ) -> Result<bool> {
        if self.backtrack.overlay_preview_active {
            match event {
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Esc,
                    kind: KeyEventKind::Press | KeyEventKind::Repeat,
                    ..
                }) => {
                    self.overlay_step_backtrack(tui, event)?;
                    Ok(true)
                }
                TuiEvent::Key(KeyEvent {
                    code: KeyCode::Enter,
                    kind: KeyEventKind::Press,
                    ..
                }) => {
                    self.overlay_confirm_backtrack(tui);
                    Ok(true)
                }
                // Catchall: forward any other events to the overlay widget.
                _ => {
                    self.overlay_forward_event(tui, event)?;
                    Ok(true)
                }
            }
        } else if let TuiEvent::Key(KeyEvent {
            code: KeyCode::Esc,
            kind: KeyEventKind::Press | KeyEventKind::Repeat,
            ..
        }) = event
        {
            // First Esc in transcript overlay: begin backtrack preview at latest user message.
            self.begin_overlay_backtrack_preview(tui);
            Ok(true)
        } else {
            // Not in backtrack mode: forward events to the overlay widget.
            self.overlay_forward_event(tui, event)?;
            Ok(true)
        }
    }

    /// Handle global Esc presses for backtracking when no overlay is present.
    pub(crate) fn handle_backtrack_esc_key(&mut self, tui: &mut tui::Tui) {
        if !self.chat_widget.composer_is_empty() {
            return;
        }

        if !self.backtrack.primed {
            self.prime_backtrack();
        } else if self.overlay.is_none() {
            self.open_backtrack_preview(tui);
        } else if self.backtrack.overlay_preview_active {
            self.step_backtrack_and_highlight(tui);
        }
    }

    /// Stage a backtrack and request conversation history from the agent.
    pub(crate) fn request_backtrack(
        &mut self,
        prefill: String,
        base_id: ConversationId,
        nth_user_message: usize,
    ) {
        self.backtrack.pending = Some((base_id, nth_user_message, prefill));
        if let Some(path) = self.chat_widget.rollout_path() {
            let ev = ConversationPathResponseEvent {
                conversation_id: base_id,
                path,
            };
            self.app_event_tx
                .send(crate::app_event::AppEvent::ConversationHistory(ev));
        } else {
            tracing::error!("rollout path unavailable; cannot backtrack");
        }
    }

    /// Open transcript overlay (enters alternate screen and shows full transcript).
    pub(crate) fn open_transcript_overlay(&mut self, tui: &mut tui::Tui) {
        let _ = tui.enter_alt_screen();
        self.overlay = Some(Overlay::new_transcript(self.transcript_cells.clone()));
        tui.frame_requester().schedule_frame();
    }

    /// Close transcript overlay and restore normal UI.
    pub(crate) fn close_transcript_overlay(&mut self, tui: &mut tui::Tui) {
        let _ = tui.leave_alt_screen();
        let was_backtrack = self.backtrack.overlay_preview_active;
        if !self.deferred_history_lines.is_empty() {
            let lines = std::mem::take(&mut self.deferred_history_lines);
            tui.insert_history_lines(lines);
        }
        self.overlay = None;
        self.backtrack.overlay_preview_active = false;
        if was_backtrack {
            // Ensure backtrack state is fully reset when overlay closes (e.g. via 'q').
            self.reset_backtrack_state();
        }
    }

    /// Re-render the full transcript into the terminal scrollback in one call.
    /// Useful when switching sessions to ensure prior history remains visible.
    pub(crate) fn render_transcript_once(&mut self, tui: &mut tui::Tui) {
        if !self.transcript_cells.is_empty() {
            let width = tui.terminal.last_known_screen_size.width;
            for cell in &self.transcript_cells {
                tui.insert_history_lines(cell.display_lines(width));
            }
        }
    }

    /// Initialize backtrack state and show composer hint.
    fn prime_backtrack(&mut self) {
        self.backtrack.primed = true;
        self.backtrack.nth_user_message = usize::MAX;
        self.backtrack.base_id = self.chat_widget.conversation_id();
        self.chat_widget.show_esc_backtrack_hint();
    }

    /// Open overlay and begin backtrack preview flow (first step + highlight).
    fn open_backtrack_preview(&mut self, tui: &mut tui::Tui) {
        self.open_transcript_overlay(tui);
        self.backtrack.overlay_preview_active = true;
        // Composer is hidden by overlay; clear its hint.
        self.chat_widget.clear_esc_backtrack_hint();
        self.step_backtrack_and_highlight(tui);
    }

    /// When overlay is already open, begin preview mode and select latest user message.
    fn begin_overlay_backtrack_preview(&mut self, tui: &mut tui::Tui) {
        self.backtrack.primed = true;
        self.backtrack.base_id = self.chat_widget.conversation_id();
        self.backtrack.overlay_preview_active = true;
        let count = user_count(&self.transcript_cells);
        if let Some(last) = count.checked_sub(1) {
            self.apply_backtrack_selection(last);
        }
        tui.frame_requester().schedule_frame();
    }

    /// Step selection to the next older user message and update overlay.
    fn step_backtrack_and_highlight(&mut self, tui: &mut tui::Tui) {
        let count = user_count(&self.transcript_cells);
        if count == 0 {
            return;
        }

        let last_index = count.saturating_sub(1);
        let next_selection = if self.backtrack.nth_user_message == usize::MAX {
            last_index
        } else if self.backtrack.nth_user_message == 0 {
            0
        } else {
            self.backtrack
                .nth_user_message
                .saturating_sub(1)
                .min(last_index)
        };

        self.apply_backtrack_selection(next_selection);
        tui.frame_requester().schedule_frame();
    }

    /// Apply a computed backtrack selection to the overlay and internal counter.
    fn apply_backtrack_selection(&mut self, nth_user_message: usize) {
        if let Some(cell_idx) = nth_user_position(&self.transcript_cells, nth_user_message) {
            self.backtrack.nth_user_message = nth_user_message;
            if let Some(Overlay::Transcript(t)) = &mut self.overlay {
                t.set_highlight_cell(Some(cell_idx));
            }
        } else {
            self.backtrack.nth_user_message = usize::MAX;
            if let Some(Overlay::Transcript(t)) = &mut self.overlay {
                t.set_highlight_cell(None);
            }
        }
    }

    /// Forward any event to the overlay and close it if done.
    fn overlay_forward_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        if let Some(overlay) = &mut self.overlay {
            overlay.handle_event(tui, event)?;
            if overlay.is_done() {
                self.close_transcript_overlay(tui);
                tui.frame_requester().schedule_frame();
            }
        }
        Ok(())
    }

    /// Handle Enter in overlay backtrack preview: confirm selection and reset state.
    fn overlay_confirm_backtrack(&mut self, tui: &mut tui::Tui) {
        let nth_user_message = self.backtrack.nth_user_message;
        if let Some(base_id) = self.backtrack.base_id {
            let prefill = nth_user_position(&self.transcript_cells, nth_user_message)
                .and_then(|idx| self.transcript_cells.get(idx))
                .and_then(|cell| cell.as_any().downcast_ref::<UserHistoryCell>())
                .map(|c| c.message.clone())
                .unwrap_or_default();
            self.close_transcript_overlay(tui);
            self.request_backtrack(prefill, base_id, nth_user_message);
        }
        self.reset_backtrack_state();
    }

    /// Handle Esc in overlay backtrack preview: step selection if armed, else forward.
    fn overlay_step_backtrack(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        if self.backtrack.base_id.is_some() {
            self.step_backtrack_and_highlight(tui);
        } else {
            self.overlay_forward_event(tui, event)?;
        }
        Ok(())
    }

    /// Confirm a primed backtrack from the main view (no overlay visible).
    /// Computes the prefill from the selected user message and requests history.
    pub(crate) fn confirm_backtrack_from_main(&mut self) {
        if let Some(base_id) = self.backtrack.base_id {
            let prefill =
                nth_user_position(&self.transcript_cells, self.backtrack.nth_user_message)
                    .and_then(|idx| self.transcript_cells.get(idx))
                    .and_then(|cell| cell.as_any().downcast_ref::<UserHistoryCell>())
                    .map(|c| c.message.clone())
                    .unwrap_or_default();
            self.request_backtrack(prefill, base_id, self.backtrack.nth_user_message);
        }
        self.reset_backtrack_state();
    }

    /// Clear all backtrack-related state and composer hints.
    pub(crate) fn reset_backtrack_state(&mut self) {
        self.backtrack.primed = false;
        self.backtrack.base_id = None;
        self.backtrack.nth_user_message = usize::MAX;
        // In case a hint is somehow still visible (e.g., race with overlay open/close).
        self.chat_widget.clear_esc_backtrack_hint();
    }

    /// Handle a ConversationHistory response while a backtrack is pending.
    /// If it matches the primed base session, fork and switch to the new conversation.
    pub(crate) fn on_conversation_history_for_backtrack(
        &mut self,
        tui: &mut tui::Tui,
        ev: ConversationPathResponseEvent,
    ) -> Result<()> {
        if let Some((base_id, _, _)) = self.backtrack.pending.as_ref()
            && ev.conversation_id == *base_id
            && let Some((_, nth_user_message, prefill)) = self.backtrack.pending.take()
        {
            self.fork_and_switch_to_new_conversation(tui, ev, nth_user_message, prefill);
        }
        Ok(())
    }

    /// Fork the conversation using provided history and switch UI/state accordingly.
    ///
    /// Builds a summary of prior conversation turns up to the selected user
    /// message, then spawns a fresh ACP session with the summary injected as
    /// context (via `fork_context`).
    fn fork_and_switch_to_new_conversation(
        &mut self,
        tui: &mut tui::Tui,
        _ev: ConversationPathResponseEvent,
        nth_user_message: usize,
        prefill: String,
    ) {
        let cfg = self.chat_widget.config_ref().clone();

        // Build a plain-text summary of the conversation prior to the
        // selected user message to inject as context into the new session.
        let cell_index = nth_user_position(&self.transcript_cells, nth_user_message)
            .unwrap_or(self.transcript_cells.len());
        let fork_summary = build_fork_summary(&self.transcript_cells, cell_index);
        let fork_context = if fork_summary.is_empty() {
            None
        } else {
            Some(fork_summary)
        };

        let init = crate::chatwidget::ChatWidgetInit {
            config: cfg,
            frame_requester: tui.frame_requester(),
            app_event_tx: self.app_event_tx.clone(),
            initial_prompt: None,
            initial_images: Vec::new(),
            enhanced_keys_supported: self.enhanced_keys_supported,
            auth_manager: self.auth_manager.clone(),
            vertical_footer: self.vertical_footer,
            expected_agent: None,
            deferred_spawn: false,
            fork_context,
        };
        self.chat_widget = crate::chatwidget::ChatWidget::new(init);
        self.chat_widget
            .set_hotkey_config(self.hotkey_config.clone());
        // Trim transcript up to the selected user message and re-render it.
        self.trim_transcript_for_backtrack(nth_user_message);
        self.render_transcript_once(tui);
        if !prefill.is_empty() {
            self.chat_widget.set_composer_text(prefill);
        }
        tui.frame_requester().schedule_frame();
    }

    /// Trim transcript_cells to preserve only content up to the selected user message.
    fn trim_transcript_for_backtrack(&mut self, nth_user_message: usize) {
        trim_transcript_cells_to_nth_user(&mut self.transcript_cells, nth_user_message);
    }
}

pub(crate) fn trim_transcript_cells_to_nth_user(
    transcript_cells: &mut Vec<Arc<dyn crate::history_cell::HistoryCell>>,
    nth_user_message: usize,
) {
    if nth_user_message == usize::MAX {
        return;
    }

    if let Some(cut_idx) = nth_user_position(transcript_cells, nth_user_message) {
        transcript_cells.truncate(cut_idx);
    }
}

pub(crate) fn user_count(cells: &[Arc<dyn crate::history_cell::HistoryCell>]) -> usize {
    user_positions_iter(cells).count()
}

/// Collect ALL user messages from the entire transcript (across all session segments).
///
/// Returns a list of `(cell_index, message_text)` tuples in chronological
/// order (oldest first). The `cell_index` is the position in the `cells` slice.
pub(crate) fn collect_all_user_messages(
    cells: &[Arc<dyn crate::history_cell::HistoryCell>],
) -> Vec<(usize, String)> {
    let user_type = TypeId::of::<UserHistoryCell>();
    cells
        .iter()
        .enumerate()
        .filter_map(|(idx, cell)| {
            if cell.as_any().type_id() == user_type {
                cell.as_any()
                    .downcast_ref::<UserHistoryCell>()
                    .map(|c| (idx, c.message.clone()))
            } else {
                None
            }
        })
        .collect()
}

/// Build a plain-text summary of the conversation up to (but not including)
/// the cell at `cell_index`. This summary is injected via `pending_compact_summary`
/// into a fresh ACP session so the agent has prior context.
///
/// All user and assistant messages before `cell_index` are included, regardless
/// of session boundaries. `SessionInfoCell` markers are skipped.
///
/// Format:
/// ```text
/// User: <message>
/// Assistant: <text from display lines>
/// ```
pub(crate) fn build_fork_summary(
    cells: &[Arc<dyn crate::history_cell::HistoryCell>],
    cell_index: usize,
) -> String {
    let cut_idx = cell_index.min(cells.len());
    let mut summary = String::new();

    for cell in &cells[..cut_idx] {
        let any = cell.as_any();
        if let Some(user) = any.downcast_ref::<UserHistoryCell>() {
            summary.push_str(&format!("User: {}\n", user.message));
        } else if any.downcast_ref::<AgentMessageCell>().is_some() {
            // Extract plain text from the display lines
            let lines = cell.display_lines(u16::MAX);
            let text: String = lines
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|s| s.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                summary.push_str(&format!("Assistant: {trimmed}\n"));
            }
        }
    }

    summary
}

pub(crate) fn nth_user_position(
    cells: &[Arc<dyn crate::history_cell::HistoryCell>],
    nth: usize,
) -> Option<usize> {
    user_positions_iter(cells)
        .enumerate()
        .find_map(|(i, idx)| (i == nth).then_some(idx))
}

fn user_positions_iter(
    cells: &[Arc<dyn crate::history_cell::HistoryCell>],
) -> impl Iterator<Item = usize> + '_ {
    let session_start_type = TypeId::of::<SessionInfoCell>();
    let user_type = TypeId::of::<UserHistoryCell>();
    let type_of = |cell: &Arc<dyn crate::history_cell::HistoryCell>| cell.as_any().type_id();

    let start = cells
        .iter()
        .rposition(|cell| type_of(cell) == session_start_type)
        .map_or(0, |idx| idx + 1);

    cells
        .iter()
        .enumerate()
        .skip(start)
        .filter_map(move |(idx, cell)| (type_of(cell) == user_type).then_some(idx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_cell::AgentMessageCell;
    use crate::history_cell::HistoryCell;
    use ratatui::prelude::Line;
    use std::sync::Arc;

    #[test]
    fn trim_transcript_for_first_user_drops_user_and_newer_cells() {
        let mut cells: Vec<Arc<dyn HistoryCell>> = vec![
            Arc::new(UserHistoryCell {
                message: "first user".to_string(),
            }) as Arc<dyn HistoryCell>,
            Arc::new(AgentMessageCell::new(vec![Line::from("assistant")], true))
                as Arc<dyn HistoryCell>,
        ];
        trim_transcript_cells_to_nth_user(&mut cells, 0);

        assert!(cells.is_empty());
    }

    #[test]
    fn trim_transcript_preserves_cells_before_selected_user() {
        let mut cells: Vec<Arc<dyn HistoryCell>> = vec![
            Arc::new(AgentMessageCell::new(vec![Line::from("intro")], true))
                as Arc<dyn HistoryCell>,
            Arc::new(UserHistoryCell {
                message: "first".to_string(),
            }) as Arc<dyn HistoryCell>,
            Arc::new(AgentMessageCell::new(vec![Line::from("after")], false))
                as Arc<dyn HistoryCell>,
        ];
        trim_transcript_cells_to_nth_user(&mut cells, 0);

        assert_eq!(cells.len(), 1);
        let agent = cells[0]
            .as_any()
            .downcast_ref::<AgentMessageCell>()
            .expect("agent cell");
        let agent_lines = agent.display_lines(u16::MAX);
        assert_eq!(agent_lines.len(), 1);
        let intro_text: String = agent_lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(intro_text, "• intro");
    }

    #[test]
    fn trim_transcript_for_later_user_keeps_prior_history() {
        let mut cells: Vec<Arc<dyn HistoryCell>> = vec![
            Arc::new(AgentMessageCell::new(vec![Line::from("intro")], true))
                as Arc<dyn HistoryCell>,
            Arc::new(UserHistoryCell {
                message: "first".to_string(),
            }) as Arc<dyn HistoryCell>,
            Arc::new(AgentMessageCell::new(vec![Line::from("between")], false))
                as Arc<dyn HistoryCell>,
            Arc::new(UserHistoryCell {
                message: "second".to_string(),
            }) as Arc<dyn HistoryCell>,
            Arc::new(AgentMessageCell::new(vec![Line::from("tail")], false))
                as Arc<dyn HistoryCell>,
        ];
        trim_transcript_cells_to_nth_user(&mut cells, 1);

        assert_eq!(cells.len(), 3);
        let agent_intro = cells[0]
            .as_any()
            .downcast_ref::<AgentMessageCell>()
            .expect("intro agent");
        let intro_lines = agent_intro.display_lines(u16::MAX);
        let intro_text: String = intro_lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(intro_text, "• intro");

        let user_first = cells[1]
            .as_any()
            .downcast_ref::<UserHistoryCell>()
            .expect("first user");
        assert_eq!(user_first.message, "first");

        let agent_between = cells[2]
            .as_any()
            .downcast_ref::<AgentMessageCell>()
            .expect("between agent");
        let between_lines = agent_between.display_lines(u16::MAX);
        let between_text: String = between_lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(between_text, "  between");
    }

    #[test]
    fn collect_all_user_messages_spans_session_boundaries() {
        let cells: Vec<Arc<dyn HistoryCell>> = vec![
            Arc::new(UserHistoryCell {
                message: "before".to_string(),
            }) as Arc<dyn HistoryCell>,
            Arc::new(AgentMessageCell::new(vec![Line::from("reply")], true))
                as Arc<dyn HistoryCell>,
            Arc::new(SessionInfoCell::new(
                crate::history_cell::CompositeHistoryCell::new(vec![]),
            )) as Arc<dyn HistoryCell>,
            Arc::new(UserHistoryCell {
                message: "after".to_string(),
            }) as Arc<dyn HistoryCell>,
        ];
        let messages = collect_all_user_messages(&cells);
        assert_eq!(
            messages,
            vec![(0, "before".to_string()), (3, "after".to_string()),]
        );
    }

    #[test]
    fn collect_all_user_messages_empty_transcript() {
        let cells: Vec<Arc<dyn HistoryCell>> = vec![];
        let messages = collect_all_user_messages(&cells);
        assert!(messages.is_empty());
    }

    #[test]
    fn build_fork_summary_includes_content_before_cell_index() {
        let cells: Vec<Arc<dyn HistoryCell>> = vec![
            Arc::new(UserHistoryCell {
                message: "hello".to_string(),
            }) as Arc<dyn HistoryCell>,
            Arc::new(AgentMessageCell::new(vec![Line::from("world")], true))
                as Arc<dyn HistoryCell>,
            Arc::new(UserHistoryCell {
                message: "goodbye".to_string(),
            }) as Arc<dyn HistoryCell>,
        ];
        // cell_index=2 is the "goodbye" user cell
        let summary = build_fork_summary(&cells, 2);
        assert!(summary.contains("User: hello"));
        assert!(summary.contains("Assistant:"));
        assert!(!summary.contains("goodbye"));
    }

    #[test]
    fn build_fork_summary_at_first_cell_is_empty() {
        let cells: Vec<Arc<dyn HistoryCell>> = vec![Arc::new(UserHistoryCell {
            message: "hello".to_string(),
        }) as Arc<dyn HistoryCell>];
        let summary = build_fork_summary(&cells, 0);
        assert!(summary.is_empty());
    }

    #[test]
    fn build_fork_summary_beyond_end_includes_everything() {
        let cells: Vec<Arc<dyn HistoryCell>> = vec![
            Arc::new(UserHistoryCell {
                message: "hello".to_string(),
            }) as Arc<dyn HistoryCell>,
            Arc::new(AgentMessageCell::new(vec![Line::from("reply")], true))
                as Arc<dyn HistoryCell>,
        ];
        // cell_index=99 is beyond the number of cells, so include everything
        let summary = build_fork_summary(&cells, 99);
        assert!(summary.contains("User: hello"));
        assert!(summary.contains("Assistant:"));
    }

    #[test]
    fn build_fork_summary_spans_session_boundaries() {
        let cells: Vec<Arc<dyn HistoryCell>> = vec![
            Arc::new(UserHistoryCell {
                message: "before session".to_string(),
            }) as Arc<dyn HistoryCell>,
            Arc::new(AgentMessageCell::new(vec![Line::from("reply1")], true))
                as Arc<dyn HistoryCell>,
            Arc::new(SessionInfoCell::new(
                crate::history_cell::CompositeHistoryCell::new(vec![]),
            )) as Arc<dyn HistoryCell>,
            Arc::new(UserHistoryCell {
                message: "after session".to_string(),
            }) as Arc<dyn HistoryCell>,
            Arc::new(AgentMessageCell::new(vec![Line::from("reply2")], true))
                as Arc<dyn HistoryCell>,
            Arc::new(UserHistoryCell {
                message: "target".to_string(),
            }) as Arc<dyn HistoryCell>,
        ];
        // cell_index=5 is the "target" user cell
        let summary = build_fork_summary(&cells, 5);
        assert!(summary.contains("User: before session"));
        assert!(summary.contains("User: after session"));
        assert!(!summary.contains("target"));
    }
}
