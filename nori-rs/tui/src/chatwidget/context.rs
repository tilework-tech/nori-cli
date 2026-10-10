//! Assemble a context snapshot without sending a prompt to the agent.

use super::ChatWidget;
use crate::context_view::ContextView;
use crate::nori::session_header::format_instruction_path;
use crate::nori::session_header::local_instruction_files;

impl ChatWidget {
    pub(crate) fn add_context_output(&mut self) {
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
