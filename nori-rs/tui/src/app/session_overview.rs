//! What the TUI shows about every session it holds: the status line now, and
//! the session list and background-request line built from the same facts.

use std::collections::HashMap;

use nori_harness::ConversationId;
use nori_protocol::SessionStatus;
use nori_tui_components::ProviderKind;
use ratatui::style::Stylize;
use ratatui::text::Line;

use super::App;
use crate::app_event::SessionGeneration;
use crate::bottom_pane::SessionRow;
use crate::chatwidget::ChatWidget;
use crate::session_status::status_summary;
use crate::session_status::status_word;

/// Longest title shown for a session that has no summary yet.
const TITLE_CHARS: i32 = 60;
/// For lines that fit names to the terminal width themselves.
pub(super) const WHOLE_NAME: i32 = i32::MAX;

fn cut(text: &str, limit: i32) -> String {
    nori_protocol::shorten_name(text, limit)
}

/// One session as the overview sees it.
pub(crate) struct SessionFacts {
    pub(crate) generation: SessionGeneration,
    pub(crate) status: SessionStatus,
    pub(crate) family_status: SessionStatus,
    pub(crate) conversation_id: Option<ConversationId>,
    pub(crate) parent: Option<ConversationId>,
    pub(crate) agent: String,
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
                let conversation_id = widget.managed_conversation_id();
                let on_screen = *generation == active;
                // A blank session that has not started is not counted.
                if conversation_id.is_none()
                    && !widget.has_harness_session()
                    && !(on_screen && self.session_setup_pending())
                {
                    return None;
                }
                let record = conversation_id.and_then(|id| self.session_manager.get(&id));
                let status = self.status_of(widget);
                Some(SessionFacts {
                    generation: *generation,
                    status,
                    family_status: conversation_id
                        .and_then(|id| self.session_manager.family_status(&id))
                        .unwrap_or(status),
                    conversation_id,
                    parent: record.as_ref().and_then(|record| record.parent),
                    agent: widget.config_ref().active_agent.clone(),
                    on_screen,
                })
            })
            .collect()
    }

    /// Rows for the session list: navigation order, with each session's
    /// children directly under it.
    pub(crate) fn session_rows(&self) -> Vec<SessionRow> {
        let mut facts = self.session_facts();
        facts.extend(self.unviewed_descendants(&facts));
        let by_conversation: HashMap<ConversationId, SessionGeneration> = facts
            .iter()
            .filter_map(|fact| Some((fact.conversation_id?, fact.generation)))
            .collect();
        let parent_of = |fact: &SessionFacts| {
            fact.parent
                .and_then(|parent| by_conversation.get(&parent).copied())
        };
        let mut rows = Vec::with_capacity(facts.len());
        let mut stack: Vec<(SessionGeneration, i32)> = facts
            .iter()
            .rev()
            .filter(|fact| parent_of(fact).is_none())
            .map(|fact| (fact.generation, 0))
            .collect();
        while let Some((generation, depth)) = stack.pop() {
            let Some(fact) = facts.iter().find(|fact| fact.generation == generation) else {
                continue;
            };
            rows.push(self.session_row(fact, depth));
            stack.extend(
                facts
                    .iter()
                    .rev()
                    .filter(|child| parent_of(child) == Some(generation))
                    .map(|child| (child.generation, depth + 1)),
            );
        }
        rows
    }

    /// The title a session is listed under: the first line of the agent's
    /// summary, else of the first prompt.
    fn session_full_title(&self, generation: SessionGeneration) -> Option<String> {
        let widget = self.widget_for(generation);
        self.session_previews
            .title(generation)
            .or_else(|| widget.and_then(ChatWidget::first_prompt_text))
            .map(|title| title.lines().next().unwrap_or_default().trim().to_owned())
            .filter(|title| !title.is_empty())
    }

    /// The session's title cut to `limit` characters.
    pub(super) fn session_title(
        &self,
        generation: SessionGeneration,
        limit: i32,
    ) -> Option<String> {
        self.session_full_title(generation)
            .map(|title| cut(&title, limit))
    }

    /// The list cuts titles to its column width itself.
    pub(super) fn session_rows_title(&self, generation: SessionGeneration) -> Option<String> {
        self.session_full_title(generation)
    }

    /// Sessions in the manager that descend from a session this TUI shows but
    /// have no view here, such as sessions another session started. They are
    /// listed under their parent; their keys are negative so they never
    /// collide with a session generation.
    fn unviewed_descendants(&self, shown: &[SessionFacts]) -> Vec<SessionFacts> {
        let mut known: Vec<ConversationId> = shown
            .iter()
            .filter_map(|fact| fact.conversation_id)
            .collect();
        let records = self.session_manager.list();
        let mut extra = Vec::new();
        loop {
            let found: Vec<_> = records
                .iter()
                .filter(|record| !known.contains(&record.conversation_id))
                .filter(|record| !self.adopted_delegations.contains(&record.conversation_id))
                .filter(|record| record.parent.is_some_and(|parent| known.contains(&parent)))
                .collect();
            if found.is_empty() {
                return extra;
            }
            for record in found {
                known.push(record.conversation_id);
                let key = -i64::try_from(extra.len() + 1).unwrap_or(i64::MAX);
                extra.push(SessionFacts {
                    generation: key,
                    status: record.status,
                    family_status: record.status,
                    conversation_id: Some(record.conversation_id),
                    parent: record.parent,
                    agent: record.agent.clone(),
                    on_screen: false,
                });
            }
        }
    }

    fn session_row(&self, fact: &SessionFacts, depth: i32) -> SessionRow {
        let agent_name = fact.agent.clone();
        let widget = self.widget_for(fact.generation);
        let title = self
            .session_rows_title(fact.generation)
            .unwrap_or_else(|| nori_protocol::UNTITLED_SESSION_NAME.to_string());
        // A parent speaks for its family: if a child needs the user, so does
        // the parent row.
        let shown_status = if depth == 0 {
            fact.family_status
        } else {
            fact.status
        };
        SessionRow {
            generation: fact.generation,
            title,
            agent: self.agent_label(&agent_name),
            provider: Some(provider_kind(&agent_name)),
            status: fact.status,
            status_word: status_word(
                shown_status,
                self.unseen_finishes.is_unseen(fact.generation),
            ),
            family_status: shown_status,
            // A session waiting on the user shows what it asks for.
            reply: (fact.status == SessionStatus::WaitingOnYou)
                .then(|| self.session_previews.pending_request(fact.generation))
                .flatten()
                .or_else(|| self.session_previews.reply(fact.generation)),
            model: widget.and_then(ChatWidget::model_name),
            idle_seconds: self
                .session_previews
                .last_activity(fact.generation)
                .map(|at| i64::try_from(at.elapsed().as_secs()).unwrap_or(i64::MAX)),
            context: widget.and_then(ChatWidget::context_summary),
            depth,
            on_screen: fact.on_screen,
        }
    }

    /// Sessions name their agent by the slug `/delegate` takes.
    fn agent_label(&self, agent_name: &str) -> String {
        if self.cloud_mode {
            return "Nori Cloud".to_string();
        }
        agent_name.to_string()
    }

    fn agent_display_name(&self, agent_name: &str) -> String {
        if self.cloud_mode {
            return "Nori Cloud".to_string();
        }
        self.agent_display_names
            .borrow_mut()
            .entry(agent_name.to_string())
            .or_insert_with(|| {
                crate::nori::agent_picker::get_agent_info(agent_name)
                    .map(|info| info.display_name)
                    .unwrap_or_else(|| agent_name.to_string())
            })
            .clone()
    }

    /// Open the session list with the active session selected.
    pub(super) fn open_session_list(&mut self) {
        let rows = self.session_rows();
        self.chat_widget.show_session_list(rows);
    }

    /// Refresh the lines about other sessions on the active widget. Runs
    /// before every draw; with one session nothing is shown.
    /// `width` is the terminal width; the lines are fitted to one row each.
    pub(super) fn refresh_session_overview(&mut self, width: u16) {
        let facts = self.session_facts();
        for fact in &facts {
            self.unseen_finishes
                .observe(fact.generation, fact.status, fact.on_screen);
        }
        let mut lines = Vec::new();
        if let Some(super::CandidateAgent::Preparing { agent_name, .. }) = &self.candidate_agent {
            let name = self.agent_display_name(agent_name);
            lines.push(Line::from(format!("Preparing {name}…").dim()));
        }
        // Session lines sit two cells in from the terminal edge.
        let width = width.saturating_sub(2);
        lines.extend(self.parent_breadcrumb(&facts, width));
        lines.extend(self.background_request_line(&facts, width));
        // The open list's header carries the summary.
        if facts.len() > 1
            && !self.chat_widget.session_list_in_front()
            && let Some(summary) = status_summary(facts.iter().map(|fact| fact.status))
        {
            lines.push(Line::from(summary.dim()));
        }
        self.chat_widget.set_session_lines(lines);
        if self.chat_widget.session_list_in_front() {
            let rows = self.session_rows();
            self.chat_widget.update_session_list(&rows);
        }
    }
}

