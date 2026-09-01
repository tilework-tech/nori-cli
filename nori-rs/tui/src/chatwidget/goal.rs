use super::*;
use crate::slash_command::CommandScope;
use crate::ui_types::format_elapsed_seconds;
use crate::ui_types::format_si_suffix;
use strum::IntoEnumIterator;

impl ChatWidget {
    pub(super) fn handle_goal_user_message(&mut self, text: &str) -> bool {
        let Some(rest) = text.strip_prefix("/goal") else {
            return false;
        };
        if !rest.is_empty() && !rest.starts_with(' ') {
            return false;
        }
        if !self.ensure_builtin_command_enabled(SlashCommand::Goal) {
            return true;
        }

        let rest = rest.trim();
        if rest.is_empty() {
            self.request_thread_goal_status();
            return true;
        }

        let lower = rest.to_ascii_lowercase();
        match lower.as_str() {
            "pause" => {
                self.submit_harness_action(crate::app_event::HarnessAction::SetGoalStatus(
                    nori_protocol::ThreadGoalStatus::Paused,
                ));
            }
            "resume" => {
                self.submit_harness_action(crate::app_event::HarnessAction::SetGoalStatus(
                    nori_protocol::ThreadGoalStatus::Active,
                ));
            }
            "clear" => {
                self.submit_harness_action(crate::app_event::HarnessAction::ClearGoal);
            }
            "edit" => {
                self.open_goal_editor_or_request_snapshot();
            }
            _ => {
                if self.should_confirm_before_replacing_goal() {
                    self.show_replace_goal_confirmation(rest.to_string());
                    return true;
                }
                self.submit_harness_action(crate::app_event::HarnessAction::SetGoal {
                    objective: rest.to_string(),
                    status: Some(nori_protocol::ThreadGoalStatus::Active),
                });
            }
        }
        true
    }

    pub(super) fn handle_session_capabilities_changed(
        &mut self,
        update: crate::presentation::SessionCapabilitiesView,
    ) {
        self.builtin_command_availability = update.builtin_commands;
        self.session_agent_capabilities = update.agent;

        // Refresh the popup greying for every builtin from the merged
        // (server + client-scope) verdict. Iterating all variants — not just
        // the server map keys — both clears stale disabled state and applies
        // scope verdicts for commands the server never mentions.
        for command in SlashCommand::iter() {
            let availability = self.builtin_command_availability(command);
            let disabled_reason = (!availability.enabled).then(|| {
                Line::from(
                    availability
                        .reason
                        .unwrap_or_else(|| default_command_unavailable_reason(command)),
                )
            });
            self.bottom_pane
                .set_builtin_command_disabled(command, disabled_reason);
        }
        self.request_redraw();
    }

    pub(super) fn ensure_builtin_command_enabled(&mut self, command: SlashCommand) -> bool {
        let availability = self.builtin_command_availability(command);
        if availability.enabled {
            return true;
        }

        self.add_error_message(format!(
            "/{} is unavailable. {}",
            command.command(),
            availability
                .reason
                .unwrap_or_else(|| default_command_unavailable_reason(command))
        ));
        self.request_redraw();
        false
    }

    /// Merged availability verdict for a builtin command: an explicit server
    /// disable (with its reason) wins; otherwise the client-side session-type
    /// scope applies. Cloud mode comes from the launch path; capabilities are
    /// consulted only for capability-gated commands such as `/close`.
    fn builtin_command_availability(
        &self,
        command: SlashCommand,
    ) -> nori_protocol::CommandAvailability {
        if let Some(server) = self.builtin_command_availability.get(command.command())
            && !server.enabled
        {
            return server.clone();
        }
        if let Some(reason) =
            scope_unavailable_reason(command, self.cloud_mode, &self.session_agent_capabilities)
        {
            return nori_protocol::CommandAvailability {
                enabled: false,
                reason: Some(reason),
            };
        }
        nori_protocol::CommandAvailability {
            enabled: true,
            reason: None,
        }
    }

    pub(super) fn request_thread_goal_status(&mut self) {
        self.pending_goal_status = true;
        self.submit_harness_action(crate::app_event::HarnessAction::LoadGoal);
    }

    pub(crate) fn handle_thread_goal_updated(&mut self, goal: nori_protocol::ThreadGoal) {
        let should_show_summary = self.current_goal.as_ref().is_none_or(|previous| {
            previous.objective != goal.objective
                || previous.status != goal.status
                || previous.created_at != goal.created_at
        });
        self.current_goal = Some(goal.clone());
        if self.pending_goal_edit {
            self.pending_goal_edit = false;
            self.pending_goal_status = false;
            self.open_goal_editor(goal);
        } else if self.pending_goal_status || should_show_summary {
            self.pending_goal_status = false;
            self.show_goal_summary(&goal);
        }
        self.request_redraw();
    }

    pub(crate) fn handle_thread_goal_cleared(&mut self) {
        self.current_goal = None;
        self.pending_goal_status = false;
        self.pending_goal_edit = false;
        self.add_info_message("Goal cleared".to_string(), None);
        self.request_redraw();
    }

    pub(super) fn clear_pending_goal_edit_if_no_goal(
        &mut self,
        update: &crate::presentation::SessionUpdateInfo,
    ) {
        if self.pending_goal_edit
            && update.kind == crate::presentation::SessionUpdateKind::SessionInfo
            && update.hint.as_deref() == Some("No goal is currently set.")
        {
            self.pending_goal_edit = false;
        }
        if self.pending_goal_status
            && update.kind == crate::presentation::SessionUpdateKind::SessionInfo
            && update.hint.as_deref() == Some("No goal is currently set.")
        {
            self.pending_goal_status = false;
        }
    }

