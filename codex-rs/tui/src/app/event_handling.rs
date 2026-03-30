use super::*;

impl App {
    pub(crate) async fn handle_tui_event(
        &mut self,
        tui: &mut tui::Tui,
        event: TuiEvent,
    ) -> Result<bool> {
        if self.overlay.is_some() {
            let _ = self.handle_backtrack_overlay_event(tui, event).await?;
        } else {
            match event {
                TuiEvent::Key(key_event) => {
                    self.handle_key_event(tui, key_event).await;
                }
                TuiEvent::Paste(pasted) => {
                    // Many terminals convert newlines to \r when pasting (e.g., iTerm2),
                    // but tui-textarea expects \n. Normalize CR to LF.
                    // [tui-textarea]: https://github.com/rhysd/tui-textarea/blob/4d18622eeac13b309e0ff6a55a46ac6706da68cf/src/textarea.rs#L782-L783
                    // [iTerm2]: https://github.com/gnachman/iTerm2/blob/5d0c0d9f68523cbd0494dad5422998964a2ecd8d/sources/iTermPasteHelper.m#L206-L216
                    let pasted = pasted.replace("\r", "\n");
                    self.chat_widget.handle_paste(pasted);
                }
                TuiEvent::Draw => {
                    self.chat_widget.pre_draw_tick();
                    self.chat_widget.maybe_post_pending_notification(tui);
                    if self
                        .chat_widget
                        .handle_paste_burst_tick(tui.frame_requester())
                    {
                        return Ok(true);
                    }
                    tui.draw(
                        self.chat_widget.desired_height(tui.terminal.size()?.width),
                        |frame| {
                            self.chat_widget.render(frame.area(), frame.buffer);
                            if let Some((x, y)) = self.chat_widget.cursor_pos(frame.area()) {
                                frame.set_cursor_position((x, y));
                            }
                        },
                    )?;
                }
            }
        }
        Ok(true)
    }

    pub(super) fn apply_approval_preset(
        &mut self,
        approval: AskForApproval,
        sandbox: SandboxPolicy,
    ) {
        self.config.approval_policy = approval;
        self.config.sandbox_policy = sandbox.clone();
        #[cfg(target_os = "windows")]
        if !matches!(sandbox, codex_core::protocol::SandboxPolicy::ReadOnly)
            || codex_core::get_platform_sandbox().is_some()
        {
            self.config.forced_auto_mode_downgraded_on_windows = false;
        }
        self.chat_widget.set_approval_policy(approval);
        self.chat_widget.set_sandbox_policy(sandbox.clone());
        self.chat_widget.submit_op(Op::OverrideTurnContext {
            cwd: None,
            approval_policy: Some(approval),
            sandbox_policy: Some(sandbox),
            model: None,
            effort: None,
            summary: None,
        });
    }