fn provider_kind(agent_name: &str) -> ProviderKind {
    let name = agent_name.to_lowercase();
    if name.contains("claude") {
        ProviderKind::Claude
    } else if name.contains("codex") {
        ProviderKind::Codex
    } else if name.contains("gemini") || matches!(name.as_str(), "antigravity" | "agy") {
        ProviderKind::Antigravity
    } else if name == "pi" || name.starts_with("pi-") {
        ProviderKind::Pi
    } else if name.contains("nori") {
        ProviderKind::Nori
    } else {
        ProviderKind::Other
    }
}

impl App {
    /// Post the notifications parked sessions asked for, naming the session.
    /// Only top-level sessions notify; a child's news reaches the user
    /// through its parent. The terminal shows them only while unfocused.
    pub(super) fn post_parked_session_notifications(&mut self, tui: &mut crate::tui::Tui) {
        let mut pending = Vec::new();
        for (generation, parked) in self.session_deck.parked_sessions_mut() {
            if let Some(message) = parked.widget.take_pending_notification() {
                let parent = parked
                    .widget
                    .managed_conversation_id()
                    .and_then(|id| self.session_manager.get(&id))
                    .and_then(|record| record.parent);
                if parent.is_none() {
                    pending.push((generation, message));
                }
            }
        }
        for (generation, message) in pending {
            let title = self
                .session_title(generation, TITLE_CHARS)
                .unwrap_or_else(|| "A session".to_string());
            tui.notify(format!("{title}: {message}"));
        }
    }

