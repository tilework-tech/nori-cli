//! The status view model: everything the compact welcome card and the full
//! `/status` card render, assembled once by `ChatWidget`.
//!
//! The header cells are pure views over [`StatusViewModel`]. Nothing in the
//! rendering path reads global state, queries git, or interprets agent-specific
//! configuration ids; if a value is not in the model, it is not shown.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock;

use nori_harness::ConversationId;
use nori_harness::TranscriptTokenUsage;

use crate::nori::agent_config_state::AgentConfigState;
use crate::nori::agent_config_state::AgentConfigValue;
use crate::system_info::NoriVersionSource;
use crate::system_info::read_active_skillset;
use nori_protocol::acp::v1 as acp;

use super::AgentKindSimple;
use super::InstructionFile;
use super::detect_agent_kind;
use super::discover_all_instruction_files;

/// Identity of the cloud session the TUI is attached to. The top-level cloud
/// launch path supplies this identity; ACP capabilities do not.
#[derive(Debug, Clone)]
pub(crate) struct CloudSessionInfo {
    /// The human-readable session id, e.g. `nori-fast-kazunoko-aac8`.
    pub id: String,
    /// The broker-reported session title, when known (e.g. "Fix login flakes").
    pub title: Option<String>,
}

/// Git values mirrored from the footer so the status card stays a superset of
/// the footer's information categories.
#[derive(Debug, Clone, Default)]
pub(crate) struct GitStatus {
    /// Current git branch, when the cwd is inside a git repo.
    pub(crate) branch: Option<String>,
    /// Whether the cwd is a git worktree (not the main checkout).
    pub(crate) is_worktree: bool,
    /// The worktree directory name, when in a worktree.
    pub(crate) worktree_name: Option<String>,
    /// Added lines relative to the branch's merge base.
    pub(crate) lines_added: Option<i32>,
    /// Removed lines relative to the branch's merge base.
    pub(crate) lines_removed: Option<i32>,
    /// Whether there are untracked, non-ignored files.
    pub(crate) has_untracked: bool,
}

/// Context-window values mirrored from the footer.
#[derive(Debug, Clone, Default)]
pub(crate) struct ContextStatus {
    /// Tokens currently used in the context window.
    pub(crate) tokens: Option<i64>,
    /// Maximum tokens available in the context window.
    pub(crate) window_tokens: Option<i64>,
    /// Context window percentage *used* (0-100).
    pub(crate) percent_used: Option<i64>,
}

/// The detected Nori skillset and the version of the tooling that installed it.
#[derive(Debug, Clone, Default)]
pub(crate) struct SkillsetStatus {
    /// The active skillset name, when one is configured.
    pub(crate) name: Option<String>,
    /// Detected Nori skillsets version.
    pub(crate) version: Option<String>,
    /// The source of the version detection (affects the display label).
    pub(crate) version_source: Option<NoriVersionSource>,
}

/// The values the status card mirrors from the footer, so the card stays a
/// superset of the footer's information categories regardless of how the user
/// configured their footer.
#[derive(Debug, Clone, Default)]
pub(crate) struct StatusFooterValues {
    /// Git values for the `Git` row.
    pub(crate) git: GitStatus,
    /// Context-window values for the `Context` row.
    pub(crate) context: ContextStatus,
    /// Agent-supplied session title.
    pub(crate) session_title: Option<String>,
    /// First-prompt summary.
    pub(crate) prompt_summary: Option<String>,
    /// Detected Nori skillsets version.
    pub(crate) nori_version: Option<String>,
    /// The source of the version detection.
    pub(crate) nori_version_source: Option<NoriVersionSource>,
    /// Transcript token usage.
    pub(crate) token_breakdown: Option<TranscriptTokenUsage>,
}

/// One agent configuration row, already resolved to agent-supplied labels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentStatusOption {
    /// The agent's label for the option, e.g. `Model` or `Fast mode`.
    pub(crate) label: String,
    /// The resolved current value, e.g. `Opus 5`.
    pub(crate) value: AgentConfigValue,
}

impl AgentStatusOption {
    /// The value as text, e.g. `Opus 5` or `Off`.
    pub(crate) fn display_value(&self) -> String {
        self.value.display()
    }

    /// How the option reads on the single-line compact agent row. Boolean
    /// toggles are presence-based there: the label appears when the toggle is
    /// on and the option is dropped entirely when it is off.
    pub(crate) fn compact_text(&self) -> Option<String> {
        match &self.value {
            AgentConfigValue::Select(value) => Some(value.clone()),
            AgentConfigValue::Boolean(true) => Some(self.label.clone()),
            AgentConfigValue::Boolean(false) => None,
        }
    }
}

