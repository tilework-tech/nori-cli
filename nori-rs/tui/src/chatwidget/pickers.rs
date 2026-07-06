use super::*;

static RESUME_PICKER_GENERATION: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl ChatWidget {
    /// Open the agent picker popup for ACP mode.
    pub(crate) fn open_agent_popup(&mut self) {
        let current_model = self.config.model.clone();
        let recording_enabled = nori_acp::config::NoriConfig::load()
            .map(|config| config.acp_proxy.enabled)
            .unwrap_or(false);
        self.bottom_pane
            .set_acp_wire_recording_enabled(recording_enabled);
        let params = crate::nori::agent_picker::agent_picker_params(
            &current_model,
            self.app_event_tx.clone(),
            recording_enabled,
        );
        self.bottom_pane.show_selection_view(params);
    }

    pub(crate) fn set_acp_wire_recording_enabled(&mut self, enabled: bool) {
        self.bottom_pane.set_acp_wire_recording_enabled(enabled);
    }

    pub(crate) fn replace_agent_popup(&mut self, recording_enabled: bool) {
        if !self.bottom_pane.has_active_view() {
            return;
        }
        let params = crate::nori::agent_picker::agent_picker_params(
            &self.config.model,
            self.app_event_tx.clone(),
            recording_enabled,
        );
        self.bottom_pane.replace_selection_view(params);
    }

    /// Show a selection view in the bottom pane.
    pub(crate) fn show_selection_view(&mut self, params: SelectionViewParams) {
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the viewonly session picker to select a previous session to view.
    pub(crate) fn open_viewonly_session_picker(&mut self) {
        let cwd = self.config.cwd.clone();
        let tx = self.app_event_tx.clone();

        // Get NORI_HOME - if not available, show error
        let nori_home = match crate::nori::config_adapter::get_nori_home() {
            Ok(home) => home,
            Err(e) => {
                self.add_error_message(format!("Failed to find NORI_HOME: {e}"));
                return;
            }
        };

        let nori_home_for_event = nori_home.clone();
        tokio::spawn(async move {
            match crate::nori::viewonly_session_picker::load_sessions_with_preview(&nori_home, &cwd)
                .await
            {
                Ok(sessions) => {
                    if sessions.is_empty() {
                        tx.send(crate::app_event::AppEvent::InsertHistoryCell(Box::new(
                            crate::history_cell::new_error_event(
                                "No previous sessions found for this project.".to_string(),
                            ),
                        )));
                    } else {
                        tx.send(crate::app_event::AppEvent::ShowViewonlySessionPicker {
                            sessions,
                            nori_home: nori_home_for_event,
                        });
                    }
                }
                Err(e) => {
                    tx.send(crate::app_event::AppEvent::InsertHistoryCell(Box::new(
                        crate::history_cell::new_error_event(format!(
                            "Failed to load sessions: {e}"
                        )),
                    )));
                }
            }
        });
    }

    pub(crate) fn open_resume_session_picker(&mut self) {
        // When the live agent advertises ACP `session/list`, source the picker
        // from the agent itself (and resume over ACP) instead of the local
        // transcript store. Resuming a listed session loads it over ACP via
        // `session/load`, so the agent must also advertise `load_session`;
        // without it, fall back to the local-transcript picker.
        if self.session_agent_capabilities.session_list
            && self.session_agent_capabilities.load_session
            && let Some(handle) = self.acp_handle.clone()
        {
            let cwd = self.config.cwd.clone();
            let tx = self.app_event_tx.clone();
            tokio::spawn(async move {
                match handle.list_sessions(cwd).await {
                    Ok(sessions) if sessions.is_empty() => {
                        tx.send(crate::app_event::AppEvent::InsertHistoryCell(Box::new(
                            crate::history_cell::new_error_event(
                                "The agent reported no resumable sessions.".to_string(),
                            ),
                        )));
                    }
                    Ok(sessions) => {
                        tx.send(crate::app_event::AppEvent::ShowAcpResumeSessionPicker {
                            sessions,
                        });
                    }
                    Err(e) => {
                        tx.send(crate::app_event::AppEvent::InsertHistoryCell(Box::new(
                            crate::history_cell::new_error_event(format!(
                                "Failed to list sessions from the agent: {e}"
                            )),
                        )));
                    }
                }
            });
            return;
        }

        let started = std::time::Instant::now();
        let cwd = self.config.cwd.clone();
        let tx = self.app_event_tx.clone();
        let model = self.config.model.clone();
        let generation =
            RESUME_PICKER_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        tracing::info!(
            target: "nori_resume",
            phase = "open_resume_session_picker.start",
            cwd = %cwd.display(),
            agent = %model,
            "starting /resume pre-picker session load",
        );

        let nori_home = match crate::nori::config_adapter::get_nori_home() {
            Ok(home) => home,
            Err(e) => {
                tracing::warn!(
                    target: "nori_resume",
                    phase = "open_resume_session_picker.nori_home_error",
                    elapsed_ms = started.elapsed().as_millis(),
                    error = %e,
                    "failed to resolve NORI_HOME before opening /resume picker",
                );
                self.add_error_message(format!("Failed to find NORI_HOME: {e}"));
                return;
            }
        };

        tracing::info!(
            target: "nori_resume",
            phase = "open_resume_session_picker.nori_home_resolved",
            elapsed_ms = started.elapsed().as_millis(),
            nori_home = %nori_home.display(),
            "resolved NORI_HOME for /resume picker",
        );

        let nori_home_for_event = nori_home.clone();
        tokio::spawn(async move {
            let task_started = std::time::Instant::now();
            tracing::info!(
                target: "nori_resume",
                phase = "open_resume_session_picker.load_task.start",
                cwd = %cwd.display(),
                agent = %model,
                nori_home = %nori_home.display(),
                "spawned /resume pre-picker load task",
            );
            match crate::nori::resume_session_picker::load_resumable_sessions(
                &nori_home, &cwd, &model,
            )
            .await
            {
                Ok(sessions) => {
                    tracing::info!(
                        target: "nori_resume",
                        phase = "open_resume_session_picker.load_task.loaded",
                        elapsed_ms = task_started.elapsed().as_millis(),
                        session_count = sessions.len(),
                        "loaded resumable sessions for /resume picker",
                    );
                    if sessions.is_empty() {
                        tx.send(crate::app_event::AppEvent::InsertHistoryCell(Box::new(
                            crate::history_cell::new_error_event(
                                "No resumable sessions found for this project and agent."
                                    .to_string(),
                            ),
                        )));
                    } else {
                        tx.send(crate::app_event::AppEvent::ShowResumeSessionPicker {
                            sessions: sessions.clone(),
                            nori_home: nori_home_for_event,
                            generation,
                        });
                        tracing::info!(
                            target: "nori_resume",
                            phase = "open_resume_session_picker.load_task.event_sent",
                            elapsed_ms = task_started.elapsed().as_millis(),
                            "sent ShowResumeSessionPicker event",
                        );
                        spawn_resume_summary_task(tx.clone(), nori_home, sessions, generation);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "nori_resume",
                        phase = "open_resume_session_picker.load_task.error",
                        elapsed_ms = task_started.elapsed().as_millis(),
                        error = %e,
                        "failed to load resumable sessions for /resume picker",
                    );
                    tx.send(crate::app_event::AppEvent::InsertHistoryCell(Box::new(
                        crate::history_cell::new_error_event(format!(
                            "Failed to load sessions: {e}"
                        )),
                    )));
                }
            }
        });
    }

    pub(crate) fn show_resume_session_picker(
        &mut self,
        params: SelectionViewParams,
        generation: u64,
    ) {
        self.active_resume_picker_generation = Some(generation);
        self.bottom_pane.show_selection_view(params);
    }

    /// Show the resume picker populated from the agent's ACP `session/list`.
    pub(crate) fn show_acp_resume_session_picker(
        &mut self,
        sessions: Vec<nori_acp::AcpSessionSummary>,
    ) {
        let params = crate::nori::resume_session_picker::acp_resume_session_picker_params(sessions);
        self.bottom_pane.show_selection_view(params);
    }

    pub(crate) fn update_resume_session_picker_item(
        &mut self,
        generation: u64,
        session_id: &str,
        started_at: &str,
        first_message_preview: Option<&str>,
        user_turn_count: Option<usize>,
    ) {
        if self.active_resume_picker_generation != Some(generation) {
            tracing::info!(
                target: "nori_resume",
                phase = "resume_session_summary.stale",
                generation,
                session_id,
                "ignored stale resume session summary update",
            );
            return;
        }

        if user_turn_count == Some(0) {
            self.bottom_pane.remove_selection_item(session_id);
            return;
        }

        let (name, description, search_value) =
            crate::nori::resume_session_picker::resume_session_item_update(
                session_id,
                started_at,
                first_message_preview,
                user_turn_count,
            );
        self.bottom_pane
            .update_selection_item(session_id, name, description, search_value);
    }

    /// Open the Nori CLI settings popup.
    pub(crate) fn open_settings_popup(&mut self, nori_config: &nori_acp::config::NoriConfig) {
        let params = crate::nori::config_picker::config_picker_params(
            nori_config,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the file manager sub-picker.
    pub(crate) fn open_file_manager_picker(
        &mut self,
        current: Option<nori_acp::config::FileManager>,
    ) {
        let params = crate::nori::config_picker::file_manager_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the vim mode sub-picker.
    pub(crate) fn open_vim_mode_picker(&mut self, current: nori_acp::config::VimEnterBehavior) {
        let params =
            crate::nori::config_picker::vim_mode_picker_params(current, self.app_event_tx.clone());
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the auto-worktree sub-picker.
    pub(crate) fn open_auto_worktree_picker(&mut self, current: nori_acp::config::AutoWorktree) {
        let params = crate::nori::config_picker::auto_worktree_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the notify-after-idle sub-picker.
    pub(crate) fn open_notify_after_idle_picker(
        &mut self,
        current: nori_acp::config::NotifyAfterIdle,
    ) {
        let params = crate::nori::config_picker::notify_after_idle_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the script timeout sub-picker.
    pub(crate) fn open_script_timeout_picker(&mut self, current: nori_acp::config::ScriptTimeout) {
        let params = crate::nori::config_picker::script_timeout_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the loop count sub-picker.
    pub(crate) fn open_loop_count_picker(&mut self, current: Option<i32>) {
        let view = crate::nori::loop_count_picker::LoopCountPickerView::new(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    /// Open the footer segments picker popup.
    pub(crate) fn open_footer_segments_picker(
        &mut self,
        current: &nori_acp::config::FooterSegmentConfig,
    ) {
        let params = crate::nori::config_picker::footer_segments_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the worktree choice picker for per-session skillsets.
    pub(crate) fn open_skillset_worktree_choice_picker(&mut self) {
        let params =
            crate::nori::config_picker::skillset_worktree_choice_params(self.app_event_tx.clone());
        self.bottom_pane.show_selection_view(params);
    }

    /// Replace the current footer segments picker with a refreshed one.
    ///
    /// Used after toggling a segment so the picker shows updated state without
    /// stacking a new view on top of the old one.
    pub(crate) fn replace_footer_segments_picker(
        &mut self,
        current: &nori_acp::config::FooterSegmentConfig,
    ) {
        let params = crate::nori::config_picker::footer_segments_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.replace_selection_view(params);
    }

    /// Set a footer segment's enabled state.
    pub(crate) fn set_footer_segment_enabled(
        &mut self,
        segment: nori_acp::config::FooterSegment,
        enabled: bool,
    ) {
        self.bottom_pane
            .set_footer_segment_enabled(segment, enabled);
    }

    /// Set the loop state for a new iteration.
    pub(crate) fn set_loop_state(&mut self, remaining: i32, total: i32) {
        self.loop_remaining = Some(remaining);
        self.loop_total = Some(total);
    }

    /// Set the ephemeral per-session loop count override.
    pub(crate) fn set_loop_count_override(&mut self, value: Option<Option<i32>>) {
        self.loop_count_override = value;
    }

    /// Cancel any active loop.
    pub(super) fn cancel_loop(&mut self) {
        if self.loop_remaining.is_some() {
            tracing::info!(remaining = ?self.loop_remaining, "loop cancelled");
            self.loop_remaining = None;
            self.loop_total = None;
            self.add_info_message("Loop cancelled.".to_string(), None);
        }
    }

    /// Open the MCP server management popup.
    pub(crate) fn open_mcp_servers_popup(&mut self) {
        let servers: std::collections::BTreeMap<
            String,
            codex_core::config::types::McpServerConfig,
        > = self
            .config
            .mcp_servers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let view = crate::nori::mcp_server_picker::McpServerPickerView::new(
            &servers,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_view(Box::new(view));
        self.app_event_tx
            .send(crate::app_event::AppEvent::ComputeMcpAuthStatuses);
    }

    /// Open the hotkey picker sub-view.
    pub(crate) fn open_hotkey_picker(&mut self, hotkey_config: nori_acp::config::HotkeyConfig) {
        let view = crate::nori::hotkey_picker::HotkeyPickerView::new(
            &hotkey_config,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    /// Update the hotkey configuration used by the textarea for editing bindings.
    pub(crate) fn set_hotkey_config(&mut self, config: nori_acp::config::HotkeyConfig) {
        self.bottom_pane.set_hotkey_config(config);
    }

    pub(crate) fn set_vim_mode(&mut self, value: nori_acp::config::VimEnterBehavior) {
        self.bottom_pane.set_vim_mode(value);
    }

    /// Handle the /switch-skillset command.
    /// Checks if nori-skillsets is available and lists available skillsets.
    pub(crate) fn handle_switch_skillset_command(&mut self) {
        use crate::nori::skillset_picker;

        // Check if nori-skillsets is available in PATH
        if !skillset_picker::is_nori_skillsets_available() {
            self.add_info_message(skillset_picker::not_installed_message(), None);
            return;
        }

        // Detect if we're in a worktree or if skillset_per_session is enabled,
        // and pass cwd as the install directory
        let is_in_worktree = crate::system_info::extract_worktree_name(&self.config.cwd).is_some();
        let skillset_per_session = nori_acp::config::NoriConfig::load()
            .map(|c| c.skillset_per_session)
            .unwrap_or(false);
        let install_dir = if is_in_worktree || skillset_per_session {
            Some(self.config.cwd.clone())
        } else {
            None
        };

        // Spawn async task to list skillsets
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            match skillset_picker::list_skillsets().await {
                Ok(names) if names.is_empty() => {
                    tx.send(AppEvent::SkillsetListResult {
                        names: Some(vec![]),
                        error: Some("No skillsets available.".to_string()),
                        install_dir,
                    });
                }
                Ok(names) => {
                    tx.send(AppEvent::SkillsetListResult {
                        names: Some(names),
                        error: None,
                        install_dir,
                    });
                }
                Err(message) => {
                    tx.send(AppEvent::SkillsetListResult {
                        names: None,
                        error: Some(message),
                        install_dir,
                    });
                }
            }
        });
    }

    /// Handle `/switch-skillset <name>` — directly switch to a named skillset
    /// without showing the picker menu.
    pub(crate) fn handle_switch_skillset_command_with_name(&mut self, name: &str) {
        use crate::nori::skillset_picker;

        if !skillset_picker::is_nori_skillsets_available() {
            self.add_info_message(skillset_picker::not_installed_message(), None);
            return;
        }

        let is_in_worktree = crate::system_info::extract_worktree_name(&self.config.cwd).is_some();
        let skillset_per_session = nori_acp::config::NoriConfig::load()
            .map(|c| c.skillset_per_session)
            .unwrap_or(false);
        let install_dir = if is_in_worktree || skillset_per_session {
            Some(self.config.cwd.clone())
        } else {
            None
        };

        if let Some(dir) = install_dir {
            self.on_switch_skillset_request(name, &dir);
        } else {
            self.on_install_skillset_request(name);
        }
    }

    /// Handle the result of listing skillsets.
    pub(crate) fn on_skillset_list_result(
        &mut self,
        names: Option<Vec<String>>,
        error: Option<String>,
        install_dir: Option<PathBuf>,
    ) {
        match (names, error) {
            (Some(names), None) if !names.is_empty() => {
                // Determine whether to show "No Skillset" option and worktree warning.
                // These are relevant when skillset_per_session is enabled (indicated
                // by the on_dismiss callback being set, which only happens in the
                // per-session startup flow).
                let nori_cfg = nori_acp::config::NoriConfig::load().unwrap_or_default();
                let is_per_session = nori_cfg.skillset_per_session;
                let auto_worktree_off = is_per_session && !nori_cfg.auto_worktree.is_enabled();

                let on_dismiss: SelectionAction = Box::new(|tx| {
                    tx.send(AppEvent::SkillsetPickerDismissed);
                });
                let params = crate::nori::skillset_picker::skillset_picker_params(
                    names,
                    install_dir,
                    Some(on_dismiss),
                    is_per_session,
                    auto_worktree_off,
                );
                self.bottom_pane.show_selection_view(params);
            }
            (_, Some(error)) => {
                self.add_error_message(error);
            }
            _ => {
                self.add_info_message("No skillsets available.".to_string(), None);
            }
        }
    }

    /// Handle a request to install a skillset.
    pub(crate) fn on_install_skillset_request(&mut self, name: &str) {
        use crate::nori::skillset_picker;

        let name = name.to_string();
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            match skillset_picker::install_skillset(&name).await {
                Ok(message) => {
                    tx.send(AppEvent::SkillsetInstallResult {
                        name,
                        success: true,
                        message,
                    });
                }
                Err(message) => {
                    tx.send(AppEvent::SkillsetInstallResult {
                        name,
                        success: false,
                        message,
                    });
                }
            }
        });
    }

    /// Handle the result of installing a skillset.
    pub(crate) fn on_skillset_install_result(&mut self, name: &str, success: bool, message: &str) {
        if success {
            self.add_to_history(history_cell::new_skillset_switched_event(name));
            self.request_redraw();
        } else {
            self.add_error_message(format!("Failed to install skillset '{name}': {message}"));
        }
    }

    /// Handle a request to switch to a skillset with a specific install directory.
    pub(crate) fn on_switch_skillset_request(&mut self, name: &str, install_dir: &std::path::Path) {
        use crate::nori::skillset_picker;

        let name = name.to_string();
        let install_dir = install_dir.to_path_buf();
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            match skillset_picker::switch_skillset(&name, &install_dir).await {
                Ok(message) => {
                    tx.send(AppEvent::SkillsetSwitchResult {
                        name,
                        success: true,
                        message,
                    });
                }
                Err(message) => {
                    tx.send(AppEvent::SkillsetSwitchResult {
                        name,
                        success: false,
                        message,
                    });
                }
            }
        });
    }

    /// Handle the result of switching a skillset.
    pub(crate) fn on_skillset_switch_result(&mut self, name: &str, success: bool, message: &str) {
        if success {
            self.add_to_history(history_cell::new_skillset_switched_event(name));
            self.request_redraw();
        } else {
            self.add_error_message(format!("Failed to switch to skillset '{name}': {message}"));
        }
    }

    /// Open the ACP model picker popup.
    ///
    /// Uses the stable `config_options` mechanism: if the agent advertises a
    /// Model-category config option, open the value picker for it. Otherwise
    /// show a static "not supported" message.
    pub(crate) fn open_model_popup(&mut self) {
        // An agent switch is pending: the new session hasn't started, so the new
        // agent's (session-scoped) models aren't available yet. Querying the live
        // handle would show the OLD agent's models, so explain instead.
        if let Some(pending) = self.pending_agent.as_ref() {
            let params = crate::nori::agent_picker::acp_model_picker_pending_agent_params(
                &pending.display_name,
            );
            self.bottom_pane.show_selection_view(params);
            return;
        }
        if let Some(handle) = self.acp_handle.clone() {
            let app_event_tx = self.app_event_tx.clone();
            tokio::spawn(async move {
                if let Some(config_options) = handle.get_session_config().await {
                    let model_option = config_options.into_iter().find(|opt| {
                        opt.category == Some(nori_acp::SessionConfigOptionCategory::Model)
                    });
                    if let Some(option) = model_option {
                        app_event_tx.send(AppEvent::OpenAcpSessionConfigValuePicker { option });
                        return;
                    }
                }

                // The agent does not advertise a Model-category config option,
                // so model selection is not supported.
                app_event_tx.send(AppEvent::OpenAcpModelPickerUnsupported);
            });
            return;
        }
        self.open_model_unsupported_popup();
    }

    /// Show the static "model switching not supported" picker.
    pub(crate) fn open_model_unsupported_popup(&mut self) {
        let params = crate::nori::agent_picker::acp_model_picker_params();
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the generic ACP session-config picker.
    pub(crate) fn open_session_config_popup(&mut self) {
        if let Some(handle) = self.acp_handle.clone() {
            let app_event_tx = self.app_event_tx.clone();
            tokio::spawn(async move {
                let config_options = handle.get_session_config().await.unwrap_or_default();
                app_event_tx.send(AppEvent::OpenAcpSessionConfigPicker { config_options });
            });
            return;
        }

        let params = crate::nori::session_config_picker::acp_session_config_picker_params(&[]);
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the top-level ACP session-config picker with the current config snapshot.
    pub(crate) fn open_acp_session_config_picker(
        &mut self,
        config_options: Vec<nori_acp::SessionConfigOption>,
    ) {
        let params =
            crate::nori::session_config_picker::acp_session_config_picker_params(&config_options);
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the value picker for one ACP session config option.
    pub(crate) fn open_acp_session_config_value_picker(
        &mut self,
        option: nori_acp::SessionConfigOption,
    ) {
        let params =
            crate::nori::session_config_picker::acp_session_config_value_picker_params(&option);
        self.bottom_pane.show_selection_view(params);
    }

    /// Set an ACP session config option via the agent handle.
    pub(crate) fn set_acp_session_config_option(
        &mut self,
        config_id: String,
        value: String,
        option_name: String,
        value_name: String,
    ) {
        if let Some(handle) = self.acp_handle.clone() {
            let app_event_tx = self.app_event_tx.clone();
            let generation = self.acp_mode_config_generation;
            let agent = self.config.model.clone();
            let option_name_for_result = option_name;
            let value_name_for_result = value_name;
            let config_id_for_result = config_id.clone();
            let value_for_result = value.clone();
            tokio::spawn(async move {
                match handle.set_session_config_option(config_id, value).await {
                    Ok(config_options) => {
                        app_event_tx.send(AppEvent::AcpModeConfigSnapshot {
                            generation,
                            mode: crate::nori::session_config_mode::acp_mode_config_from_options(
                                &config_options,
                            ),
                        });
                        app_event_tx.send(AppEvent::AcpSessionConfigSetResult {
                            success: true,
                            agent,
                            config_id: config_id_for_result,
                            value: value_for_result,
                            option_name: option_name_for_result,
                            value_name: value_name_for_result,
                            config_options: Some(config_options),
                            error: None,
                        });
                    }
                    Err(err) => {
                        app_event_tx.send(AppEvent::AcpSessionConfigSetResult {
                            success: false,
                            agent,
                            config_id: config_id_for_result,
                            value: value_for_result,
                            option_name: option_name_for_result,
                            value_name: value_name_for_result,
                            config_options: None,
                            error: Some(err.to_string()),
                        });
                    }
                }
            });
        } else {
            self.add_info_message(
                "No ACP agent handle available for session config".to_string(),
                None,
            );
        }
    }
}

fn spawn_resume_summary_task(
    tx: AppEventSender,
    nori_home: std::path::PathBuf,
    sessions: Vec<crate::nori::viewonly_session_picker::SessionPickerInfo>,
    generation: u64,
) {
    tokio::spawn(async move {
        let loader = nori_acp::transcript::TranscriptLoader::new(nori_home);
        let mut previews = std::collections::HashMap::new();

        for session in &sessions {
            let preview = loader
                .load_first_user_preview(&session.project_id, &session.session_id)
                .await
                .ok()
                .flatten();
            previews.insert(session.session_id.clone(), preview.clone());
            tx.send(crate::app_event::AppEvent::ResumeSessionSummaryReady {
                generation,
                session_id: session.session_id.clone(),
                started_at: session.started_at.clone(),
                first_message_preview: preview,
                user_turn_count: None,
            });
        }

        for session in sessions {
            let user_turn_count = loader
                .count_user_turns(&session.project_id, &session.session_id)
                .await
                .ok();
            let preview = previews.remove(&session.session_id).flatten();
            tx.send(crate::app_event::AppEvent::ResumeSessionSummaryReady {
                generation,
                session_id: session.session_id,
                started_at: session.started_at,
                first_message_preview: preview,
                user_turn_count,
            });
        }
    });
}
