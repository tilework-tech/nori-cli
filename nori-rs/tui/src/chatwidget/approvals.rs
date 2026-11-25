use super::*;
use crate::app_event_sender::AppEventSender;
use nori_harness::permission_mode::PermissionMode;

impl ChatWidget {
    /// Open the host modes for the session on screen. Choosing one changes
    /// that session only; later sessions start in the host's default.
    pub(crate) fn open_approvals_popup(&mut self) {
        let current = self.current_approval_mode();
        let session_id = self.managed_conversation_id.map(|id| id.to_string());
        let warn_before_approve_all = !self
            .config
            .notices
            .hide_full_access_warning
            .unwrap_or(false);
        let unavailable_reason = format!(
            "{} has no mode that asks before every edit and command.",
            nori_harness::get_agent_display_name(&self.config.active_agent)
        );
        let items = PermissionMode::ALL
            .into_iter()
            .map(|mode| {
                let wire = nori_protocol::ApprovalMode::from(mode);
                let offered = self
                    .available_approval_modes
                    .as_ref()
                    .is_none_or(|available| available.contains(&wire));
                if !offered {
                    let reason = unavailable_reason.clone();
                    return SelectionItem {
                        name: format!("{} (not available)", mode.name()),
                        description: Some(reason.clone()),
                        actions: vec![Box::new(move |tx: &AppEventSender| {
                            tx.send(AppEvent::HarnessActionFailed(format!(
                                "/approvals: {reason}"
                            )));
                        }) as SelectionAction],
                        dismiss_on_select: true,
                        ..Default::default()
                    };
                }
                let actions = if mode == PermissionMode::ApproveAll && warn_before_approve_all {
                    let session_id = session_id.clone();
                    vec![Box::new(move |tx: &AppEventSender| {
                        tx.send(AppEvent::OpenApproveAllConfirmation {
                            session_id: session_id.clone(),
                        });
                    }) as SelectionAction]
                } else {
                    Self::approval_mode_actions(session_id.clone(), wire)
                };
                SelectionItem {
                    name: mode.name().to_string(),
                    description: Some(mode.description().to_string()),
                    is_current: mode == current,
                    actions,
                    dismiss_on_select: true,
                    ..Default::default()
                }
            })
            .collect();

        self.bottom_pane.show_selection_view(
            SelectionViewParams {
                title: Some("Approvals for this session".to_string()),
                subtitle: Some(
                    "New sessions start in the host default. The sandbox is set separately."
                        .to_string(),
                ),
                footer_hint: Some(standard_popup_hint_line()),
                items,
                header: Box::new(()),
                ..Default::default()
            }
            .picker(),
        );
    }

    /// `/approvals MODE` on a joined host: set the session's mode without the
    /// menu. Approving everything still asks first. Returns whether `text`
    /// was such a command.
    pub(super) fn handle_approvals_with_mode(&mut self, text: &str) -> bool {
        let Some(wanted) = text
            .strip_prefix("/approvals ")
            .map(str::trim)
            .filter(|wanted| !wanted.is_empty() && self.host_connection.is_some())
        else {
            return false;
        };
        let surface = self.command_surface();
        if let Some(reason) =
            crate::command_availability::unavailable_reason(SlashCommand::Approvals, surface)
        {
            self.add_error_message(format!("/approvals: {reason}"));
            self.set_composer_text(text.to_owned());
            return true;
        }
        let mode = match wanted {
            "ask" => nori_protocol::ApprovalMode::Ask,
            "agent" => nori_protocol::ApprovalMode::Agent,
            "approve-all" => nori_protocol::ApprovalMode::ApproveAll,
            _ => {
                self.add_error_message(format!(
                    "/approvals: unknown mode {wanted}; use ask, agent or approve-all"
                ));
                return true;
            }
        };
        let session_id = self.managed_conversation_id.map(|id| id.to_string());
        let warn = !self
            .config
            .notices
            .hide_full_access_warning
            .unwrap_or(false);
        if mode == nori_protocol::ApprovalMode::ApproveAll && warn {
            self.open_approve_all_confirmation(session_id);
        } else {
            for action in Self::approval_mode_actions(session_id, mode) {
                action(&self.app_event_tx);
            }
        }
        true
    }