/// The agent identity plus its advertised configuration, ordered for display.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AgentStatus {
    /// The provider name shown on the agent row, e.g. `Claude`. This is the
    /// only coloured element of the row.
    pub(crate) provider: String,
    /// The agent family, used for the provider colour only.
    pub(crate) kind: Option<AgentKindSimple>,
    /// Configuration rows in display order: model, thought level, then every
    /// remaining option in the order the agent advertised it. Empty until the
    /// agent advertises its configuration.
    pub(crate) options: Vec<AgentStatusOption>,
}

impl AgentStatus {
    /// The identity-only status used before the agent advertises any
    /// configuration. Rendering never guesses a model or mode.
    pub(crate) fn new(agent_slug: &str) -> Self {
        Self {
            provider: provider_display_name(agent_slug),
            kind: detect_agent_kind(agent_slug),
            options: Vec::new(),
        }
    }

    /// The identity plus the agent's advertised configuration.
    ///
    /// Display order is the model selector, then the thought-level selector,
    /// then everything else in exactly the order the agent advertised it. The
    /// categories come from ACP, so no agent-specific option id is special
    /// cased here.
    pub(crate) fn from_config(agent_slug: &str, config: &AgentConfigState) -> Self {
        let leading = [
            acp::SessionConfigOptionCategory::Model,
            acp::SessionConfigOptionCategory::ThoughtLevel,
        ];

        let mut options: Vec<AgentStatusOption> = Vec::new();
        for category in &leading {
            if let Some(option) = config.option_for_category(category) {
                options.push(AgentStatusOption {
                    label: option.name.clone(),
                    value: option.value.clone(),
                });
            }
        }
        for option in config.options() {
            if leading.iter().any(|category| option.is_category(category)) {
                continue;
            }
            options.push(AgentStatusOption {
                label: option.name.clone(),
                value: option.value.clone(),
            });
        }

        Self {
            options,
            ..Self::new(agent_slug)
        }
    }
}

/// A live handle to the agent status.
///
/// The welcome card is written to history before the agent has advertised its
/// configuration, so it holds the shared handle and fills in as soon as the
/// configuration arrives. `/status` takes a fixed snapshot instead, because a
/// command's output should not change after it was printed.
#[derive(Debug, Clone, Default)]
pub(crate) struct AgentStatusHandle(Arc<RwLock<AgentStatus>>);

impl AgentStatusHandle {
    pub(crate) fn new(status: AgentStatus) -> Self {
        Self(Arc::new(RwLock::new(status)))
    }

    /// Publish a new status to every view holding this handle.
    pub(crate) fn set(&self, status: AgentStatus) {
        if let Ok(mut current) = self.0.write() {
            *current = status;
        }
    }

    /// The current status.
    pub(crate) fn get(&self) -> AgentStatus {
        self.0
            .read()
            .map(|status| status.clone())
            .unwrap_or_default()
    }

    /// A detached copy that no longer follows later updates.
    pub(crate) fn snapshot(&self) -> Self {
        Self::new(self.get())
    }
}

/// Everything the session header and `/status` render.
#[derive(Debug, Clone)]
pub(crate) struct StatusViewModel {
    /// Nori CLI version shown in the heading.
    pub(crate) version: &'static str,
    /// The session's working directory.
    pub(crate) directory: PathBuf,
    /// Agent identity and configuration.
    pub(crate) agent: AgentStatusHandle,
    /// The active skillset and the version that installed it.
    pub(crate) skillset: SkillsetStatus,
    /// Local instruction files with their activation state.
    pub(crate) instruction_files: Vec<InstructionFile>,
    /// Approval mode label (e.g. "Agent", "Read Only", "Full Access").
    pub(crate) approval_mode_label: Option<String>,
    /// First-prompt summary.
    pub(crate) prompt_summary: Option<String>,
    /// Agent-supplied session title from ACP session-info updates.
    pub(crate) session_title: Option<String>,
    /// The local conversation id, when one has been assigned.
    pub(crate) conversation_id: Option<ConversationId>,
    /// The parent conversation id after a branch-at-head fork.
    pub(crate) forked_from: Option<ConversationId>,
    /// Cloud session identity when attached through cloud mode.
    pub(crate) cloud_session: Option<CloudSessionInfo>,
    /// Git values mirrored from the footer.
    pub(crate) git: GitStatus,
    /// Context-window values mirrored from the footer.
    pub(crate) context: ContextStatus,
    /// Token usage breakdown from the transcript.
    pub(crate) token_breakdown: Option<TranscriptTokenUsage>,
}

impl StatusViewModel {
    /// An empty model for `directory`, running `agent`. Callers layer the
    /// session, footer, and local-context values on top; nothing is discovered
    /// here so the views stay reproducible.
    pub(crate) fn new(agent: AgentStatusHandle, directory: PathBuf) -> Self {
        Self {
            version: crate::version::CODEX_CLI_VERSION,
            directory,
            agent,
            skillset: SkillsetStatus::default(),
            instruction_files: Vec::new(),
            approval_mode_label: None,
            prompt_summary: None,
            session_title: None,
            conversation_id: None,
            forked_from: None,
            cloud_session: None,
            git: GitStatus::default(),
            context: ContextStatus::default(),
            token_breakdown: None,
        }
    }

