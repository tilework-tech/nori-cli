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
const TITLE_CHARS: usize = 60;

/// One session as the overview sees it.
pub(crate) struct SessionFacts {
    pub(crate) generation: SessionGeneration,
    pub(crate) status: SessionStatus,
    pub(crate) family_status: SessionStatus,
    pub(crate) conversation_id: Option<ConversationId>,
    pub(crate) parent: Option<ConversationId>,
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
                let record = conversation_id.and_then(|id| self.session_manager.get(&id));
                let status = self.status_of(widget);
                Some(SessionFacts {
                    generation: *generation,
                    status,
                    family_status: conversation_id
                        .and_then(|id| self.session_manager.family_status(&id))
                        .unwrap_or(status),
                    conversation_id,
                    parent: record.and_then(|record| record.parent),
                    on_screen: *generation == active,
                })
            })
            .collect()
    }

    /// Rows for the session list: navigation order, with each session's
    /// children directly under it.
    pub(crate) fn session_rows(&self) -> Vec<SessionRow> {
        let facts = self.session_facts();
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

    fn session_row(&self, fact: &SessionFacts, depth: i32) -> SessionRow {
        let widget = self.widget_for(fact.generation);
        let agent_name = widget
            .map(|widget| widget.config_ref().active_agent.clone())
            .unwrap_or_default();
        let title = self
            .session_previews
            .title(fact.generation)
            .or_else(|| widget.and_then(ChatWidget::first_prompt_text))
            .map(|title| {
                let line = title.lines().next().unwrap_or_default().trim().to_string();
                if line.chars().count() > TITLE_CHARS {
                    let cut: String = line.chars().take(TITLE_CHARS - 1).collect();
                    format!("{cut}…")
                } else {
                    line
                }
            })
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| "New session".to_string());
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
            agent: self.agent_display_name(&agent_name),
            provider: Some(provider_kind(&agent_name)),
            status: fact.status,
            status_word: status_word(
                shown_status,
                self.unseen_finishes.is_unseen(fact.generation),
            ),
            family_status: shown_status,
            preview: self.session_previews.preview(fact.generation),
            depth,
            on_screen: fact.on_screen,
        }
    }

    fn agent_display_name(&self, agent_name: &str) -> String {
        if self.cloud_mode {
            return "Nori Cloud".to_string();
        }
        crate::nori::agent_picker::get_agent_info(agent_name)
            .map(|info| info.display_name)
            .unwrap_or_else(|| agent_name.to_string())
    }

    /// Open the session list with the active session selected.
    pub(super) fn open_session_list(&mut self) {
        if !self.config.multi_session {
            return;
        }
        let rows = self.session_rows();
        self.chat_widget.show_session_list(rows);
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
        if self.chat_widget.has_active_popup() {
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
    } else if name.contains("gemini") {
        ProviderKind::Gemini
    } else if name == "pi" || name.starts_with("pi-") {
        ProviderKind::Pi
    } else if name.contains("nori") {
        ProviderKind::Nori
    } else {
        ProviderKind::Other
    }
}
