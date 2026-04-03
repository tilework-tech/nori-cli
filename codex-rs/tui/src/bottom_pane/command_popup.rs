use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::WidgetRef;

use super::popup_consts::MAX_POPUP_ROWS;
use super::scroll_state::ScrollState;
use super::selection_popup_common::GenericDisplayRow;
use super::selection_popup_common::render_rows;
use crate::render::Insets;
use crate::render::RectExt;
use crate::slash_command::SlashCommand;
use crate::slash_command::built_in_slash_commands;
use codex_common::fuzzy_match::fuzzy_match;
use codex_protocol::custom_prompts::CustomPrompt;
use codex_protocol::custom_prompts::PROMPTS_CMD_PREFIX;
use nori_protocol::AgentCommandInfo;
use std::collections::HashMap;
use std::collections::HashSet;

/// A selectable item in the popup: either a built-in command, a user prompt,
/// or an agent-provided command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandItem {
    Builtin(SlashCommand),
    // Index into `prompts`
    UserPrompt(usize),
    // Index into `agent_commands`
    AgentCommand(usize),
}

pub(crate) struct CommandPopup {
    command_filter: String,
    builtins: Vec<(&'static str, SlashCommand)>,
    prompts: Vec<CustomPrompt>,
    agent_commands: Vec<AgentCommandInfo>,
    agent_command_prefix: String,
    state: ScrollState,
    description_overrides: HashMap<SlashCommand, String>,
}

impl CommandPopup {
    #[cfg(test)]
    pub(crate) fn new(prompts: Vec<CustomPrompt>) -> Self {
        Self::new_with_overrides(prompts, HashMap::new())
    }

    #[cfg(test)]
    pub(crate) fn new_with_overrides(
        prompts: Vec<CustomPrompt>,
        description_overrides: HashMap<SlashCommand, String>,
    ) -> Self {
        Self::new_full(prompts, Vec::new(), String::new(), description_overrides)
    }

    pub(crate) fn new_full(
        mut prompts: Vec<CustomPrompt>,
        agent_commands: Vec<AgentCommandInfo>,
        agent_command_prefix: String,
        description_overrides: HashMap<SlashCommand, String>,
    ) -> Self {
        let builtins = built_in_slash_commands();
        // Exclude prompts that collide with builtin command names and sort by name.
        let exclude: HashSet<String> = builtins.iter().map(|(n, _)| (*n).to_string()).collect();
        prompts.retain(|p| !exclude.contains(&p.name));
        prompts.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            command_filter: String::new(),
            builtins,
            prompts,
            agent_commands,
            agent_command_prefix,
            state: ScrollState::new(),
            description_overrides,
        }
    }

    pub(crate) fn set_prompts(&mut self, mut prompts: Vec<CustomPrompt>) {
        let exclude: HashSet<String> = self
            .builtins
            .iter()
            .map(|(n, _)| (*n).to_string())
            .collect();
        prompts.retain(|p| !exclude.contains(&p.name));
        prompts.sort_by(|a, b| a.name.cmp(&b.name));
        self.prompts = prompts;
    }

    pub(crate) fn prompt(&self, idx: usize) -> Option<&CustomPrompt> {
        self.prompts.get(idx)
    }

    pub(crate) fn agent_command(&self, idx: usize) -> Option<&AgentCommandInfo> {
        self.agent_commands.get(idx)
    }

    pub(crate) fn set_agent_commands(&mut self, commands: Vec<AgentCommandInfo>, prefix: String) {
        self.agent_commands = commands;
        self.agent_command_prefix = prefix;
    }

    /// Update the filter string based on the current composer text. The text
    /// passed in is expected to start with a leading '/'. Everything after the
    /// *first* '/" on the *first* line becomes the active filter that is used
    /// to narrow down the list of available commands.
    pub(crate) fn on_composer_text_change(&mut self, text: String) {
        let first_line = text.lines().next().unwrap_or("");

        if let Some(stripped) = first_line.strip_prefix('/') {
            // Extract the *first* token (sequence of non-whitespace
            // characters) after the slash so that `/clear something` still
            // shows the help for `/clear`.
            let token = stripped.trim_start();
            let cmd_token = token.split_whitespace().next().unwrap_or("");

            // Update the filter keeping the original case (commands are all
            // lower-case for now but this may change in the future).
            self.command_filter = cmd_token.to_string();
        } else {
            // The composer no longer starts with '/'. Reset the filter so the
            // popup shows the *full* command list if it is still displayed
            // for some reason.
            self.command_filter.clear();
        }

        // Reset or clamp selected index based on new filtered list.
        let matches_len = self.filtered_items().len();
        self.state.clamp_selection(matches_len);
        self.state
            .ensure_visible(matches_len, MAX_POPUP_ROWS.min(matches_len));
    }