    fn open_goal_editor_or_request_snapshot(&mut self) {
        if let Some(goal) = self.current_goal.clone() {
            self.open_goal_editor(goal);
        } else {
            self.pending_goal_edit = true;
            self.submit_harness_action(crate::app_event::HarnessAction::LoadGoal);
        }
    }

    fn open_goal_editor(&mut self, goal: nori_protocol::ThreadGoal) {
        self.bottom_pane
            .set_composer_text(format!("/goal {}", goal.objective));
    }

    fn should_confirm_before_replacing_goal(&self) -> bool {
        let Some(goal) = &self.current_goal else {
            return false;
        };

        match goal.status {
            nori_protocol::ThreadGoalStatus::Complete => false,
            nori_protocol::ThreadGoalStatus::Active
            | nori_protocol::ThreadGoalStatus::Paused
            | nori_protocol::ThreadGoalStatus::Blocked
            | nori_protocol::ThreadGoalStatus::UsageLimited
            | nori_protocol::ThreadGoalStatus::BudgetLimited => true,
        }
    }

    fn show_replace_goal_confirmation(&mut self, objective: String) {
        let replacement = objective.clone();
        let items = vec![
            SelectionItem {
                name: "Replace current goal".to_string(),
                description: Some("Set the new objective and start it now".to_string()),
                menu_tone: nori_tui_components::MenuItemTone::Warning,
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::HarnessAction(
                        crate::app_event::HarnessAction::SetGoal {
                            objective: replacement.clone(),
                            status: Some(nori_protocol::ThreadGoalStatus::Active),
                        },
                    ));
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Keep current goal".to_string(),
                description: Some("Leave the current objective unchanged".to_string()),
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.show_selection_view(
            SelectionViewParams {
                title: Some("Replace goal?".to_string()),
                subtitle: Some(format!("New objective: {objective}")),
                footer_hint: Some(standard_popup_hint_line()),
                items,
                ..Default::default()
            }
            .menu(
                58,
                nori_tui_components::MenuDensity::Normal,
                nori_tui_components::MenuRowPattern::Plain,
                nori_tui_components::MenuPlacement::Centered,
            ),
        );
    }

    fn show_goal_summary(&mut self, goal: &nori_protocol::ThreadGoal) {
        self.add_plain_history_lines(vec![
            Line::from("Goal".bold()),
            Line::from(vec![
                "Status: ".dim(),
                goal_status_label(goal.status).into(),
            ]),
            Line::from(vec!["Objective: ".dim(), goal.objective.clone().into()]),
            Line::from(vec![
                "Time used: ".dim(),
                format_elapsed_seconds(goal.time_used_seconds).into(),
            ]),
            Line::from(vec![
                "Tokens used: ".dim(),
                format_si_suffix(goal.tokens_used).into(),
            ]),
            Line::default(),
            Line::from(goal_command_hint(goal.status).dim()),
        ]);
    }
}

fn default_command_unavailable_reason(command: SlashCommand) -> String {
    format!("/{} is disabled for the active session.", command.command())
}

/// Client-side session-type scope verdict: Some(reason) when `command` is
/// unavailable for the current session shape. LocalOnly commands operate on
/// the local machine and are meaningless in a cloud session (agent on a
/// remote VM); /close (CloudOnly) needs both cloud mode and the agent's
/// `session/close` capability.
fn scope_unavailable_reason(
    command: SlashCommand,
    cloud_mode: bool,
    agent: &crate::presentation::AgentCapabilitiesView,
) -> Option<String> {
    match command.scope() {
        CommandScope::LocalOnly if cloud_mode => Some(format!(
            "/{} runs on the local machine and is unavailable in cloud sessions.",
            command.command()
        )),
        CommandScope::CloudOnly if !cloud_mode => {
            Some("/close is available only in cloud sessions.".to_string())
        }
        CommandScope::CloudOnly if !agent.session_close => Some(
            "/close releases a cloud session and needs the agent's session/close capability."
                .to_string(),
        ),
        CommandScope::LocalOnly | CommandScope::CloudOnly | CommandScope::Universal => None,
    }
}

fn goal_status_label(status: nori_protocol::ThreadGoalStatus) -> &'static str {
    match status {
        nori_protocol::ThreadGoalStatus::Active => "active",
        nori_protocol::ThreadGoalStatus::Paused => "paused",
        nori_protocol::ThreadGoalStatus::Blocked => "blocked",
        nori_protocol::ThreadGoalStatus::UsageLimited => "usage limited",
        nori_protocol::ThreadGoalStatus::BudgetLimited => "limited by budget",
        nori_protocol::ThreadGoalStatus::Complete => "complete",
    }
}

fn goal_command_hint(status: nori_protocol::ThreadGoalStatus) -> &'static str {
    match status {
        nori_protocol::ThreadGoalStatus::Active => "Commands: /goal edit, /goal pause, /goal clear",
        nori_protocol::ThreadGoalStatus::Paused
        | nori_protocol::ThreadGoalStatus::Blocked
        | nori_protocol::ThreadGoalStatus::UsageLimited => {
            "Commands: /goal edit, /goal resume, /goal clear"
        }
        nori_protocol::ThreadGoalStatus::BudgetLimited
        | nori_protocol::ThreadGoalStatus::Complete => "Commands: /goal edit, /goal clear",
    }
}