    /// Bring on screen the next session, after the active one in navigation
    /// order, that is waiting on the user.
    /// Returns whether a session was brought on screen.
    pub(super) fn show_next_waiting_session(&mut self) -> bool {
        let waiting: Vec<SessionGeneration> = self
            .session_facts()
            .into_iter()
            .filter(|fact| fact.status == SessionStatus::WaitingOnYou)
            .map(|fact| fact.generation)
            .collect();
        let current = self.chat_widget.session_generation();
        if let Some(target) = self.session_deck.next_where(
            current,
            super::session_deck::Direction::Next,
            |generation| waiting.contains(&generation),
        ) {
            self.show_session(target);
            return true;
        }
        false
    }

    /// The line naming a session in the background that waits on the user,
    /// with the key that jumps to it. A child is named under its parent.
    fn background_request_line(&self, facts: &[SessionFacts], width: u16) -> Option<Line<'static>> {
        let waiting: Vec<&SessionFacts> = facts
            .iter()
            .filter(|fact| !fact.on_screen && fact.status == SessionStatus::WaitingOnYou)
            .collect();
        let first = waiting.first()?;
        let title = self
            .session_title(first.generation, WHOLE_NAME)
            .unwrap_or_else(|| nori_protocol::UNTITLED_SESSION_NAME.to_string());
        let mut path = Vec::new();
        if let Some(parent) = first.parent {
            path.push(self.session_name(&parent, WHOLE_NAME));
        }
        path.push(title);
        let request = self.session_previews.pending_request(first.generation);
        let binding = self
            .hotkey_config
            .binding_for(nori_config::HotkeyAction::NextWaitingSession);
        let key = (!binding.is_none()).then(|| binding.display_name());
        Some(crate::session_path::waiting_line(
            &path,
            request.as_deref(),
            waiting.len() - 1,
            key.as_deref(),
            width,
        ))
    }

    /// A readable name for any session the manager knows: its title in this
    /// TUI, else its recorded title, else a placeholder.
    pub(super) fn session_name(&self, id: &ConversationId, limit: i32) -> String {
        self.session_facts()
            .iter()
            .find(|fact| fact.conversation_id.as_ref() == Some(id))
            .and_then(|fact| self.session_title(fact.generation, limit))
            .or_else(|| {
                self.session_manager
                    .replay_info(id)
                    .and_then(|info| info.title)
                    .and_then(|title| nori_protocol::session_full_name(&title))
                    .map(|title| cut(&title, limit))
            })
            .unwrap_or_else(|| nori_protocol::UNTITLED_SESSION_NAME.to_string())
    }

    /// The parent of the delegated child on screen, if any.
    pub(super) fn on_screen_parent(&self) -> Option<ConversationId> {
        self.chat_widget
            .managed_conversation_id()
            .and_then(|id| self.session_manager.get(&id))
            .filter(|record| record.source == nori_harness::session_manager::SessionSource::Session)
            .and_then(|record| record.parent)
    }

    /// Where a delegated child sits, from its top-level session down, with
    /// the key that returns to its parent.
    fn parent_breadcrumb(&self, facts: &[SessionFacts], width: u16) -> Option<Line<'static>> {
        self.on_screen_parent()?;
        let child = facts.iter().find(|fact| fact.on_screen)?;
        let mut path = vec![crate::session_path::PathStep {
            agent: child.agent.clone(),
            name: self
                .session_title(child.generation, WHOLE_NAME)
                .unwrap_or_else(|| nori_protocol::UNTITLED_SESSION_NAME.to_string()),
        }];
        let mut record = child
            .conversation_id
            .and_then(|id| self.session_manager.get(&id));
        let mut seen = Vec::new();
        while let Some(current) = record
            && current.source == nori_harness::session_manager::SessionSource::Session
            && let Some(parent) = current.parent
            && !seen.contains(&parent)
            && let Some(parent_record) = self.session_manager.get(&parent)
        {
            seen.push(parent);
            path.push(crate::session_path::PathStep {
                agent: parent_record.agent.clone(),
                name: self.session_name(&parent, WHOLE_NAME),
            });
            record = Some(parent_record);
        }
        path.reverse();
        let binding = self
            .hotkey_config
            .binding_for(nori_config::HotkeyAction::ParentSession);
        let tail = if binding.is_none() {
            String::new()
        } else {
            format!(" · {} to parent", binding.display_name())
        };
        Some(crate::session_path::breadcrumb_line(
            &path, "", &tail, width,
        ))
    }
}