    /// Determine the preferred height of the popup for a given width.
    /// Accounts for wrapped descriptions so that long tooltips don't overflow.
    pub(crate) fn calculate_required_height(&self, width: u16) -> u16 {
        use super::selection_popup_common::measure_rows_height;
        let rows = self.rows_from_matches(self.filtered());

        // Subtract 2 to match the horizontal inset applied to the render area
        // in render_ref (Insets::tlbr(0, 2, 0, 0)).
        measure_rows_height(&rows, &self.state, MAX_POPUP_ROWS, width.saturating_sub(2))
    }

    /// Compute fuzzy-filtered matches over built-in commands and user prompts,
    /// paired with optional highlight indices and score. Sorted by ascending
    /// score, then by name for stability.
    /// Build the display key for an agent command (e.g. "claude-code:loop").
    fn agent_command_display_key(&self, idx: usize) -> String {
        let cmd = &self.agent_commands[idx];
        if self.agent_command_prefix.is_empty() {
            cmd.name.clone()
        } else {
            format!("{}:{}", self.agent_command_prefix, cmd.name)
        }
    }

    fn filtered(&self) -> Vec<(CommandItem, Option<Vec<usize>>, i32)> {
        let filter = self.command_filter.trim();
        let builtin_names: HashSet<&str> = self.builtins.iter().map(|(n, _)| *n).collect();
        let mut out: Vec<(CommandItem, Option<Vec<usize>>, i32)> = Vec::new();
        if filter.is_empty() {
            // Built-ins first, in presentation order.
            for (_, cmd) in self.builtins.iter() {
                out.push((CommandItem::Builtin(*cmd), None, 0));
            }
            // Agent commands next, excluding collisions with builtins.
            for (idx, cmd) in self.agent_commands.iter().enumerate() {
                if !builtin_names.contains(cmd.name.as_str()) {
                    out.push((CommandItem::AgentCommand(idx), None, 0));
                }
            }
            // Then prompts, already sorted by name.
            for idx in 0..self.prompts.len() {
                out.push((CommandItem::UserPrompt(idx), None, 0));
            }
            return out;
        }

        for (_, cmd) in self.builtins.iter() {
            if let Some((indices, score)) = fuzzy_match(cmd.command(), filter) {
                out.push((CommandItem::Builtin(*cmd), Some(indices), score));
            }
        }
        // Agent commands with prefix-based display key.
        for (idx, cmd) in self.agent_commands.iter().enumerate() {
            if builtin_names.contains(cmd.name.as_str()) {
                continue;
            }
            let display = self.agent_command_display_key(idx);
            if let Some((indices, score)) = fuzzy_match(&display, filter) {
                out.push((CommandItem::AgentCommand(idx), Some(indices), score));
            }
        }
        // Support both search styles:
        // - Typing "name" should surface "/prompts:name" results.
        // - Typing "prompts:name" should also work.
        for (idx, p) in self.prompts.iter().enumerate() {
            let display = format!("{PROMPTS_CMD_PREFIX}:{}", p.name);
            if let Some((indices, score)) = fuzzy_match(&display, filter) {
                out.push((CommandItem::UserPrompt(idx), Some(indices), score));
            }
        }
        // When filtering, sort by ascending score and then by name for stability.
        out.sort_by(|a, b| {
            a.2.cmp(&b.2).then_with(|| {
                let an = match a.0 {
                    CommandItem::Builtin(c) => c.command(),
                    CommandItem::UserPrompt(i) => &self.prompts[i].name,
                    CommandItem::AgentCommand(i) => &self.agent_commands[i].name,
                };
                let bn = match b.0 {
                    CommandItem::Builtin(c) => c.command(),
                    CommandItem::UserPrompt(i) => &self.prompts[i].name,
                    CommandItem::AgentCommand(i) => &self.agent_commands[i].name,
                };
                an.cmp(bn)
            })
        });
        out
    }

    pub(crate) fn filtered_items(&self) -> Vec<CommandItem> {
        self.filtered().into_iter().map(|(c, _, _)| c).collect()
    }

