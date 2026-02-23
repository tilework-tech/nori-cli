use super::*;

impl ChatWidget {
    /// Open a popup to choose the approvals mode (ask for approval policy + sandbox policy).
    pub(crate) fn open_approvals_popup(&mut self) {
        let current_approval = self.config.approval_policy;
        let current_sandbox = self.config.sandbox_policy.clone();
        let mut items: Vec<SelectionItem> = Vec::new();
        let presets: Vec<ApprovalPreset> = builtin_approval_presets();
        for preset in presets.into_iter() {
            let is_current =
                Self::preset_matches_current(current_approval, &current_sandbox, &preset);
            let name = preset.label.to_string();
            let description_text = preset.description;
            let description = Some(description_text.to_string());
            let requires_confirmation = preset.id == "full-access"
                && !self
                    .config
                    .notices
                    .hide_full_access_warning
                    .unwrap_or(false);
            let actions: Vec<SelectionAction> = if requires_confirmation {
                let preset_clone = preset.clone();
                vec![Box::new(move |tx| {
                    tx.send(AppEvent::OpenFullAccessConfirmation {
                        preset: preset_clone.clone(),
                    });
                })]
            } else if preset.id == "auto" {
                #[cfg(target_os = "windows")]
                {
                    if codex_core::get_platform_sandbox().is_none() {
                        let preset_clone = preset.clone();
                        vec![Box::new(move |tx| {
                            tx.send(AppEvent::OpenWindowsSandboxEnablePrompt {
                                preset: preset_clone.clone(),
                            });
                        })]
                    } else if let Some((sample_paths, extra_count, failed_scan)) =
                        self.world_writable_warning_details()
                    {
                        let preset_clone = preset.clone();
                        vec![Box::new(move |tx| {
                            tx.send(AppEvent::OpenWorldWritableWarningConfirmation {
                                preset: Some(preset_clone.clone()),
                                sample_paths: sample_paths.clone(),
                                extra_count,
                                failed_scan,
                            });
                        })]
                    } else {
                        Self::approval_preset_actions(preset.approval, preset.sandbox.clone())
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    Self::approval_preset_actions(preset.approval, preset.sandbox.clone())
                }
            } else {
                Self::approval_preset_actions(preset.approval, preset.sandbox.clone())
            };
            items.push(SelectionItem {
                name,
                description,
                is_current,
                actions,
                dismiss_on_select: true,
                ..Default::default()
            });
        }

        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some("Select Approval Mode".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            items,
            header: Box::new(()),
            ..Default::default()
        });
    }

    pub(super) fn approval_preset_actions(
        approval: AskForApproval,
        sandbox: SandboxPolicy,
    ) -> Vec<SelectionAction> {
        vec![Box::new(move |tx| {
            let sandbox_clone = sandbox.clone();
            tx.send(AppEvent::CodexOp(Op::OverrideTurnContext {
                cwd: None,
                approval_policy: Some(approval),
                sandbox_policy: Some(sandbox_clone.clone()),
                model: None,
                effort: None,
                summary: None,
            }));
            tx.send(AppEvent::UpdateAskForApprovalPolicy(approval));
            tx.send(AppEvent::UpdateSandboxPolicy(sandbox_clone));
        })]
    }

    pub(super) fn preset_matches_current(
        current_approval: AskForApproval,
        current_sandbox: &SandboxPolicy,
        preset: &ApprovalPreset,
    ) -> bool {
        if current_approval != preset.approval {
            return false;
        }
        matches!(
            (&preset.sandbox, current_sandbox),
            (SandboxPolicy::ReadOnly, SandboxPolicy::ReadOnly)
                | (
                    SandboxPolicy::DangerFullAccess,
                    SandboxPolicy::DangerFullAccess
                )
                | (
                    SandboxPolicy::WorkspaceWrite { .. },
                    SandboxPolicy::WorkspaceWrite { .. }
                )
        )
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
            self.config.codex_home.as_path(),
            cwd.as_path(),
            &env_map,
            &self.config.sandbox_policy,
            Some(self.config.codex_home.as_path()),
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

    pub(crate) fn open_full_access_confirmation(&mut self, preset: ApprovalPreset) {
        let approval = preset.approval;
        let sandbox = preset.sandbox;
        let mut header_children: Vec<Box<dyn Renderable>> = Vec::new();
        let title_line = Line::from("Enable full access?").bold();
        let info_line = Line::from(vec![
            "When Nori runs with full access, it can edit any file on your computer and run commands with network, without your approval."
                .into(),
            "Exercise caution when enabling full access. This significantly increases the risk of data loss, leaks, or unexpected behavior."
                .fg(Color::Red),
        ]);
        header_children.push(Box::new(title_line));
        header_children.push(Box::new(
            Paragraph::new(vec![info_line]).wrap(Wrap { trim: false }),
        ));
        let header = ColumnRenderable::with(header_children);

        let mut accept_actions = Self::approval_preset_actions(approval, sandbox.clone());
        accept_actions.push(Box::new(|tx| {
            tx.send(AppEvent::UpdateFullAccessWarningAcknowledged(true));
        }));

        let mut accept_and_remember_actions = Self::approval_preset_actions(approval, sandbox);
        accept_and_remember_actions.push(Box::new(|tx| {
            tx.send(AppEvent::UpdateFullAccessWarningAcknowledged(true));
            tx.send(AppEvent::PersistFullAccessWarningAcknowledged);
        }));

        let deny_actions: Vec<SelectionAction> = vec![Box::new(|tx| {
            tx.send(AppEvent::OpenApprovalsPopup);
        })];

        let items = vec![
            SelectionItem {
                name: "Yes, continue anyway".to_string(),
                description: Some("Apply full access for this session".to_string()),
                actions: accept_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Yes, and don't ask again".to_string(),
                description: Some("Enable full access and remember this choice".to_string()),
                actions: accept_and_remember_actions,
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Cancel".to_string(),
                description: Some("Go back without enabling full access".to_string()),
                actions: deny_actions,
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
            && codex_core::get_platform_sandbox().is_none()
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
        self.update_approval_mode_label();
    }

    /// Set the sandbox policy in the widget's config copy.
    pub(crate) fn set_sandbox_policy(&mut self, policy: SandboxPolicy) {
        #[cfg(target_os = "windows")]
        let should_clear_downgrade = !matches!(policy, SandboxPolicy::ReadOnly)
            || codex_core::get_platform_sandbox().is_some();

        self.config.sandbox_policy = policy;

        #[cfg(target_os = "windows")]
        if should_clear_downgrade {
            self.config.forced_auto_mode_downgraded_on_windows = false;
        }

        self.update_approval_mode_label();
    }

    /// Update the approval mode label displayed in the footer based on current config.
    pub(super) fn update_approval_mode_label(&mut self) {
        let label = approval_mode_label(self.config.approval_policy, &self.config.sandbox_policy);
        self.bottom_pane.set_approval_mode_label(label);
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

    /// Set the reasoning effort in the widget's config copy.
    pub(crate) fn set_reasoning_effort(&mut self, effort: Option<ReasoningEffortConfig>) {
        self.config.model_reasoning_effort = effort;
    }
}