    /// The agent status at render time.
    pub(crate) fn agent_status(&self) -> AgentStatus {
        self.agent.get()
    }
}

/// The active skillset and the instruction files `agent` loads from `cwd`.
/// This is the filesystem half of the assembly step.
pub(crate) fn local_context(
    agent_slug: &str,
    cwd: &Path,
) -> (Option<String>, Vec<InstructionFile>) {
    (
        read_active_skillset(cwd),
        discover_all_instruction_files(cwd, detect_agent_kind(agent_slug)),
    )
}

/// The provider name for an agent slug: the short brand name for the agent
/// families the TUI colours, and the agent's registered display name for
/// everything else.
pub(crate) fn provider_display_name(agent_slug: &str) -> String {
    match detect_agent_kind(agent_slug) {
        Some(AgentKindSimple::Claude) => "Claude".to_string(),
        Some(AgentKindSimple::Codex) => "Codex".to_string(),
        Some(AgentKindSimple::Gemini) => "Gemini".to_string(),
        None => nori_harness::get_agent_display_name(agent_slug),
    }
}

/// Discover the instruction files an agent loads from `cwd`, together with
/// their contents. Used by the local-context inspector.
pub(crate) fn active_instruction_file_contents(agent: &str, cwd: &Path) -> Vec<(PathBuf, String)> {
    let agent_kind = detect_agent_kind(agent);
    discover_all_instruction_files(cwd, agent_kind)
        .into_iter()
        .filter(|file| file.active)
        .filter_map(|file| {
            std::fs::read_to_string(&file.path)
                .ok()
                .map(|contents| (file.path, contents))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn select(
        id: &str,
        name: &str,
        current: &str,
        values: &[(&str, &str)],
    ) -> acp::SessionConfigOption {
        acp::SessionConfigOption::select(
            id.to_string(),
            name.to_string(),
            current.to_string(),
            values
                .iter()
                .map(|(value, label)| {
                    acp::SessionConfigSelectOption::new(value.to_string(), label.to_string())
                })
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn agent_status_without_config_is_provider_only() {
        let status = AgentStatus::new("claude-code");

        assert_eq!(status.provider, "Claude");
        assert_eq!(status.kind, Some(AgentKindSimple::Claude));
        assert!(
            status.options.is_empty(),
            "no configuration must mean no guessed values"
        );
    }

    #[test]
    fn agent_status_orders_model_then_thought_level_then_advertised_order() {
        let config = AgentConfigState::from_options(&[
            select("mode", "Mode", "plan", &[("plan", "Plan")])
                .category(acp::SessionConfigOptionCategory::Mode),
            select("model", "Model", "opus-5", &[("opus-5", "Opus 5")])
                .category(acp::SessionConfigOptionCategory::Model),
            select("effort", "Effort", "xhigh", &[("xhigh", "xhigh")])
                .category(acp::SessionConfigOptionCategory::ThoughtLevel),
            select("priority", "Priority", "normal", &[("normal", "Normal")]),
        ]);

        let status = AgentStatus::from_config("claude-code", &config);

        let labels: Vec<&str> = status
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect();
        assert_eq!(labels, vec!["Model", "Effort", "Mode", "Priority"]);
    }

    #[test]
    fn compact_text_drops_toggles_that_are_off() {
        let on = AgentStatusOption {
            label: "Fast mode".to_string(),
            value: AgentConfigValue::Boolean(true),
        };
        let off = AgentStatusOption {
            label: "Fast mode".to_string(),
            value: AgentConfigValue::Boolean(false),
        };

        assert_eq!(on.compact_text().as_deref(), Some("Fast mode"));
        assert_eq!(off.compact_text(), None);
        assert_eq!(off.display_value(), "Off");
    }

    #[test]
    fn unknown_agents_fall_back_to_their_registered_display_name() {
        assert_eq!(provider_display_name("codex"), "Codex");
        // Unregistered slugs round-trip rather than being renamed.
        assert_eq!(provider_display_name("totally-unknown"), "totally-unknown");
    }

    #[test]
    fn handle_snapshot_stops_following_updates() {
        let live = AgentStatusHandle::new(AgentStatus::new("claude-code"));
        let snapshot = live.snapshot();

        live.set(AgentStatus::from_config(
            "claude-code",
            &AgentConfigState::from_options(&[select(
                "model",
                "Model",
                "opus-5",
                &[("opus-5", "Opus 5")],
            )
            .category(acp::SessionConfigOptionCategory::Model)]),
        ));

        assert_eq!(live.get().options.len(), 1);
        assert!(
            snapshot.get().options.is_empty(),
            "a printed /status must not change after the fact"
        );
    }
}
