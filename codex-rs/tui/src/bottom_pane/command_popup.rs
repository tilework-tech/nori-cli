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
use codex_protocol::custom_prompts::AGENT_CMD_PREFIX;
use codex_protocol::custom_prompts::AgentCommand;
use codex_protocol::custom_prompts::CustomPrompt;
use codex_protocol::custom_prompts::PROMPTS_CMD_PREFIX;
use std::collections::HashSet;

/// A selectable item in the popup: either a built-in command, a user prompt, or an agent command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandItem {
    Builtin(SlashCommand),
    /// Index into `prompts`
    UserPrompt(usize),
    /// Index into `agent_commands`
    AgentCommand(usize),
}

pub(crate) struct CommandPopup {
    command_filter: String,
    builtins: Vec<(&'static str, SlashCommand)>,
    prompts: Vec<CustomPrompt>,
    agent_commands: Vec<AgentCommand>,
    state: ScrollState,
}

impl CommandPopup {
    pub(crate) fn new(mut prompts: Vec<CustomPrompt>) -> Self {
        let builtins = built_in_slash_commands();
        // Exclude prompts that collide with builtin command names and sort by name.
        let exclude: HashSet<String> = builtins.iter().map(|(n, _)| (*n).to_string()).collect();
        prompts.retain(|p| !exclude.contains(&p.name));
        prompts.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            command_filter: String::new(),
            builtins,
            prompts,
            agent_commands: Vec::new(),
            state: ScrollState::new(),
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

    pub(crate) fn set_agent_commands(&mut self, mut commands: Vec<AgentCommand>) {
        let exclude: HashSet<String> = self
            .builtins
            .iter()
            .map(|(n, _)| (*n).to_string())
            .collect();
        commands.retain(|c| !exclude.contains(&c.name));
        commands.sort_by(|a, b| a.name.cmp(&b.name));
        self.agent_commands = commands;
    }

    pub(crate) fn agent_command(&self, idx: usize) -> Option<&AgentCommand> {
        self.agent_commands.get(idx)
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

        measure_rows_height(&rows, &self.state, MAX_POPUP_ROWS, width)
    }

    /// Compute fuzzy-filtered matches over built-in commands, user prompts, and agent commands,
    /// paired with optional highlight indices and score. Sorted by ascending
    /// score, then by name for stability.
    fn filtered(&self) -> Vec<(CommandItem, Option<Vec<usize>>, i32)> {
        let filter = self.command_filter.trim();
        let mut out: Vec<(CommandItem, Option<Vec<usize>>, i32)> = Vec::new();
        if filter.is_empty() {
            // Built-ins first, in presentation order.
            for (_, cmd) in self.builtins.iter() {
                out.push((CommandItem::Builtin(*cmd), None, 0));
            }
            // Then prompts, already sorted by name.
            for idx in 0..self.prompts.len() {
                out.push((CommandItem::UserPrompt(idx), None, 0));
            }
            // Then agent commands, already sorted by name.
            for idx in 0..self.agent_commands.len() {
                out.push((CommandItem::AgentCommand(idx), None, 0));
            }
            return out;
        }

        for (_, cmd) in self.builtins.iter() {
            if let Some((indices, score)) = fuzzy_match(cmd.command(), filter) {
                out.push((CommandItem::Builtin(*cmd), Some(indices), score));
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
        // Agent commands: support both "name" and "agent:name" search styles.
        for (idx, c) in self.agent_commands.iter().enumerate() {
            let display = format!("{AGENT_CMD_PREFIX}:{}", c.name);
            if let Some((indices, score)) = fuzzy_match(&display, filter) {
                out.push((CommandItem::AgentCommand(idx), Some(indices), score));
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

    fn filtered_items(&self) -> Vec<CommandItem> {
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
                        (format!("/{}", cmd.command()), cmd.description().to_string())
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
                        let cmd = &self.agent_commands[i];
                        (
                            format!("/{AGENT_CMD_PREFIX}:{}", cmd.name),
                            cmd.description.clone(),
                        )
                    }
                };
                GenericDisplayRow {
                    name,
                    match_indices: indices.map(|v| v.into_iter().map(|i| i + 1).collect()),
                    is_current: false,
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
            Some(CommandItem::UserPrompt(_)) => panic!("unexpected prompt selected for '/init'"),
            Some(CommandItem::AgentCommand(_)) => {
                panic!("unexpected agent command selected for '/init'")
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
            Some(CommandItem::UserPrompt(_)) => {
                panic!("unexpected prompt ranked before '/model' for '/mo'")
            }
            Some(CommandItem::AgentCommand(_)) => {
                panic!("unexpected agent command ranked before '/model' for '/mo'")
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

    // ==================== Agent Commands Tests ====================

    #[test]
    fn agent_commands_appear_in_filtered_list() {
        use codex_protocol::custom_prompts::AgentCommand;

        let mut popup = CommandPopup::new(Vec::new());

        // Set agent commands
        popup.set_agent_commands(vec![
            AgentCommand {
                name: "review".to_string(),
                description: "Review code changes".to_string(),
                argument_hint: None,
            },
            AgentCommand {
                name: "test".to_string(),
                description: "Run tests".to_string(),
                argument_hint: Some("[file]".to_string()),
            },
        ]);

        let items = popup.filtered_items();

        // Find agent commands in the list
        let agent_command_names: Vec<String> = items
            .into_iter()
            .filter_map(|it| match it {
                CommandItem::AgentCommand(i) => popup.agent_command(i).map(|c| c.name.clone()),
                _ => None,
            })
            .collect();

        assert_eq!(agent_command_names.len(), 2);
        assert!(agent_command_names.contains(&"review".to_string()));
        assert!(agent_command_names.contains(&"test".to_string()));
    }

    #[test]
    fn agent_commands_displayed_with_agent_prefix() {
        use codex_protocol::custom_prompts::AGENT_CMD_PREFIX;
        use codex_protocol::custom_prompts::AgentCommand;

        let mut popup = CommandPopup::new(Vec::new());
        popup.set_agent_commands(vec![AgentCommand {
            name: "review".to_string(),
            description: "Review code changes".to_string(),
            argument_hint: None,
        }]);

        let rows = popup.rows_from_matches(vec![(CommandItem::AgentCommand(0), None, 0)]);
        let name = rows.first().map(|row| row.name.as_str());

        // Should be displayed as "/agent:review"
        assert_eq!(name, Some(format!("/{AGENT_CMD_PREFIX}:review").as_str()));
    }

    #[test]
    fn agent_commands_filtered_by_typing() {
        use codex_protocol::custom_prompts::AgentCommand;

        let mut popup = CommandPopup::new(Vec::new());
        popup.set_agent_commands(vec![
            AgentCommand {
                name: "review".to_string(),
                description: "Review code changes".to_string(),
                argument_hint: None,
            },
            AgentCommand {
                name: "test".to_string(),
                description: "Run tests".to_string(),
                argument_hint: None,
            },
        ]);

        // Filter by typing "agent:rev"
        popup.on_composer_text_change("/agent:rev".to_string());

        let items = popup.filtered_items();
        let has_review = items.iter().any(|it| match it {
            CommandItem::AgentCommand(i) => {
                popup.agent_command(*i).is_some_and(|c| c.name == "review")
            }
            _ => false,
        });

        assert!(has_review, "expected 'review' command to match 'agent:rev'");
    }

    #[test]
    fn agent_command_collision_with_builtin_is_ignored() {
        use codex_protocol::custom_prompts::AgentCommand;

        let mut popup = CommandPopup::new(Vec::new());

        // Try to add an agent command with same name as a builtin
        popup.set_agent_commands(vec![AgentCommand {
            name: "init".to_string(),
            description: "Should be ignored".to_string(),
            argument_hint: None,
        }]);

        let items = popup.filtered_items();
        let has_agent_init = items.iter().any(|it| match it {
            CommandItem::AgentCommand(i) => {
                popup.agent_command(*i).is_some_and(|c| c.name == "init")
            }
            _ => false,
        });

        assert!(
            !has_agent_init,
            "agent command with builtin name should be filtered out"
        );
    }

    #[test]
    fn set_agent_commands_clears_previous_commands() {
        use codex_protocol::custom_prompts::AgentCommand;

        let mut popup = CommandPopup::new(Vec::new());

        // Set initial commands
        popup.set_agent_commands(vec![AgentCommand {
            name: "old-command".to_string(),
            description: "Old".to_string(),
            argument_hint: None,
        }]);

        // Replace with new commands
        popup.set_agent_commands(vec![AgentCommand {
            name: "new-command".to_string(),
            description: "New".to_string(),
            argument_hint: None,
        }]);

        let items = popup.filtered_items();
        let command_names: Vec<String> = items
            .into_iter()
            .filter_map(|it| match it {
                CommandItem::AgentCommand(i) => popup.agent_command(i).map(|c| c.name.clone()),
                _ => None,
            })
            .collect();

        assert_eq!(command_names, vec!["new-command".to_string()]);
    }
}
