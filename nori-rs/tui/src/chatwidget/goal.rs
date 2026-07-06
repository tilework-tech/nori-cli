use super::*;
use codex_protocol::num_format::format_elapsed_seconds;
use codex_protocol::num_format::format_si_suffix;

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
                self.submit_op(Op::ThreadGoalSet {
                    objective: None,
                    status: Some(codex_protocol::protocol::ThreadGoalStatus::Paused),
                });
            }
            "resume" => {
                self.submit_op(Op::ThreadGoalSet {
                    objective: None,
                    status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                });
            }
            "clear" => {
                self.submit_op(Op::ThreadGoalClear);
            }
            "edit" => {
                self.open_goal_editor_or_request_snapshot();
            }
            _ => {
                if let Err(message) = codex_protocol::protocol::validate_thread_goal_objective(rest)
                {
                    self.add_error_message(message);
                    return true;
                }
                if self.should_confirm_before_replacing_goal() {
                    self.show_replace_goal_confirmation(rest.to_string());
                    return true;
                }
                self.submit_op(Op::ThreadGoalSet {
                    objective: Some(rest.to_string()),
                    status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                });
            }
        }
        true
    }

    pub(super) fn handle_session_capabilities_changed(
        &mut self,
        update: nori_protocol::SessionCapabilitiesView,
    ) {
        let previous = std::mem::replace(
            &mut self.builtin_command_availability,
            update.builtin_commands,
        );
        self.session_agent_capabilities = update.agent;

        let command_names: HashSet<String> = previous
            .keys()
            .chain(self.builtin_command_availability.keys())
            .cloned()
            .collect();
        for command_name in command_names {
            let Ok(command) = command_name.parse::<SlashCommand>() else {
                continue;
            };
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

    fn builtin_command_availability(
        &self,
        command: SlashCommand,
    ) -> nori_protocol::CommandAvailability {
        self.builtin_command_availability
            .get(command.command())
            .cloned()
            .unwrap_or(nori_protocol::CommandAvailability {
                enabled: true,
                reason: None,
            })
    }

    pub(super) fn request_thread_goal_status(&mut self) {
        self.pending_goal_status = true;
        self.submit_op(Op::ThreadGoalGet);
    }

    pub(super) fn handle_thread_goal_updated(&mut self, goal: nori_protocol::ThreadGoal) {
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

    pub(super) fn handle_thread_goal_cleared(&mut self) {
        self.current_goal = None;
        self.pending_goal_status = false;
        self.pending_goal_edit = false;
        self.add_info_message("Goal cleared".to_string(), None);
        self.request_redraw();
    }

    pub(super) fn clear_pending_goal_edit_if_no_goal(
        &mut self,
        update: &nori_protocol::SessionUpdateInfo,
    ) {
        if self.pending_goal_edit
            && update.kind == nori_protocol::SessionUpdateKind::SessionInfo
            && update.hint.as_deref() == Some("No goal is currently set.")
        {
            self.pending_goal_edit = false;
        }
        if self.pending_goal_status
            && update.kind == nori_protocol::SessionUpdateKind::SessionInfo
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
            self.submit_op(Op::ThreadGoalGet);
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
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::CodexOp(Op::ThreadGoalSet {
                        objective: Some(replacement.clone()),
                        status: Some(codex_protocol::protocol::ThreadGoalStatus::Active),
                    }));
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

        self.show_selection_view(SelectionViewParams {
            title: Some("Replace goal?".to_string()),
            subtitle: Some(format!("New objective: {objective}")),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            ..Default::default()
        });
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
