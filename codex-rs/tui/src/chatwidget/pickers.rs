use super::*;

impl ChatWidget {
    /// Open the agent picker popup for ACP mode.
    pub(crate) fn open_agent_popup(&mut self) {
        let current_model = self.config.model.clone();
        let params = crate::nori::agent_picker::agent_picker_params(
            &current_model,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
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
        let cwd = self.config.cwd.clone();
        let tx = self.app_event_tx.clone();
        let model = self.config.model.clone();

        let nori_home = match crate::nori::config_adapter::get_nori_home() {
            Ok(home) => home,
            Err(e) => {
                self.add_error_message(format!("Failed to find NORI_HOME: {e}"));
                return;
            }
        };

        let nori_home_for_event = nori_home.clone();
        tokio::spawn(async move {
            match crate::nori::resume_session_picker::load_resumable_sessions(
                &nori_home, &cwd, &model,
            )
            .await
            {
                Ok(sessions) => {
                    if sessions.is_empty() {
                        tx.send(crate::app_event::AppEvent::InsertHistoryCell(Box::new(
                            crate::history_cell::new_error_event(
                                "No resumable sessions found for this project and agent."
                                    .to_string(),
                            ),
                        )));
                    } else {
                        tx.send(crate::app_event::AppEvent::ShowResumeSessionPicker {
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

    /// Open the config popup for TUI settings.
    #[cfg(feature = "nori-config")]
    pub(crate) fn open_config_popup(&mut self, nori_config: &codex_acp::config::NoriConfig) {
        let params = crate::nori::config_picker::config_picker_params(
            nori_config,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the notify-after-idle sub-picker.
    #[cfg(feature = "nori-config")]
    pub(crate) fn open_notify_after_idle_picker(
        &mut self,
        current: codex_acp::config::NotifyAfterIdle,
    ) {
        let params = crate::nori::config_picker::notify_after_idle_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the script timeout sub-picker.
    #[cfg(feature = "nori-config")]
    pub(crate) fn open_script_timeout_picker(&mut self, current: codex_acp::config::ScriptTimeout) {
        let params = crate::nori::config_picker::script_timeout_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the loop count sub-picker.
    #[cfg(feature = "nori-config")]
    pub(crate) fn open_loop_count_picker(&mut self, current: Option<i32>) {
        let view = crate::nori::loop_count_picker::LoopCountPickerView::new(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    /// Open the footer segments picker popup.
    #[cfg(feature = "nori-config")]
    pub(crate) fn open_footer_segments_picker(
        &mut self,
        current: &codex_acp::config::FooterSegmentConfig,
    ) {
        let params = crate::nori::config_picker::footer_segments_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Replace the current footer segments picker with a refreshed one.
    ///
    /// Used after toggling a segment so the picker shows updated state without
    /// stacking a new view on top of the old one.
    #[cfg(feature = "nori-config")]
    pub(crate) fn replace_footer_segments_picker(
        &mut self,
        current: &codex_acp::config::FooterSegmentConfig,
    ) {
        let params = crate::nori::config_picker::footer_segments_picker_params(
            current,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.replace_selection_view(params);
    }

    /// Set a footer segment's enabled state.
    #[cfg(feature = "nori-config")]
    pub(crate) fn set_footer_segment_enabled(
        &mut self,
        segment: codex_acp::config::FooterSegment,
        enabled: bool,
    ) {
        self.bottom_pane
            .set_footer_segment_enabled(segment, enabled);
    }

    /// Set the loop state for a new iteration.
    #[cfg(feature = "nori-config")]
    pub(crate) fn set_loop_state(&mut self, remaining: i32, total: i32) {
        self.loop_remaining = Some(remaining);
        self.loop_total = Some(total);
    }

    /// Set the ephemeral per-session loop count override.
    #[cfg(feature = "nori-config")]
    pub(crate) fn set_loop_count_override(&mut self, value: Option<Option<i32>>) {
        self.loop_count_override = value;
    }

    /// Cancel any active loop.
    pub(super) fn cancel_loop(&mut self) {
        if self.loop_remaining.is_some() {
            self.loop_remaining = None;
            self.loop_total = None;
            self.add_info_message("Loop cancelled.".to_string(), None);
        }
    }

    /// Open the hotkey picker sub-view.
    pub(crate) fn open_hotkey_picker(&mut self, hotkey_config: codex_acp::config::HotkeyConfig) {
        let view = crate::nori::hotkey_picker::HotkeyPickerView::new(
            &hotkey_config,
            self.app_event_tx.clone(),
        );
        self.bottom_pane.show_view(Box::new(view));
    }

    /// Update the hotkey configuration used by the textarea for editing bindings.
    pub(crate) fn set_hotkey_config(&mut self, config: codex_acp::config::HotkeyConfig) {
        self.bottom_pane.set_hotkey_config(config);
    }

    pub(crate) fn set_vim_mode_enabled(&mut self, enabled: bool) {
        self.bottom_pane.set_vim_mode_enabled(enabled);
    }

    /// Handle the /switch-skillset command.
    /// Checks if nori-skillsets is available and lists available skillsets.
    pub(super) fn handle_switch_skillset_command(&mut self) {
        use crate::nori::skillset_picker;

        // Check if nori-skillsets is available in PATH
        if !skillset_picker::is_nori_skillsets_available() {
            self.add_info_message(skillset_picker::not_installed_message(), None);
            return;
        }

        // Spawn async task to list skillsets
        let tx = self.app_event_tx.clone();
        tokio::spawn(async move {
            match skillset_picker::list_skillsets().await {
                Ok(names) if names.is_empty() => {
                    tx.send(AppEvent::SkillsetListResult {
                        names: Some(vec![]),
                        error: Some("No skillsets available.".to_string()),
                    });
                }
                Ok(names) => {
                    tx.send(AppEvent::SkillsetListResult {
                        names: Some(names),
                        error: None,
                    });
                }
                Err(message) => {
                    tx.send(AppEvent::SkillsetListResult {
                        names: None,
                        error: Some(message),
                    });
                }
            }
        });
    }

    /// Handle the result of listing skillsets.
    pub(crate) fn on_skillset_list_result(
        &mut self,
        names: Option<Vec<String>>,
        error: Option<String>,
    ) {
        match (names, error) {
            (Some(names), None) if !names.is_empty() => {
                // Open the skillset picker
                let params = crate::nori::skillset_picker::skillset_picker_params(names);
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
            self.add_info_message(message.to_string(), None);
        } else {
            self.add_error_message(format!("Failed to install skillset '{name}': {message}"));
        }
    }

    /// Open the ACP model picker popup.
    pub(crate) fn open_model_popup(&mut self) {
        #[cfg(feature = "unstable")]
        {
            // ACP mode with unstable features - try to get model state from the agent
            if let Some(handle) = self.acp_handle.clone() {
                let app_event_tx = self.app_event_tx.clone();
                tokio::spawn(async move {
                    if let Some(model_state) = handle.get_model_state().await {
                        let models: Vec<crate::app_event::AcpModelInfo> = model_state
                            .available_models
                            .iter()
                            .map(|m| {
                                let display_name = if m.name.is_empty() {
                                    m.model_id.to_string()
                                } else {
                                    m.name.clone()
                                };
                                crate::app_event::AcpModelInfo {
                                    model_id: m.model_id.to_string(),
                                    display_name,
                                    description: m.description.clone(),
                                }
                            })
                            .collect();
                        let current_model_id =
                            model_state.current_model_id.map(|id| id.to_string());
                        app_event_tx.send(AppEvent::OpenAcpModelPicker {
                            models,
                            current_model_id,
                        });
                    } else {
                        // Failed to get model state - show empty picker with explanation
                        tracing::warn!("Failed to get ACP model state");
                        app_event_tx.send(AppEvent::OpenAcpModelPicker {
                            models: vec![],
                            current_model_id: None,
                        });
                    }
                });
                return;
            }
        }
        // No ACP handle or unstable not enabled - show disabled model picker
        let params = crate::nori::agent_picker::acp_model_picker_params();
        self.bottom_pane.show_selection_view(params);
    }

    /// Open the ACP model picker with fetched models.
    #[cfg(feature = "unstable")]
    pub(crate) fn open_acp_model_picker(
        &mut self,
        models: Vec<crate::app_event::AcpModelInfo>,
        current_model_id: Option<String>,
    ) {
        let params = crate::nori::agent_picker::acp_model_picker_params_with_models(
            &models,
            current_model_id.as_deref(),
        );
        self.bottom_pane.show_selection_view(params);
    }

    /// Set the ACP model via the agent handle.
    #[cfg(feature = "unstable")]
    pub(crate) fn set_acp_model(&mut self, model_id: String, display_name: String) {
        if let Some(handle) = self.acp_handle.clone() {
            let app_event_tx = self.app_event_tx.clone();
            let model_id_for_result = model_id.clone();
            let display_name_for_result = display_name.clone();
            tokio::spawn(async move {
                match handle.set_model(model_id).await {
                    Ok(()) => {
                        app_event_tx.send(AppEvent::AcpModelSetResult {
                            success: true,
                            model_id: model_id_for_result,
                            display_name: display_name_for_result,
                            error: None,
                        });
                    }
                    Err(e) => {
                        app_event_tx.send(AppEvent::AcpModelSetResult {
                            success: false,
                            model_id: model_id_for_result,
                            display_name: display_name_for_result,
                            error: Some(e.to_string()),
                        });
                    }
                }
            });
            self.add_info_message(format!("Switching to model: {display_name}..."), None);
        } else {
            self.add_info_message(
                "No ACP agent handle available for model switching".to_string(),
                None,
            );
        }
    }
}