    /// Change the mode of `session_id` on its host, or of this terminal's
    /// own session when it has no host session.
    fn approval_mode_actions(
        session_id: Option<String>,
        mode: nori_protocol::ApprovalMode,
    ) -> Vec<SelectionAction> {
        vec![Box::new(move |tx| match session_id.clone() {
            Some(session_id) => tx.send(AppEvent::HostRequest {
                call: crate::host_client::HostCall::SetMode { session_id, mode },
                reply: crate::app_event_sender::HostReply::ReportError(tx.clone()),
            }),
            None => tx.send(AppEvent::ApplyApprovalMode(mode)),
        })]
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(super) fn approval_preset_actions(
        approval: AskForApproval,
        sandbox: SandboxPolicy,
    ) -> Vec<SelectionAction> {
        vec![Box::new(move |tx| {
            tx.send(AppEvent::ApplyApprovalPreset {
                approval,
                sandbox: sandbox.clone(),
            });
        })]
    }

    /// The mode the host last announced, or the one this widget's own
    /// settings stand for before any announcement.
    fn current_approval_mode(&self) -> PermissionMode {
        self.approval_mode.map_or_else(
            || PermissionMode::from_approval_policy(self.config.approval_policy),
            PermissionMode::from,
        )
    }

    pub(crate) fn approval_mode_name(&self) -> &'static str {
        self.current_approval_mode().name()
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn world_writable_warning_details(&self) -> Option<(Vec<String>, usize, bool)> {
        if self
            .config
            .notices
            .hide_world_writable_warning
            .unwrap_or(false)
        {
            return None;
        }
        let cwd = self.config.cwd.clone();
        let env_map: std::collections::HashMap<String, String> = std::env::vars().collect();
        match codex_windows_sandbox::apply_world_writable_scan_and_denies(
            self.config.nori_home.as_path(),
            cwd.as_path(),
            &env_map,
            &self.config.sandbox_policy,
            Some(self.config.nori_home.as_path()),
        ) {
            Ok(_) => None,
            Err(_) => Some((Vec::new(), 0, true)),
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code)]
    pub(crate) fn world_writable_warning_details(&self) -> Option<(Vec<String>, usize, bool)> {
        None
    }

    pub(crate) fn open_approve_all_confirmation(&mut self, session_id: Option<String>) {
        let mode = nori_protocol::ApprovalMode::ApproveAll;
        let header = ColumnRenderable::with([
            Box::new(Line::from("Approve every request in this session?").bold())
                as Box<dyn Renderable>,
            Box::new(
                Paragraph::new(vec![Line::from(vec![
                    "Nori will say yes to every edit, command and tool call the agent asks about. "
                        .into(),
                    "The sandbox setting still applies; nothing else stops a harmful request."
                        .fg(Color::Red),
                ])])
                .wrap(Wrap { trim: false }),
            ),
        ]);
        let mut accept_actions = Self::approval_mode_actions(session_id.clone(), mode);
        accept_actions.push(Box::new(|tx| {
            tx.send(AppEvent::UpdateFullAccessWarningAcknowledged(true));
        }));
        let mut remember_actions = Self::approval_mode_actions(session_id, mode);
        remember_actions.push(Box::new(|tx| {
            tx.send(AppEvent::UpdateFullAccessWarningAcknowledged(true));
            tx.send(AppEvent::PersistFullAccessWarningAcknowledged);
        }));
        let items = vec![
            SelectionItem {
                name: "Approve all".to_string(),
                description: Some("Only for this session".to_string()),
                actions: accept_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Approve all, and stop asking me this".to_string(),
                description: Some("Skip this question next time".to_string()),
                actions: remember_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Go back".to_string(),
                description: Some("Keep the current mode".to_string()),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenApprovalsPopup))],
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(header),
            ..Default::default()
        });
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn open_world_writable_warning_confirmation(
        &mut self,
        preset: Option<ApprovalPreset>,
        sample_paths: Vec<String>,
        extra_count: usize,
        failed_scan: bool,
    ) {
        let (approval, sandbox) = match &preset {
            Some(p) => (Some(p.approval), Some(p.sandbox.clone())),
            None => (None, None),
        };
        let mut header_children: Vec<Box<dyn Renderable>> = Vec::new();
        let describe_policy = |policy: &SandboxPolicy| match policy {
            SandboxPolicy::WorkspaceWrite { .. } => "Agent mode",
            SandboxPolicy::ReadOnly => "Read-Only mode",
            _ => "Agent mode",
        };
        let mode_label = preset
            .as_ref()
            .map(|p| describe_policy(&p.sandbox))
            .unwrap_or_else(|| describe_policy(&self.config.sandbox_policy));
        let info_line = if failed_scan {
            Line::from(vec![
                "We couldn't complete the world-writable scan, so protections cannot be verified. "
                    .into(),
                format!("The Windows sandbox cannot guarantee protection in {mode_label}.")
                    .fg(Color::Red),
            ])
        } else {
            Line::from(vec![
                "The Windows sandbox cannot protect writes to folders that are writable by Everyone.".into(),
                " Consider removing write access for Everyone from the following folders:".into(),
            ])
        };
        header_children.push(Box::new(
            Paragraph::new(vec![info_line]).wrap(Wrap { trim: false }),
        ));

        if !sample_paths.is_empty() {
            // Show up to three examples and optionally an "and X more" line.
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(""));
            for p in &sample_paths {
                lines.push(Line::from(format!("  - {p}")));
            }
            if extra_count > 0 {
                lines.push(Line::from(format!("and {extra_count} more")));
            }
            header_children.push(Box::new(Paragraph::new(lines).wrap(Wrap { trim: false })));
        }
        let header = ColumnRenderable::with(header_children);

        // Build actions ensuring acknowledgement happens before applying the new sandbox policy,
        // so downstream policy-change hooks don't re-trigger the warning.
        let mut accept_actions: Vec<SelectionAction> = Vec::new();
        // Suppress the immediate re-scan only when a preset will be applied (i.e., via /approvals),
        // to avoid duplicate warnings from the ensuing policy change.
        if preset.is_some() {
            accept_actions.push(Box::new(|tx| {
                tx.send(AppEvent::SkipNextWorldWritableScan);
            }));
        }
        if let (Some(approval), Some(sandbox)) = (approval, sandbox.clone()) {
            accept_actions.extend(Self::approval_preset_actions(approval, sandbox));
        }

        let mut accept_and_remember_actions: Vec<SelectionAction> = Vec::new();
        accept_and_remember_actions.push(Box::new(|tx| {
            tx.send(AppEvent::UpdateWorldWritableWarningAcknowledged(true));
            tx.send(AppEvent::PersistWorldWritableWarningAcknowledged);
        }));
        if let (Some(approval), Some(sandbox)) = (approval, sandbox) {
            accept_and_remember_actions.extend(Self::approval_preset_actions(approval, sandbox));
        }

        let items = vec![
            SelectionItem {
                name: "Continue".to_string(),
                description: Some(format!("Apply {mode_label} for this session")),
                actions: accept_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Continue and don't warn again".to_string(),
                description: Some(format!("Enable {mode_label} and remember this choice")),
                actions: accept_and_remember_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(header),
            ..Default::default()
        });
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn open_world_writable_warning_confirmation(
        &mut self,
        _preset: Option<ApprovalPreset>,
        _sample_paths: Vec<String>,
        _extra_count: usize,
        _failed_scan: bool,
    ) {
    }

    #[cfg(target_os = "windows")]
    pub(crate) fn open_windows_sandbox_enable_prompt(&mut self, preset: ApprovalPreset) {
        use ratatui_macros::line;

        let mut header = ColumnRenderable::new();
        header.push(*Box::new(
            Paragraph::new(vec![
                line!["Agent mode on Windows uses an experimental sandbox to limit network and filesystem access.".bold()],
                line![
                    "Learn more: https://github.com/tilework-tech/nori-cli/blob/main/docs/windows.md"
                ],
            ])
            .wrap(Wrap { trim: false }),
        ));

        let preset_clone = preset;
        let items = vec![
            SelectionItem {
                name: "Enable experimental sandbox".to_string(),
                description: None,
                actions: vec![Box::new(move |tx| {
                    tx.send(AppEvent::EnableWindowsSandboxForAgentMode {
                        preset: preset_clone.clone(),
                    });
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Go back".to_string(),
                description: None,
                actions: vec![Box::new(|tx| {
                    tx.send(AppEvent::OpenApprovalsPopup);
                })],
                dismiss_on_select: true,
                ..Default::default()
            },
        ];

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: None,
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(header),
            ..Default::default()
        });
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn open_windows_sandbox_enable_prompt(&mut self, _preset: ApprovalPreset) {}

    #[cfg(target_os = "windows")]
    pub(crate) fn maybe_prompt_windows_sandbox_enable(&mut self) {
        if self.config.forced_auto_mode_downgraded_on_windows
            && codex_sandbox::get_platform_sandbox().is_none()
            && let Some(preset) = builtin_approval_presets()
                .into_iter()
                .find(|preset| preset.id == "auto")
        {
            self.open_windows_sandbox_enable_prompt(preset);
        }
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn maybe_prompt_windows_sandbox_enable(&mut self) {}

    #[cfg(target_os = "windows")]
    pub(crate) fn clear_forced_auto_mode_downgrade(&mut self) {
        self.config.forced_auto_mode_downgraded_on_windows = false;
    }

    #[cfg(not(target_os = "windows"))]
    #[allow(dead_code)]
    pub(crate) fn clear_forced_auto_mode_downgrade(&mut self) {}

    /// Set the approval policy in the widget's config copy.
    pub(crate) fn set_approval_policy(&mut self, policy: AskForApproval) {
        self.config.approval_policy = policy;
        if let Some(handle) = self.harness_handle.clone() {
            tokio::spawn(async move {
                if let Err(error) = handle.set_approval_policy(policy).await {
                    tracing::warn!(%error, "failed to update harness approval policy");
                }
            });
        }
        self.update_approval_mode_label();
    }

    /// Set the sandbox policy in the widget's config copy.
    pub(crate) fn set_sandbox_policy(&mut self, policy: SandboxPolicy) {
        #[cfg(target_os = "windows")]
        let should_clear_downgrade = !matches!(policy, SandboxPolicy::ReadOnly)
            || codex_sandbox::get_platform_sandbox().is_some();

        self.config.sandbox_policy = policy;

        #[cfg(target_os = "windows")]
        if should_clear_downgrade {
            self.config.forced_auto_mode_downgraded_on_windows = false;
        }

        self.update_approval_mode_label();
    }

    /// Show the session's permission mode in the footer.
    pub(crate) fn update_approval_mode_label(&mut self) {
        let label = self.approval_mode_name().to_owned();
        self.bottom_pane.set_approval_mode_label(Some(label));
        self.refresh_host_status();
    }

    /// Change the mode of a session that has no host session, such as a
    /// cloud session, on its own harness.
    pub(crate) fn apply_local_approval_mode(&mut self, mode: nori_protocol::ApprovalMode) {
        self.approval_mode = Some(mode);
        self.set_approval_policy(PermissionMode::from(mode).approval_policy());
    }

    pub(crate) fn set_full_access_warning_acknowledged(&mut self, acknowledged: bool) {
        self.config.notices.hide_full_access_warning = Some(acknowledged);
    }

    pub(crate) fn set_world_writable_warning_acknowledged(&mut self, acknowledged: bool) {
        self.config.notices.hide_world_writable_warning = Some(acknowledged);
    }

    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub(crate) fn world_writable_warning_hidden(&self) -> bool {
        self.config
            .notices
            .hide_world_writable_warning
            .unwrap_or(false)
    }
}
