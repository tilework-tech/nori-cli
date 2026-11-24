//! Which host permission modes a built-in adapter can honor, read from the
//! adapter's own mode option. Shared by the host, which moves the adapter's
//! mode, and the session manager, which tells clients the modes a session
//! offers.

use nori_protocol::acp::v1 as acp;

use crate::AgentKind;

/// A built-in adapter's mode ids for `ask` and for `agent`.
pub struct AdapterModes {
    pub ask: &'static str,
    pub agent: &'static str,
}

pub fn adapter_modes(agent: &str) -> Option<AdapterModes> {
    match AgentKind::from_slug(agent)? {
        // Claude Code's `default` asks before edits and before any command
        // outside its read-only set; `acceptEdits` allows edits.
        AgentKind::ClaudeCode => Some(AdapterModes {
            ask: "default",
            agent: "acceptEdits",
        }),
        // Codex's `read-only` runs commands in a read-only sandbox and asks
        // before every edit or escalation; `workspace-write` lets it work in
        // the workspace and ask the user, not Codex's auto-reviewer, for more.
        AgentKind::Codex => Some(AdapterModes {
            ask: "read-only",
            agent: "workspace-write",
        }),
        // None of these has a mode that asks before every edit and command:
        // their modes change behavior, or their prompts depend on settings
        // ACP cannot reach.
        AgentKind::Gemini
        | AgentKind::GrokBuild
        | AgentKind::OpenCode
        | AgentKind::Cursor
        | AgentKind::Hermes
        | AgentKind::Pi => None,
    }
}

/// The adapter's mode option: its ID, current value and offered values.
pub fn adapter_mode_select(
    options: &[acp::SessionConfigOption],
) -> Option<(String, String, Vec<String>)> {
    options.iter().find_map(|option| {
        let is_mode = option.id.0.as_ref() == "mode"
            || option.category == Some(acp::SessionConfigOptionCategory::Mode);
        let acp::SessionConfigKind::Select(select) = &option.kind else {
            return None;
        };
        if !is_mode {
            return None;
        }
        let values = match &select.options {
            acp::SessionConfigSelectOptions::Ungrouped(values) => values
                .iter()
                .map(|choice| choice.value.0.to_string())
                .collect(),
            acp::SessionConfigSelectOptions::Grouped(groups) => groups
                .iter()
                .flat_map(|group| &group.options)
                .map(|choice| choice.value.0.to_string())
                .collect(),
            _ => Vec::new(),
        };
        Some((
            option.id.0.to_string(),
            select.current_value.0.to_string(),
            values,
        ))
    })
}

/// Whether `agent`'s adapter, with these options, has a mode that asks
/// before every edit and command.
pub fn offers_ask(agent: &str, options: &[acp::SessionConfigOption]) -> bool {
    match (adapter_modes(agent), adapter_mode_select(options)) {
        (Some(modes), Some((_, _, values))) => values.iter().any(|value| value == modes.ask),
        (None, _) | (_, None) => false,
    }
}