    pub(super) async fn handle_event(
        &mut self,
        tui: &mut tui::Tui,
        event: AppEvent,
    ) -> Result<bool> {
        match event {
            AppEvent::NewSession => {
                let summary = session_summary(
                    self.chat_widget.token_usage(),
                    self.chat_widget.conversation_id(),
                );
                self.shutdown_current_conversation();
                let init = crate::chatwidget::ChatWidgetInit {
                    config: self.config.clone(),
                    frame_requester: tui.frame_requester(),
                    app_event_tx: self.app_event_tx.clone(),
                    initial_prompt: None,
                    initial_images: Vec::new(),
                    enhanced_keys_supported: self.enhanced_keys_supported,
                    auth_manager: self.auth_manager.clone(),
                    vertical_footer: self.vertical_footer,
                    expected_agent: None, // No filtering for /new command
                    deferred_spawn: false,
                    fork_context: None,
                };
                self.chat_widget = ChatWidget::new(init);
                self.chat_widget
                    .set_hotkey_config(self.hotkey_config.clone());
                self.chat_widget.set_vim_mode(self.vim_mode);
                self.chat_widget.set_plan_drawer_mode(self.plan_drawer_mode);
                #[cfg(feature = "nori-config")]
                self.chat_widget
                    .set_loop_count_override(self.loop_count_override);
                if let Some(summary) = summary {
                    let mut lines: Vec<Line<'static>> = vec![summary.usage_line.clone().into()];
                    if let Some(command) = summary.resume_command {
                        let spans = vec!["To continue this session, run ".into(), command.cyan()];
                        lines.push(spans.into());
                    }
                    self.chat_widget.add_plain_history_lines(lines);
                }
                tui.frame_requester().schedule_frame();
            }
            AppEvent::InsertHistoryCell(cell) => {
                let cell: Arc<dyn HistoryCell> = cell.into();
                if let Some(Overlay::Transcript(t)) = &mut self.overlay {
                    t.insert_cell(cell.clone());
                    tui.frame_requester().schedule_frame();
                }
                self.transcript_cells.push(cell.clone());
                let mut display = cell.display_lines(tui.terminal.last_known_screen_size.width);
                if !display.is_empty() {
                    // Only insert a separating blank line for new cells that are not
                    // part of an ongoing stream. Streaming continuations should not
                    // accrue extra blank lines between chunks.
                    if !cell.is_stream_continuation() {
                        if self.has_emitted_history_lines {
                            display.insert(0, Line::from(""));
                        } else {
                            self.has_emitted_history_lines = true;
                        }
                    }
                    if self.overlay.is_some() {
                        self.deferred_history_lines.extend(display);
                    } else {
                        tui.insert_history_lines(display);
                    }
                }
            }
            AppEvent::StartCommitAnimation => {
                if self
                    .commit_anim_running
                    .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    let tx = self.app_event_tx.clone();
                    let running = self.commit_anim_running.clone();
                    thread::spawn(move || {
                        while running.load(Ordering::Relaxed) {
                            thread::sleep(Duration::from_millis(50));
                            tx.send(AppEvent::CommitTick);
                        }
                    });
                }
            }
            AppEvent::StopCommitAnimation => {
                self.commit_anim_running.store(false, Ordering::Release);
            }
            AppEvent::CommitTick => {
                self.chat_widget.on_commit_tick();
            }
            AppEvent::CodexEvent(event) => {
                if self.suppress_shutdown_complete {
                    if matches!(event.msg, EventMsg::ShutdownComplete) {
                        self.suppress_shutdown_complete = false;
                        return Ok(true);
                    }
                    if matches!(event.msg, EventMsg::TurnAborted(_)) {
                        return Ok(true);
                    }
                }
                self.chat_widget.handle_codex_event(event);
            }
            AppEvent::ClientEvent(event) => {
                self.chat_widget.handle_client_event(event);
            }
            AppEvent::ConversationHistory(ev) => {
                self.on_conversation_history_for_backtrack(tui, ev)?;
            }
            AppEvent::ExitRequest => {
                // Create and insert exit message cell before exiting
                let exit_cell = self.chat_widget.create_exit_message_cell();

                // Insert the cell directly (inline the InsertHistoryCell logic to avoid recursion)
                let cell: Arc<dyn HistoryCell> = exit_cell.into();
                if let Some(Overlay::Transcript(t)) = &mut self.overlay {
                    t.insert_cell(cell.clone());
                }
                self.transcript_cells.push(cell.clone());
                let mut display = cell.display_lines(tui.terminal.last_known_screen_size.width);
                if !display.is_empty() {
                    if !cell.is_stream_continuation() {
                        if self.has_emitted_history_lines {
                            display.insert(0, Line::from(""));
                        } else {
                            self.has_emitted_history_lines = true;
                        }
                    }
                    if self.overlay.is_some() {
                        self.deferred_history_lines.extend(display);
                    } else {
                        tui.insert_history_lines(display);
                    }
                }

                // Force immediate synchronous draw to flush all history lines to scrollback
                // This will temporarily show the bottom pane in the viewport
                tui.draw(
                    self.chat_widget.desired_height(tui.terminal.size()?.width),
                    |frame| {
                        self.chat_widget.render(frame.area(), frame.buffer);
                        if let Some((x, y)) = self.chat_widget.cursor_pos(frame.area()) {
                            frame.set_cursor_position((x, y));
                        }
                    },
                )?;

                // Clear the viewport to remove the bottom pane, but keep scrollback intact
                tui.terminal.clear()?;

                // Exit the application
                return Ok(false);
            }
            AppEvent::CodexOp(op) => self.chat_widget.submit_op(op),
            AppEvent::DiffResult(text) => {
                // Clear the in-progress state in the bottom pane
                self.chat_widget.on_diff_complete();
                // Enter alternate screen using TUI helper and build pager lines
                let _ = tui.enter_alt_screen();
                let pager_lines: Vec<ratatui::text::Line<'static>> = if text.trim().is_empty() {
                    vec!["No changes detected.".italic().into()]
                } else {
                    text.lines().map(ansi_escape_line).collect()
                };
                self.overlay = Some(Overlay::new_static_with_lines(
                    pager_lines,
                    "D I F F".to_string(),
                ));
                tui.frame_requester().schedule_frame();
            }
            AppEvent::StartFileSearch(query) => {
                if !query.is_empty() {
                    self.file_search.on_user_query(query);
                }
            }
            AppEvent::FileSearchResult { query, matches } => {
                self.chat_widget.apply_file_search_result(query, matches);
            }
            AppEvent::SystemInfoRefreshed(info) => {
                if !self.worktree_warning_shown
                    && let Some(warning) = &info.worktree_cleanup_warning
                {
                    let free = warning.free_percent;
                    let count = warning.worktree_count;
                    let message = format!(
                        "Low disk space: {free}% free. You have {count} git worktree(s) that may be consuming disk space. \
                         Consider running `git worktree remove <path>` to clean up unused worktrees.",
                    );
                    self.chat_widget.add_warning_message(message);
                    self.worktree_warning_shown = true;
                }
                self.chat_widget.apply_system_info_refresh(info);
            }
            AppEvent::RefreshSystemInfoForDirectory { dir, agent } => {
                self.request_system_info_refresh(dir, agent, self.chat_widget.first_prompt_text());
            }
            AppEvent::RateLimitSnapshotFetched(snapshot) => {
                self.chat_widget.on_rate_limit_snapshot(Some(snapshot));
            }
            AppEvent::UpdateReasoningEffort(effort) => {
                self.on_update_reasoning_effort(effort);
            }
            AppEvent::UpdateAgent(model) => {
                self.chat_widget.set_agent(&model);
                self.config.model = model.clone();
                if let Some(family) = find_family_for_model(&model) {
                    self.config.model_family = family;
                }
            }
            AppEvent::OpenFullAccessConfirmation { preset } => {
                self.chat_widget.open_full_access_confirmation(preset);
            }
            AppEvent::OpenWorldWritableWarningConfirmation {
                preset,
                sample_paths,
                extra_count,
                failed_scan,
            } => {
                self.chat_widget.open_world_writable_warning_confirmation(
                    preset,
                    sample_paths,
                    extra_count,
                    failed_scan,
                );
            }
            AppEvent::OpenWindowsSandboxEnablePrompt { preset } => {
                self.chat_widget.open_windows_sandbox_enable_prompt(preset);
            }
            AppEvent::EnableWindowsSandboxForAgentMode { preset } => {
                #[cfg(target_os = "windows")]
                {
                    let profile = self.active_profile.as_deref();
                    let feature_key = Feature::WindowsSandbox.key();
                    match ConfigEditsBuilder::new(&self.config.codex_home)
                        .with_profile(profile)
                        .set_feature_enabled(feature_key, true)
                        .apply()
                        .await
                    {
                        Ok(()) => {
                            self.config.set_windows_sandbox_globally(true);
                            self.chat_widget.clear_forced_auto_mode_downgrade();
                            if let Some((sample_paths, extra_count, failed_scan)) =
                                self.chat_widget.world_writable_warning_details()
                            {
                                self.app_event_tx.send(
                                    AppEvent::OpenWorldWritableWarningConfirmation {
                                        preset: Some(preset.clone()),
                                        sample_paths,
                                        extra_count,
                                        failed_scan,
                                    },
                                );
                            } else {
                                self.app_event_tx.send(AppEvent::ApplyApprovalPreset {
                                    approval: preset.approval,
                                    sandbox: preset.sandbox.clone(),
                                });
                                self.chat_widget.add_info_message(
                                    "Enabled experimental Windows sandbox.".to_string(),
                                    None,
                                );
                            }
                        }
                        Err(err) => {
                            tracing::error!(
                                error = %err,
                                "failed to enable Windows sandbox feature"
                            );
                            self.chat_widget.add_error_message(format!(
                                "Failed to enable the Windows sandbox feature: {err}"
                            ));
                        }
                    }
                }
                #[cfg(not(target_os = "windows"))]
                {
                    let _ = preset;
                }
            }
            AppEvent::PersistAgentSelection {
                agent: model,
                effort,
            } => {
                let profile = self.active_profile.as_deref();
                match ConfigEditsBuilder::new(&self.config.codex_home)
                    .with_profile(profile)
                    .set_model(Some(model.as_str()), effort)
                    .apply()
                    .await
                {
                    Ok(()) => {
                        let reasoning_label = Self::reasoning_label(effort);
                        if let Some(profile) = profile {
                            self.chat_widget.add_info_message(
                                format!(
                                    "Model changed to {model} {reasoning_label} for {profile} profile"
                                ),
                                None,
                            );
                        } else {
                            self.chat_widget.add_info_message(
                                format!("Model changed to {model} {reasoning_label}"),
                                None,
                            );
                        }
                    }
                    Err(err) => {
                        tracing::error!(
                            error = %err,
                            "failed to persist model selection"
                        );
                        if let Some(profile) = profile {
                            self.chat_widget.add_error_message(format!(
                                "Failed to save model for profile `{profile}`: {err}"
                            ));
                        } else {
                            self.chat_widget
                                .add_error_message(format!("Failed to save default model: {err}"));
                        }
                    }
                }
            }
            AppEvent::ApplyApprovalPreset { approval, sandbox } => {
                #[cfg(target_os = "windows")]
                let sandbox_is_workspace_write_or_ro = matches!(
                    sandbox,
                    codex_core::protocol::SandboxPolicy::WorkspaceWrite { .. }
                        | codex_core::protocol::SandboxPolicy::ReadOnly
                );

                self.apply_approval_preset(approval, sandbox);

                // If sandbox policy becomes workspace-write or read-only, run the Windows world-writable scan.
                #[cfg(target_os = "windows")]
                {
                    // One-shot suppression if the user just confirmed continue.
                    if self.skip_world_writable_scan_once {
                        self.skip_world_writable_scan_once = false;
                        return Ok(true);
                    }

                    let should_check = codex_core::get_platform_sandbox().is_some()
                        && sandbox_is_workspace_write_or_ro
                        && !self.chat_widget.world_writable_warning_hidden();
                    if should_check {
                        let cwd = self.config.cwd.clone();
                        let env_map: std::collections::HashMap<String, String> =
                            std::env::vars().collect();
                        let tx = self.app_event_tx.clone();
                        let logs_base_dir = self.config.codex_home.clone();
                        let sandbox_policy = self.config.sandbox_policy.clone();
                        Self::spawn_world_writable_scan(
                            cwd,
                            env_map,
                            logs_base_dir,
                            sandbox_policy,
                            tx,
                        );
                    }
                }
            }
            AppEvent::SkipNextWorldWritableScan => {
                self.skip_world_writable_scan_once = true;
            }
            AppEvent::UpdateFullAccessWarningAcknowledged(ack) => {
                self.chat_widget.set_full_access_warning_acknowledged(ack);
            }
            AppEvent::UpdateWorldWritableWarningAcknowledged(ack) => {
                self.chat_widget
                    .set_world_writable_warning_acknowledged(ack);
            }
            AppEvent::PersistFullAccessWarningAcknowledged => {
                if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
                    .set_hide_full_access_warning(true)
                    .apply()
                    .await
                {
                    tracing::error!(
                        error = %err,
                        "failed to persist full access warning acknowledgement"
                    );
                    self.chat_widget.add_error_message(format!(
                        "Failed to save full access confirmation preference: {err}"
                    ));
                }
            }
            AppEvent::PersistWorldWritableWarningAcknowledged => {
                if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
                    .set_hide_world_writable_warning(true)
                    .apply()
                    .await
                {
                    tracing::error!(
                        error = %err,
                        "failed to persist world-writable warning acknowledgement"
                    );
                    self.chat_widget.add_error_message(format!(
                        "Failed to save Agent mode warning preference: {err}"
                    ));
                }
            }
            AppEvent::PersistModelMigrationPromptAcknowledged { migration_config } => {
                if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
                    .set_hide_model_migration_prompt(&migration_config, true)
                    .apply()
                    .await
                {
                    tracing::error!(error = %err, "failed to persist model migration prompt acknowledgement");
                    self.chat_widget.add_error_message(format!(
                        "Failed to save model migration prompt preference: {err}"
                    ));
                }
            }
            AppEvent::OpenApprovalsPopup => {
                self.chat_widget.open_approvals_popup();
            }
            AppEvent::FullScreenApprovalRequest(request) => match request {
                ApprovalRequest::ApplyPatch { cwd, changes, .. } => {
                    let _ = tui.enter_alt_screen();
                    let diff_summary = DiffSummary::new(changes, cwd);
                    self.overlay = Some(Overlay::new_static_with_renderables(
                        vec![diff_summary.into()],
                        "P A T C H".to_string(),
                    ));
                }
                ApprovalRequest::Exec { command, .. } => {
                    let _ = tui.enter_alt_screen();
                    let full_cmd = strip_bash_lc_and_escape(&command);
                    let full_cmd_lines = highlight_bash_to_lines(&full_cmd);
                    self.overlay = Some(Overlay::new_static_with_lines(
                        full_cmd_lines,
                        "E X E C".to_string(),
                    ));
                }
                ApprovalRequest::McpElicitation {
                    server_name,
                    message,
                    ..
                } => {
                    let _ = tui.enter_alt_screen();
                    let paragraph = Paragraph::new(vec![
                        Line::from(vec!["Server: ".into(), server_name.bold()]),
                        Line::from(""),
                        Line::from(message),
                    ])
                    .wrap(Wrap { trim: false });
                    self.overlay = Some(Overlay::new_static_with_renderables(
                        vec![Box::new(paragraph)],
                        "E L I C I T A T I O N".to_string(),
                    ));
                }
            },
            AppEvent::SetPendingAgent {
                agent_name,
                display_name,
            } => {
                // Store the pending agent selection in both App and ChatWidget
                self.pending_agent = Some(PendingAgentSelection {
                    agent_name: agent_name.clone(),
                    display_name: display_name.clone(),
                });
                // Also set on ChatWidget so it can trigger the switch on prompt submission
                self.chat_widget
                    .set_pending_agent(agent_name.clone(), display_name.clone());
                tracing::info!(
                    "Pending agent set: {} ({}). Will switch on next prompt.",
                    display_name,
                    agent_name
                );
                self.chat_widget.add_info_message(
                    format!(
                        "Agent '{display_name}' selected. On next prompt, will start a new conversation with this agent (current history will not be transferred)."
                    ),
                    None,
                );
            }
            AppEvent::SubmitWithAgentSwitch {
                agent_name,
                display_name,
                message_text,
                image_paths,
            } => {
                tracing::info!(
                    "Switching agent to {} ({}) and submitting message",
                    display_name,
                    agent_name
                );

                // Clear the pending agent since we're applying it now
                self.pending_agent = None;

                // Update the model in config
                self.config.model = agent_name.clone();

                // Persist the agent selection to config.toml for next TUI startup
                if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
                    .set_agent(Some(&agent_name))
                    .apply()
                    .await
                {
                    tracing::error!(error = %err, "failed to persist agent selection");
                    // Non-fatal: continue with the switch even if persistence fails
                }

                // Shutdown current conversation
                self.shutdown_current_conversation();

                // Create the new chat widget with the new config and the message as initial prompt
                // Set expected_agent to filter events from the OLD agent until SessionConfigured
                let init = crate::chatwidget::ChatWidgetInit {
                    config: self.config.clone(),
                    frame_requester: tui.frame_requester(),
                    app_event_tx: self.app_event_tx.clone(),
                    initial_prompt: Some(message_text),
                    initial_images: image_paths,
                    enhanced_keys_supported: self.enhanced_keys_supported,
                    auth_manager: self.auth_manager.clone(),
                    vertical_footer: self.vertical_footer,
                    expected_agent: Some(agent_name.clone()),
                    deferred_spawn: false,
                    fork_context: None,
                };
                self.chat_widget = ChatWidget::new(init);
                self.chat_widget
                    .set_hotkey_config(self.hotkey_config.clone());
                self.chat_widget.set_vim_mode(self.vim_mode);
                self.chat_widget.set_plan_drawer_mode(self.plan_drawer_mode);
                #[cfg(feature = "nori-config")]
                self.chat_widget
                    .set_loop_count_override(self.loop_count_override);

                self.chat_widget.add_info_message(
                    format!("Started new conversation with agent: {display_name}"),
                    None,
                );
            }
            AppEvent::AgentSpawnFailed { agent_name, error } => {
                tracing::warn!(
                    agent = %agent_name,
                    error = %error,
                    "Agent failed to spawn, opening agent picker"
                );

                self.chat_widget.on_agent_spawn_failed(&agent_name, &error);
            }
            AppEvent::AgentConnecting { display_name } => {
                tracing::info!(
                    display_name = %display_name,
                    "Agent connecting, showing status indicator"
                );
                self.chat_widget.show_connecting_status(&display_name);
            }
            #[cfg(feature = "unstable")]
            AppEvent::OpenAcpModelPicker {
                models,
                current_model_id,
            } => {
                self.chat_widget
                    .open_acp_model_picker(models, current_model_id);
            }
            #[cfg(feature = "unstable")]
            AppEvent::SetAcpModel {
                model_id,
                display_name,
            } => {
                self.chat_widget.set_acp_model(model_id, display_name);
            }
            #[cfg(feature = "unstable")]
            AppEvent::AcpModelSetResult {
                success,
                model_id,
                display_name,
                error,
            } => {
                if success {
                    // Update the approval dialog display name to reflect the new model
                    self.chat_widget
                        .update_agent_display_name(display_name.clone());
                    self.chat_widget
                        .add_info_message(format!("Model switched to: {display_name}"), None);

                    // Persist the model selection to [default_models] in config.toml
                    let agent = self.config.model.clone();
                    if let Err(err) = ConfigEditsBuilder::new(&self.config.codex_home)
                        .set_default_model(&agent, &model_id)
                        .apply()
                        .await
                    {
                        tracing::error!(
                            error = %err,
                            "failed to persist default model selection"
                        );
                    }
                } else {
                    let error_msg = error.unwrap_or_else(|| "Unknown error".to_string());
                    self.chat_widget
                        .add_info_message(format!("Failed to switch model: {error_msg}"), None);
                }
            }
            AppEvent::LoginComplete { success } => {
                self.chat_widget.handle_login_complete(success);
            }
            AppEvent::ExternalCliLoginOutput { data } => {
                self.chat_widget.handle_external_cli_login_output(data);
            }
            AppEvent::ExternalCliLoginComplete {
                success,
                agent_name,
            } => {
                self.chat_widget
                    .handle_external_cli_login_complete(success, agent_name);
            }
            AppEvent::SetConfigVerticalFooter(enabled) => {
                self.persist_config_setting("vertical_footer", enabled)
                    .await;
            }
            AppEvent::SetConfigTerminalNotifications(enabled) => {
                self.persist_notification_setting("terminal_notifications", enabled)
                    .await;
            }
            AppEvent::SetConfigOsNotifications(enabled) => {
                self.persist_notification_setting("os_notifications", enabled)
                    .await;
            }
            AppEvent::SetConfigHotkey { action, binding } => {
                self.persist_hotkey_setting(action, binding).await;
            }
            AppEvent::OpenHotkeyPicker => {
                self.chat_widget
                    .open_hotkey_picker(self.hotkey_config.clone());
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenNotifyAfterIdlePicker => {
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                self.chat_widget
                    .open_notify_after_idle_picker(nori_config.notify_after_idle);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigNotifyAfterIdle(value) => {
                self.persist_notify_after_idle_setting(value).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenScriptTimeoutPicker => {
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                self.chat_widget
                    .open_script_timeout_picker(nori_config.script_timeout);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigScriptTimeout(value) => {
                self.persist_script_timeout_setting(value).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenLoopCountPicker => {
                let current = match self.loop_count_override {
                    Some(overridden) => overridden,
                    None => {
                        codex_acp::config::NoriConfig::load()
                            .unwrap_or_default()
                            .loop_count
                    }
                };
                self.chat_widget.open_loop_count_picker(current);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigLoopCount(value) => {
                self.set_session_loop_count(value);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenVimModePicker => {
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                self.chat_widget.open_vim_mode_picker(nori_config.vim_mode);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenAutoWorktreePicker => {
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                self.chat_widget
                    .open_auto_worktree_picker(nori_config.auto_worktree);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigAutoWorktree(value) => {
                self.persist_auto_worktree_setting(value).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigSkillsetPerSession(enabled) => {
                self.persist_skillset_per_session_setting(enabled).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigPinnedPlanDrawer(enabled) => {
                self.persist_pinned_plan_drawer_setting(enabled).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenSkillsetPerSessionWorktreeChoice => {
                self.chat_widget.open_skillset_worktree_choice_picker();
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenFooterSegmentsPicker => {
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                self.chat_widget
                    .open_footer_segments_picker(&nori_config.footer_segment_config);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigFooterSegment(segment, enabled) => {
                self.persist_footer_segment_setting(segment, enabled).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::BrowseFiles(fm) => {
                self.browse_files(fm, tui);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::SetConfigFileManager(value) => {
                self.persist_file_manager_setting(value).await;
            }
            #[cfg(feature = "nori-config")]
            AppEvent::OpenFileManagerPicker => {
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                self.chat_widget
                    .open_file_manager_picker(nori_config.file_manager);
            }
            #[cfg(feature = "nori-config")]
            AppEvent::LoopIteration {
                prompt,
                remaining,
                total,
            } => {
                let iteration = total - remaining;
                tracing::info!("Loop iteration {iteration}/{total} (remaining: {remaining})");

                self.shutdown_current_conversation();

                let init = crate::chatwidget::ChatWidgetInit {
                    config: self.config.clone(),
                    frame_requester: tui.frame_requester(),
                    app_event_tx: self.app_event_tx.clone(),
                    initial_prompt: Some(prompt),
                    initial_images: Vec::new(),
                    enhanced_keys_supported: self.enhanced_keys_supported,
                    auth_manager: self.auth_manager.clone(),
                    vertical_footer: self.vertical_footer,
                    expected_agent: None,
                    deferred_spawn: false,
                    fork_context: None,
                };
                self.chat_widget = ChatWidget::new(init);
                self.chat_widget
                    .set_hotkey_config(self.hotkey_config.clone());
                self.chat_widget.set_vim_mode(self.vim_mode);
                self.chat_widget.set_plan_drawer_mode(self.plan_drawer_mode);
                self.chat_widget
                    .set_loop_count_override(self.loop_count_override);
                self.chat_widget.set_loop_state(remaining, total);

                self.chat_widget
                    .add_info_message(format!("Loop iteration {iteration} of {total}"), None);
            }
            AppEvent::SetConfigVimMode(value) => {
                self.persist_vim_mode_setting(value).await;
            }
            AppEvent::SkillsetListResult {
                names,
                error,
                install_dir,
            } => {
                self.chat_widget
                    .on_skillset_list_result(names, error, install_dir);
            }
            AppEvent::InstallSkillset { name } => {
                self.chat_widget.on_install_skillset_request(&name);
            }
            AppEvent::SwitchSkillset { name, install_dir } => {
                self.chat_widget
                    .on_switch_skillset_request(&name, &install_dir);
            }
            AppEvent::SkillsetInstallResult {
                name,
                success,
                message,
            } => {
                self.chat_widget
                    .on_skillset_install_result(&name, success, &message);
                if success {
                    self.request_system_info_refresh(
                        self.config.cwd.clone(),
                        self.config.model.clone().into(),
                        self.chat_widget.first_prompt_text(),
                    );
                }
            }
            AppEvent::SkillsetSwitchResult {
                name,
                success,
                message,
            } => {
                self.chat_widget
                    .on_skillset_switch_result(&name, success, &message);
                // If the agent spawn was deferred (waiting for skillset switch to
                // complete), trigger it now that files are on disk.
                #[cfg(feature = "nori-config")]
                if success && self.deferred_spawn_pending {
                    self.deferred_spawn_pending = false;
                    self.chat_widget
                        .spawn_deferred_agent(self.config.clone(), self.app_event_tx.clone());
                }
                if success {
                    self.request_system_info_refresh(
                        self.config.cwd.clone(),
                        self.config.model.clone().into(),
                        self.chat_widget.first_prompt_text(),
                    );
                }
            }
            AppEvent::SkillsetPickerDismissed => {
                // The skillset picker was dismissed without selection. If the
                // agent spawn was deferred, spawn it now without a skillset
                // (behaves as if skillset_per_session is disabled).
                #[cfg(feature = "nori-config")]
                if self.deferred_spawn_pending {
                    self.deferred_spawn_pending = false;
                    self.chat_widget
                        .spawn_deferred_agent(self.config.clone(), self.app_event_tx.clone());
                }
            }
            AppEvent::ExecuteScript { prompt, args } => {
                let tx = self.app_event_tx.clone();
                let nori_config = codex_acp::config::NoriConfig::load().unwrap_or_default();
                let timeout = nori_config.script_timeout.as_duration();
                let name = prompt.name.clone();
                self.chat_widget
                    .add_info_message(format!("Running script '{name}'..."), None);
                tokio::spawn(async move {
                    let result =
                        codex_core::custom_prompts::execute_script(&prompt, &args, timeout).await;
                    tx.send(AppEvent::ScriptExecutionComplete {
                        name: prompt.name.clone(),
                        result,
                    });
                });
            }
            AppEvent::ScriptExecutionComplete { name, result } => match result {
                Ok(stdout) => {
                    if stdout.trim().is_empty() {
                        self.chat_widget.add_info_message(
                            format!("Script '{name}' completed with no output."),
                            None,
                        );
                    } else {
                        let message = format!("Output from script '{name}':\n{stdout}");
                        self.chat_widget.queue_text_as_user_message(message);
                    }
                }
                Err(err) => {
                    self.chat_widget
                        .add_error_message(format!("Script '{name}' failed: {err}"));
                    let error_context =
                        format!("Script '{name}' failed with the following error:\n{err}");
                    self.chat_widget.queue_text_as_user_message(error_context);
                }
            },
            AppEvent::ShowViewonlySessionPicker {
                sessions,
                nori_home,
            } => {
                let params = crate::nori::viewonly_session_picker::viewonly_session_picker_params(
                    sessions,
                    nori_home,
                    self.app_event_tx.clone(),
                );
                self.chat_widget.show_selection_view(params);
            }
            AppEvent::LoadViewonlyTranscript {
                nori_home,
                project_id,
                session_id,
            } => {
                let tx = self.app_event_tx.clone();
                tokio::spawn(async move {
                    let loader = codex_acp::transcript::TranscriptLoader::new(nori_home);
                    match loader.load_transcript(&project_id, &session_id).await {
                        Ok(transcript) => {
                            let entries =
                                crate::viewonly_transcript::transcript_to_entries(&transcript);
                            tx.send(AppEvent::DisplayViewonlyTranscript { entries });
                        }
                        Err(e) => {
                            tx.send(AppEvent::InsertHistoryCell(Box::new(
                                crate::history_cell::new_error_event(format!(
                                    "Failed to load transcript: {e}"
                                )),
                            )));
                        }
                    }
                });
            }
            AppEvent::DisplayViewonlyTranscript { entries } => {
                self.display_viewonly_transcript(entries);
            }
            AppEvent::ShowResumeSessionPicker {
                sessions,
                nori_home,
            } => {
                let params = crate::nori::resume_session_picker::resume_session_picker_params(
                    sessions,
                    nori_home,
                    self.app_event_tx.clone(),
                );
                self.chat_widget.show_selection_view(params);
            }
            AppEvent::ResumeSession {
                nori_home,
                project_id,
                session_id,
            } => {
                let loader = codex_acp::transcript::TranscriptLoader::new(nori_home);
                match loader.load_transcript(&project_id, &session_id).await {
                    Ok(transcript) => {
                        let acp_session_id = transcript.meta.acp_session_id.clone();
                        let display_name =
                            crate::nori::agent_picker::get_agent_info(&self.config.model)
                                .map(|info| info.display_name)
                                .unwrap_or_else(|| self.config.model.clone());

                        self.shutdown_current_conversation();

                        let init = crate::chatwidget::ChatWidgetInit {
                            config: self.config.clone(),
                            frame_requester: tui.frame_requester(),
                            app_event_tx: self.app_event_tx.clone(),
                            initial_prompt: None,
                            initial_images: Vec::new(),
                            enhanced_keys_supported: self.enhanced_keys_supported,
                            auth_manager: self.auth_manager.clone(),
                            vertical_footer: self.vertical_footer,
                            expected_agent: None,
                            deferred_spawn: false,
                            fork_context: None,
                        };
                        self.chat_widget =
                            ChatWidget::new_resumed_acp(init, acp_session_id, transcript);
                        self.chat_widget
                            .set_hotkey_config(self.hotkey_config.clone());
                        self.chat_widget.set_vim_mode(self.vim_mode);
                        self.chat_widget.set_plan_drawer_mode(self.plan_drawer_mode);

                        self.chat_widget.add_info_message(
                            format!("Resuming session with {display_name}..."),
                            None,
                        );
                        tui.frame_requester().schedule_frame();
                    }
                    Err(e) => {
                        self.chat_widget
                            .add_error_message(format!("Failed to load session transcript: {e}"));
                    }
                }
            }
            AppEvent::OpenForkPicker => {
                let messages =
                    crate::app_backtrack::collect_all_user_messages(&self.transcript_cells);
                if messages.is_empty() {
                    self.chat_widget
                        .add_info_message("No messages to fork from.".to_string(), None);
                } else {
                    let params = crate::nori::fork_picker::fork_picker_params(
                        messages,
                        self.app_event_tx.clone(),
                    );
                    self.chat_widget.show_selection_view(params);
                }
                tui.frame_requester().schedule_frame();
            }
            AppEvent::ForkToMessage {
                cell_index,
                prefill,
            } => {
                let summary =
                    crate::app_backtrack::build_fork_summary(&self.transcript_cells, cell_index);
                let fork_context = if summary.is_empty() {
                    None
                } else {
                    Some(summary)
                };

                self.shutdown_current_conversation();
                let init = crate::chatwidget::ChatWidgetInit {
                    config: self.config.clone(),
                    frame_requester: tui.frame_requester(),
                    app_event_tx: self.app_event_tx.clone(),
                    initial_prompt: None,
                    initial_images: Vec::new(),
                    enhanced_keys_supported: self.enhanced_keys_supported,
                    auth_manager: self.auth_manager.clone(),
                    vertical_footer: self.vertical_footer,
                    expected_agent: None,
                    deferred_spawn: false,
                    fork_context,
                };
                self.chat_widget = ChatWidget::new(init);
                self.chat_widget
                    .set_hotkey_config(self.hotkey_config.clone());
                self.chat_widget.set_vim_mode(self.vim_mode);
                self.chat_widget.set_plan_drawer_mode(self.plan_drawer_mode);
                #[cfg(feature = "nori-config")]
                self.chat_widget
                    .set_loop_count_override(self.loop_count_override);

                // Trim transcript to preserve history before the fork point
                self.transcript_cells
                    .truncate(cell_index.min(self.transcript_cells.len()));
                self.render_transcript_once(tui);

                if !prefill.is_empty() {
                    self.chat_widget.set_composer_text(prefill);
                }
                tui.frame_requester().schedule_frame();
            }
            AppEvent::SaveMcpServers(servers) => {
                self.persist_mcp_servers(servers).await;
            }
        }
        Ok(true)
    }

    pub(super) async fn handle_key_event(&mut self, tui: &mut tui::Tui, key_event: KeyEvent) {
        use crate::nori::hotkey_match::matches_binding;
        use codex_acp::config::HotkeyAction;

        // Check configurable hotkeys first (before the structural match),
        // but only when no popup/view is active — otherwise the popup should
        // capture the key (e.g. the hotkey picker in rebinding mode).
        if key_event.kind == KeyEventKind::Press && !self.chat_widget.has_active_popup() {
            let transcript_binding = self.hotkey_config.binding_for(HotkeyAction::OpenTranscript);
            if matches_binding(transcript_binding, &key_event) {
                let _ = tui.enter_alt_screen();
                self.overlay = Some(Overlay::new_transcript(self.transcript_cells.clone()));
                tui.frame_requester().schedule_frame();
                return;
            }

            let editor_binding = self.hotkey_config.binding_for(HotkeyAction::OpenEditor);
            if matches_binding(editor_binding, &key_event) {
                self.open_external_editor(tui);
                return;
            }

            let plan_binding = self
                .hotkey_config
                .binding_for(HotkeyAction::TogglePlanDrawer);
            if matches_binding(plan_binding, &key_event) {
                self.chat_widget.toggle_plan_drawer();
                self.plan_drawer_mode = self.chat_widget.plan_drawer_mode();
                tui.frame_requester().schedule_frame();
                return;
            }
        }

        match key_event {
            // Esc primes/advances backtracking only in normal (not working) mode
            // with the composer focused and empty. In any other state, forward
            // Esc so the active UI (e.g. status indicator, modals, popups)
            // handles it.
            KeyEvent {
                code: KeyCode::Esc,
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            } => {
                if self.chat_widget.is_normal_backtrack_mode()
                    && self.chat_widget.composer_is_empty()
                {
                    self.handle_backtrack_esc_key(tui);
                } else {
                    self.chat_widget.handle_key_event(key_event);
                }
            }
            // Enter confirms backtrack when primed + count > 0. Otherwise pass to widget.
            KeyEvent {
                code: KeyCode::Enter,
                kind: KeyEventKind::Press,
                ..
            } if self.backtrack.primed
                && self.backtrack.nth_user_message != usize::MAX
                && self.chat_widget.composer_is_empty() =>
            {
                // Delegate to helper for clarity; preserves behavior.
                self.confirm_backtrack_from_main();
            }
            KeyEvent {
                kind: KeyEventKind::Press | KeyEventKind::Repeat,
                ..
            } => {
                // Any non-Esc key press should cancel a primed backtrack.
                // This avoids stale "Esc-primed" state after the user starts typing
                // (even if they later backspace to empty).
                if key_event.code != KeyCode::Esc && self.backtrack.primed {
                    self.reset_backtrack_state();
                }
                self.chat_widget.handle_key_event(key_event);
            }
            _ => {
                // Ignore Release key events.
            }
        };
    }
}
