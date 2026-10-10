//! Assemble a context snapshot without sending a prompt to the agent.

use super::ChatWidget;
use crate::context_view::ContextView;
use crate::nori::session_header::format_instruction_path;
use crate::nori::session_header::local_instruction_files;

impl ChatWidget {
    pub(crate) fn add_context_output(&mut self) {
        let Some(handle) = self.harness_handle.clone() else {
            self.insert_context_output(Default::default());
            return;
        };
        let generation = self.session_generation;
        let conversation_id = self.conversation_id;
        let app_event_tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            let result = handle
                .context_timeline()
                .await
                .map_err(|error| error.to_string());
            app_event_tx.send(crate::app_event::AppEvent::ContextTimelineReady {
                generation,
                conversation_id,
                result,
            });
        });
    }

    pub(crate) fn handle_context_timeline(
        &mut self,
        generation: crate::app_event::SessionGeneration,
        conversation_id: Option<nori_harness::ConversationId>,
        result: Result<nori_harness::context_timeline::ContextTimeline, String>,
    ) {
        if generation != self.session_generation || conversation_id != self.conversation_id {
            return;
        }
        match result {
            Ok(timeline) => self.insert_context_output(timeline),
            Err(error) => {
                self.add_error_message(format!("Could not read context timeline: {error}"))
            }
        }
    }

    fn insert_context_output(&mut self, timeline: nori_harness::context_timeline::ContextTimeline) {
        let (usage, source) = self.bottom_pane.context_usage_snapshot();
        let remote = self.cloud_mode || self.cloud_session_identity().is_some();
        let instructions = if remote {
            Vec::new()
        } else {
            let cwd = self
                .effective_cwd_tracker
                .effective_cwd()
                .unwrap_or(&self.config.cwd);
            local_instruction_files(&self.config.active_agent, cwd)
                .into_iter()
                .filter(|file| file.active)
                .map(|file| {
                    (
                        format_instruction_path(&file.path),
                        file.token_count.map(|tokens| tokens.count),
                    )
                })
                .collect()
        };
        self.add_to_history(ContextView {
            timeline,
            used: usage.tokens,
            capacity: usage.window_tokens,
            source: source.into(),
            agent: self.bottom_pane.agent_display_name().to_string(),
            turns: i64::from(self.session_stats.user_messages),
            instructions,
            skills: self.session_stats.skills_used.clone(),
            subagents: self.session_stats.subagents_used.clone(),
            remote,
        });
    }
}
