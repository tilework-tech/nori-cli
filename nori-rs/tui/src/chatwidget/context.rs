//! Assemble a context snapshot without sending a prompt to the agent.

use super::ChatWidget;
use crate::context_view::ContextView;
use crate::nori::session_header::format_instruction_path;
use crate::nori::session_header::local_instruction_files;

#[derive(Debug, Default)]
pub(crate) struct ContextSnapshot {
    timeline: nori_harness::context_timeline::ContextTimeline,
    instructions: Vec<(String, Option<i64>)>,
}

impl ChatWidget {
    pub(crate) fn add_context_output(&mut self) {
        let started = std::time::Instant::now();
        tracing::debug!(target: "nori_context_profile", stage = "context_dispatch", "context command dispatched");
        let handle = self.harness_handle.clone();
        let remote = self.cloud_mode || self.cloud_session_identity().is_some();
        if remote && handle.is_none() {
            self.insert_context_output(Default::default());
            return;
        }
        let cwd = self
            .effective_cwd_tracker
            .effective_cwd()
            .unwrap_or(&self.config.cwd)
            .clone();
        let agent = self.config.active_agent.clone();
        let generation = self.session_generation;
        let conversation_id = self.conversation_id;
        let app_event_tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            let timeline = async {
                match handle {
                    Some(handle) => handle
                        .context_timeline()
                        .await
                        .map_err(|error| error.to_string()),
                    None => Ok(Default::default()),
                }
            };
            let inventory = async {
                if remote {
                    return Ok(Vec::new());
                }
                tokio::task::spawn_blocking(move || {
                    let inventory_started = std::time::Instant::now();
                    let instructions: Vec<_> = local_instruction_files(&agent, &cwd)
                        .into_iter()
                        .filter(|file| file.active)
                        .map(|file| (
                            format_instruction_path(&file.path),
                            file.token_count.map(|tokens| tokens.count),
                        ))
                        .collect();
                    tracing::debug!(target: "nori_context_profile", stage = "instruction_inventory", elapsed_us = inventory_started.elapsed().as_micros(), files = instructions.len(), "context inventory collected");
                    instructions
                })
                .await
                .map_err(|error| format!("Instruction inventory failed: {error}"))
            };
            let (timeline, instructions) = tokio::join!(timeline, inventory);
            let result = timeline.and_then(|timeline| {
                instructions.map(|instructions| ContextSnapshot {
                    timeline,
                    instructions,
                })
            });
            tracing::debug!(target: "nori_context_profile", stage = "context_ready", elapsed_us = started.elapsed().as_micros(), "context snapshot ready");
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
        result: Result<ContextSnapshot, String>,
    ) {
        if generation != self.session_generation || conversation_id != self.conversation_id {
            return;
        }
        match result {
            Ok(snapshot) => self.insert_context_output(snapshot),
            Err(error) => {
                self.add_error_message(format!("Could not read context snapshot: {error}"))
            }
        }
    }

    fn insert_context_output(&mut self, snapshot: ContextSnapshot) {
        let started = std::time::Instant::now();
        let (usage, source) = self.bottom_pane.context_usage_snapshot();
        let remote = self.cloud_mode || self.cloud_session_identity().is_some();
        let insertion_started = std::time::Instant::now();
        self.add_to_history(ContextView {
            timeline: snapshot.timeline,
            used: usage.tokens,
            capacity: usage.window_tokens,
            source: source.into(),
            agent: self.bottom_pane.agent_display_name().to_string(),
            turns: i64::from(self.session_stats.user_messages),
            instructions: snapshot.instructions,
            skills: self.session_stats.skills_used.clone(),
            subagents: self.session_stats.subagents_used.clone(),
            remote,
        });
        tracing::debug!(target: "nori_context_profile", stage = "context_history_queued", elapsed_us = insertion_started.elapsed().as_micros(), total_us = started.elapsed().as_micros(), "context snapshot queued");
    }
}