    fn rows_from_matches(
        &self,
        matches: Vec<(CommandItem, Option<Vec<usize>>, i32)>,
    ) -> Vec<GenericDisplayRow> {
        matches
            .into_iter()
            .map(|(item, indices, _)| {
                let (name, description) = match item {
                    CommandItem::Builtin(cmd) => {
                        let desc = self
                            .description_overrides
                            .get(&cmd)
                            .cloned()
                            .unwrap_or_else(|| cmd.description().to_string());
                        (format!("/{}", cmd.command()), desc)
                    }
                    CommandItem::UserPrompt(i) => {
                        let prompt = &self.prompts[i];
                        let description = prompt
                            .description
                            .clone()
                            .unwrap_or_else(|| "send saved prompt".to_string());
                        (
                            format!("/{PROMPTS_CMD_PREFIX}:{}", prompt.name),
                            description,
                        )
                    }
                    CommandItem::AgentCommand(i) => {
                        let display_key = self.agent_command_display_key(i);
                        let cmd = &self.agent_commands[i];
                        (format!("/{display_key}"), cmd.description.clone())
                    }
                };
                GenericDisplayRow {
                    name,
                    match_indices: indices.map(|v| v.into_iter().map(|i| i + 1).collect()),
                    display_shortcut: None,
                    description: Some(description),
                }
            })
            .collect()
    }

    /// Move the selection cursor one step up.
    pub(crate) fn move_up(&mut self) {
        let len = self.filtered_items().len();
        self.state.move_up_wrap(len);
        self.state.ensure_visible(len, MAX_POPUP_ROWS.min(len));
    }

    /// Move the selection cursor one step down.
    pub(crate) fn move_down(&mut self) {
        let matches_len = self.filtered_items().len();
        self.state.move_down_wrap(matches_len);
        self.state
            .ensure_visible(matches_len, MAX_POPUP_ROWS.min(matches_len));
    }

    /// Return currently selected command, if any.
    pub(crate) fn selected_item(&self) -> Option<CommandItem> {
        let matches = self.filtered_items();
        self.state
            .selected_idx
            .and_then(|idx| matches.get(idx).copied())
    }
}

impl WidgetRef for CommandPopup {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let rows = self.rows_from_matches(self.filtered());
        render_rows(
            area.inset(Insets::tlbr(0, 2, 0, 0)),
            buf,
            &rows,
            &self.state,
            MAX_POPUP_ROWS,
            "no matches",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn filter_includes_init_when_typing_prefix() {
        let mut popup = CommandPopup::new(Vec::new());
        // Simulate the composer line starting with '/in' so the popup filters
        // matching commands by prefix.
        popup.on_composer_text_change("/in".to_string());

        // Access the filtered list via the selected command and ensure that
        // one of the matches is the new "init" command.
        let matches = popup.filtered_items();
        let has_init = matches.iter().any(|item| match item {
            CommandItem::Builtin(cmd) => cmd.command() == "init",
            CommandItem::UserPrompt(_) | CommandItem::AgentCommand(_) => false,
        });
        assert!(
            has_init,
            "expected '/init' to appear among filtered commands"
        );
    }

    #[test]
    fn selecting_init_by_exact_match() {
        let mut popup = CommandPopup::new(Vec::new());
        popup.on_composer_text_change("/init".to_string());

        // When an exact match exists, the selected command should be that
        // command by default.
        let selected = popup.selected_item();
        match selected {
            Some(CommandItem::Builtin(cmd)) => assert_eq!(cmd.command(), "init"),
            Some(CommandItem::UserPrompt(_) | CommandItem::AgentCommand(_)) => {
                panic!("unexpected non-builtin selected for '/init'")
            }
            None => panic!("expected a selected command for exact match"),
        }
    }

    #[test]
    fn model_is_first_suggestion_for_mo() {
        let mut popup = CommandPopup::new(Vec::new());
        popup.on_composer_text_change("/mo".to_string());
        let matches = popup.filtered_items();
        match matches.first() {
            Some(CommandItem::Builtin(cmd)) => assert_eq!(cmd.command(), "model"),
            Some(CommandItem::UserPrompt(_) | CommandItem::AgentCommand(_)) => {
                panic!("unexpected non-builtin ranked before '/model' for '/mo'")
            }
            None => panic!("expected at least one match for '/mo'"),
        }
    }

    #[test]
    fn prompt_discovery_lists_custom_prompts() {
        let prompts = vec![
            CustomPrompt {
                name: "foo".to_string(),
                path: "/tmp/foo.md".to_string().into(),
                content: "hello from foo".to_string(),
                description: None,
                argument_hint: None,
                kind: Default::default(),
            },
            CustomPrompt {
                name: "bar".to_string(),
                path: "/tmp/bar.md".to_string().into(),
                content: "hello from bar".to_string(),
                description: None,
                argument_hint: None,
                kind: Default::default(),
            },
        ];
        let popup = CommandPopup::new(prompts);
        let items = popup.filtered_items();
        let mut prompt_names: Vec<String> = items
            .into_iter()
            .filter_map(|it| match it {
                CommandItem::UserPrompt(i) => popup.prompt(i).map(|p| p.name.clone()),
                _ => None,
            })
            .collect();
        prompt_names.sort();
        assert_eq!(prompt_names, vec!["bar".to_string(), "foo".to_string()]);
    }

    #[test]
    fn prompt_name_collision_with_builtin_is_ignored() {
        // Create a prompt named like a builtin (e.g. "init").
        let popup = CommandPopup::new(vec![CustomPrompt {
            name: "init".to_string(),
            path: "/tmp/init.md".to_string().into(),
            content: "should be ignored".to_string(),
            description: None,
            argument_hint: None,
            kind: Default::default(),
        }]);
        let items = popup.filtered_items();
        let has_collision_prompt = items.into_iter().any(|it| match it {
            CommandItem::UserPrompt(i) => popup.prompt(i).is_some_and(|p| p.name == "init"),
            _ => false,
        });
        assert!(
            !has_collision_prompt,
            "prompt with builtin name should be ignored"
        );
    }

    #[test]
    fn prompt_description_uses_frontmatter_metadata() {
        let popup = CommandPopup::new(vec![CustomPrompt {
            name: "draftpr".to_string(),
            path: "/tmp/draftpr.md".to_string().into(),
            content: "body".to_string(),
            description: Some("Create feature branch, commit and open draft PR.".to_string()),
            argument_hint: None,
            kind: Default::default(),
        }]);
        let rows = popup.rows_from_matches(vec![(CommandItem::UserPrompt(0), None, 0)]);
        let description = rows.first().and_then(|row| row.description.as_deref());
        assert_eq!(
            description,
            Some("Create feature branch, commit and open draft PR.")
        );
    }

    #[test]
    fn prompt_description_falls_back_when_missing() {
        let popup = CommandPopup::new(vec![CustomPrompt {
            name: "foo".to_string(),
            path: "/tmp/foo.md".to_string().into(),
            content: "body".to_string(),
            description: None,
            argument_hint: None,
            kind: Default::default(),
        }]);
        let rows = popup.rows_from_matches(vec![(CommandItem::UserPrompt(0), None, 0)]);
        let description = rows.first().and_then(|row| row.description.as_deref());
        assert_eq!(description, Some("send saved prompt"));
    }

    #[test]
    fn description_override_replaces_builtin_description() {
        let mut overrides = HashMap::new();
        overrides.insert(
            SlashCommand::Agent,
            "switch between available ACP agents (current: Claude Code)".to_string(),
        );
        let popup = CommandPopup::new_with_overrides(Vec::new(), overrides);
        let rows =
            popup.rows_from_matches(vec![(CommandItem::Builtin(SlashCommand::Agent), None, 0)]);
        let description = rows.first().and_then(|row| row.description.as_deref());
        assert_eq!(
            description,
            Some("switch between available ACP agents (current: Claude Code)")
        );
    }

    #[test]
    fn description_override_does_not_affect_other_commands() {
        let mut overrides = HashMap::new();
        overrides.insert(
            SlashCommand::Agent,
            "switch between available ACP agents (current: Claude Code)".to_string(),
        );
        let popup = CommandPopup::new_with_overrides(Vec::new(), overrides);
        let rows =
            popup.rows_from_matches(vec![(CommandItem::Builtin(SlashCommand::Model), None, 0)]);
        let description = rows.first().and_then(|row| row.description.as_deref());
        assert_eq!(description, Some(SlashCommand::Model.description()));
    }

    #[test]
    fn agent_commands_appear_in_unfiltered_list() {
        let agent_commands = vec![
            AgentCommandInfo {
                name: "loop".to_string(),
                description: "Run a prompt on a recurring interval".to_string(),
                input_hint: Some("interval command".to_string()),
            },
            AgentCommandInfo {
                name: "schedule".to_string(),
                description: "Create scheduled remote agents".to_string(),
                input_hint: None,
            },
        ];
        let popup = CommandPopup::new_full(
            Vec::new(),
            agent_commands,
            "claude-code".to_string(),
            HashMap::new(),
        );
        let items = popup.filtered_items();
        let agent_count = items
            .iter()
            .filter(|it| matches!(it, CommandItem::AgentCommand(_)))
            .count();
        assert_eq!(agent_count, 2);
    }

    #[test]
    fn agent_commands_filtered_by_prefix() {
        let agent_commands = vec![
            AgentCommandInfo {
                name: "loop".to_string(),
                description: "Run a prompt on a recurring interval".to_string(),
                input_hint: None,
            },
            AgentCommandInfo {
                name: "schedule".to_string(),
                description: "Create scheduled remote agents".to_string(),
                input_hint: None,
            },
        ];
        let mut popup = CommandPopup::new_full(
            Vec::new(),
            agent_commands,
            "claude-code".to_string(),
            HashMap::new(),
        );
        popup.on_composer_text_change("/claude-code:lo".to_string());
        let items = popup.filtered_items();
        let has_loop = items
            .iter()
            .any(|it| matches!(it, CommandItem::AgentCommand(0)));
        assert!(
            has_loop,
            "expected 'loop' agent command to match filter 'claude-code:lo'"
        );
        let has_schedule = items
            .iter()
            .any(|it| matches!(it, CommandItem::AgentCommand(1)));
        assert!(
            !has_schedule,
            "expected 'schedule' agent command NOT to match filter 'claude-code:lo'"
        );
    }

    #[test]
    fn agent_command_collision_with_builtin_is_excluded() {
        let agent_commands = vec![
            AgentCommandInfo {
                name: "compact".to_string(),
                description: "agent compact".to_string(),
                input_hint: None,
            },
            AgentCommandInfo {
                name: "loop".to_string(),
                description: "agent loop".to_string(),
                input_hint: None,
            },
        ];
        let popup = CommandPopup::new_full(
            Vec::new(),
            agent_commands,
            "claude-code".to_string(),
            HashMap::new(),
        );
        let items = popup.filtered_items();
        // "compact" collides with builtin, should only appear as Builtin
        let agent_compact = items
            .iter()
            .any(|it| matches!(it, CommandItem::AgentCommand(i) if popup.agent_command(*i).is_some_and(|c| c.name == "compact")));
        assert!(
            !agent_compact,
            "agent command 'compact' should be excluded due to builtin collision"
        );
        // "loop" has no collision, should appear
        let agent_loop = items
            .iter()
            .any(|it| matches!(it, CommandItem::AgentCommand(i) if popup.agent_command(*i).is_some_and(|c| c.name == "loop")));
        assert!(
            agent_loop,
            "agent command 'loop' should appear (no collision)"
        );
    }

    #[test]
    fn agent_command_renders_with_prefix() {
        let agent_commands = vec![AgentCommandInfo {
            name: "loop".to_string(),
            description: "Run a prompt on a recurring interval".to_string(),
            input_hint: None,
        }];
        let popup = CommandPopup::new_full(
            Vec::new(),
            agent_commands,
            "claude-code".to_string(),
            HashMap::new(),
        );
        let rows = popup.rows_from_matches(vec![(CommandItem::AgentCommand(0), None, 0)]);
        let name = rows.first().map(|r| r.name.as_str());
        assert_eq!(name, Some("/claude-code:loop"));
        let desc = rows.first().and_then(|r| r.description.as_deref());
        assert_eq!(desc, Some("Run a prompt on a recurring interval"));
    }

    #[test]
    fn agent_command_without_prefix_renders_bare_name() {
        let agent_commands = vec![AgentCommandInfo {
            name: "loop".to_string(),
            description: "loop desc".to_string(),
            input_hint: None,
        }];
        let popup =
            CommandPopup::new_full(Vec::new(), agent_commands, String::new(), HashMap::new());
        let rows = popup.rows_from_matches(vec![(CommandItem::AgentCommand(0), None, 0)]);
        let name = rows.first().map(|r| r.name.as_str());
        assert_eq!(name, Some("/loop"));
    }

    #[test]
    fn set_agent_commands_updates_prefix_on_existing_commands() {
        let agent_commands = vec![AgentCommandInfo {
            name: "loop".to_string(),
            description: "loop desc".to_string(),
            input_hint: None,
        }];
        let mut popup = CommandPopup::new_full(
            Vec::new(),
            agent_commands.clone(),
            String::new(),
            HashMap::new(),
        );
        // Initially no prefix
        let rows = popup.rows_from_matches(vec![(CommandItem::AgentCommand(0), None, 0)]);
        assert_eq!(rows.first().map(|r| r.name.as_str()), Some("/loop"));

        // Update with a prefix via set_agent_commands
        popup.set_agent_commands(agent_commands, "claude-code".to_string());
        let rows = popup.rows_from_matches(vec![(CommandItem::AgentCommand(0), None, 0)]);
        assert_eq!(
            rows.first().map(|r| r.name.as_str()),
            Some("/claude-code:loop")
        );
    }
}
