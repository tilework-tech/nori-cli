use super::*;

impl ChatWidget {
    pub(crate) fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event {
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) && c.eq_ignore_ascii_case(&'c') => {
                self.on_ctrl_c();
                return;
            }
            KeyEvent {
                code: KeyCode::Char(c),
                modifiers,
                kind: KeyEventKind::Press,
                ..
            } if modifiers.contains(KeyModifiers::CONTROL) && c.eq_ignore_ascii_case(&'v') => {
                match paste_image_to_temp_png() {
                    Ok((path, info)) => {
                        self.attach_image(
                            path,
                            info.width,
                            info.height,
                            info.encoded_format.label(),
                        );
                    }
                    Err(err) => {
                        tracing::warn!("failed to paste image: {err}");
                        self.add_to_history(history_cell::new_error_event(format!(
                            "Failed to paste image: {err}",
                        )));
                    }
                }
                return;
            }
            KeyEvent {
                code: KeyCode::BackTab,
                kind: KeyEventKind::Press,
                ..
            } if !self.exiting
                && !self.bottom_pane.has_active_overlay_or_popup()
                && self.cycle_acp_mode_config() =>
            {
                return;
            }
            other if other.kind == KeyEventKind::Press => {
                self.bottom_pane.clear_ctrl_c_quit_hint();
            }
            _ => {}
        }

        match self.bottom_pane.handle_key_event(key_event) {
            InputResult::Submitted(text) => {
                let user_message = UserMessage {
                    text,
                    image_paths: self.bottom_pane.take_recent_submission_images(),
                };
                self.submit_user_message(user_message);
            }
            InputResult::Command(cmd) => {
                self.dispatch_command(cmd);
            }
            InputResult::None => {}
        }
    }

    pub(crate) fn attach_image(
        &mut self,
        path: PathBuf,
        width: u32,
        height: u32,
        format_label: &str,
    ) {
        tracing::info!(
            "attach_image path={path:?} width={width} height={height} format={format_label}",
        );
        self.bottom_pane
            .attach_image(path, width, height, format_label);
        self.request_redraw();
    }

    pub(super) fn dispatch_command(&mut self, cmd: SlashCommand) {
        // Once exit begins, the session is over: ignore repeated quit
        // commands and refuse everything else instead of racing teardown.
        if self.exiting {
            if !matches!(cmd, SlashCommand::Quit | SlashCommand::Exit) {
                let message = format!("'/{}' is unavailable while exiting.", cmd.command());
                self.add_to_history(history_cell::new_error_event(message));
                self.request_redraw();
            }
            return;
        }
        if !cmd.available_during_task() && self.bottom_pane.is_task_running() {
            let message = format!(
                "'/{}' is disabled while a task is in progress.",
                cmd.command()
            );
            self.add_to_history(history_cell::new_error_event(message));
            self.request_redraw();
            return;
        }
        // While a /close awaits the agent, block session-switching commands:
        // the deferred NewSession would otherwise clobber whatever session the
        // user switched to in the meantime.
        if self.session_close_in_flight
            && matches!(
                cmd,
                SlashCommand::New
                    | SlashCommand::Resume
                    | SlashCommand::ResumeViewonly
                    | SlashCommand::Agent
                    | SlashCommand::Close
            )
        {
            let message = format!("'/{}' is disabled while the session closes.", cmd.command());
            self.add_to_history(history_cell::new_error_event(message));
            self.request_redraw();
            return;
        }
        // Unified availability gate: server-advertised verdicts and
        // client-side cloud/local scoping share one mechanism. /quit and
        // /exit are exempt — they must never be disabled.
        if !matches!(cmd, SlashCommand::Quit | SlashCommand::Exit)
            && !self.ensure_builtin_command_enabled(cmd)
        {
            return;
        }
        match cmd {
            SlashCommand::New => {
                self.app_event_tx.send(AppEvent::NewSession);
            }
            SlashCommand::Resume => {
                self.open_resume_session_picker();
            }
            SlashCommand::ResumeViewonly => {
                self.open_viewonly_session_picker();
            }
            SlashCommand::Close => {
                let Some(handle) = self.acp_handle.clone() else {
                    self.add_error_message("No live agent connection to close.".to_string());
                    return;
                };
                self.session_close_in_flight = true;
                let tx = self.app_event_tx.clone();
                tokio::spawn(async move {
                    match handle.close_session().await {
                        Ok(()) => {
                            tx.send(AppEvent::InsertHistoryCell(Box::new(
                                history_cell::new_info_event(
                                    "Session closed (released).".to_string(),
                                    None,
                                ),
                            )));
                            // Return to the session picker — never auto-claim
                            // a fresh session (on a cloud agent that would
                            // silently boot a new VM).
                            tx.send(AppEvent::SessionClosed);
                        }
                        Err(e) => {
                            tx.send(AppEvent::SessionCloseFailed {
                                message: format!("{e}"),
                            });
                        }
                    }
                });
            }
            SlashCommand::Init => {
                let init_target = self.config.cwd.join(DEFAULT_PROJECT_DOC_FILENAME);
                if init_target.exists() {
                    let message = format!(
                        "{DEFAULT_PROJECT_DOC_FILENAME} already exists here. Skipping /init to avoid overwriting it."
                    );
                    self.add_info_message(message, None);
                    return;
                }
                const INIT_PROMPT: &str = include_str!("../../prompt_for_init_command.md");
                self.submit_user_message(INIT_PROMPT.to_string().into());
            }
            SlashCommand::Compact => {
                self.clear_token_usage();
                self.app_event_tx.send(AppEvent::CodexOp(Op::Compact));
            }
            SlashCommand::Agent => {
                self.open_agent_popup();
            }
            SlashCommand::Model => {
                self.open_model_popup();
            }
            SlashCommand::Config => {
                self.open_session_config_popup();
            }
            SlashCommand::Approvals => {
                self.open_approvals_popup();
            }
            SlashCommand::Settings => {
                // Apply ephemeral session overrides so the picker shows the
                // current in-session value rather than the persisted one.
                let mut config = self.config.clone();
                if let Some(overridden) = self.loop_count_override {
                    config.loop_count = overridden;
                }
                self.open_settings_popup(&config);
            }
            SlashCommand::Vim => self.open_vim_mode_picker(self.config.vim_mode),
            SlashCommand::Goal => {
                self.request_thread_goal_status();
            }
            SlashCommand::Quit | SlashCommand::Exit => {
                self.begin_exit();
            }
            SlashCommand::Login => {
                self.handle_login_command();
            }
            SlashCommand::Logout => {
                self.add_info_message(
                    "To logout, run the agent's logout command directly (e.g., `claude /logout`)"
                        .to_string(),
                    None,
                );
            }
            SlashCommand::Undo => {
                self.app_event_tx.send(AppEvent::CodexOp(Op::UndoList));
            }
            SlashCommand::Browse => match self.config.file_manager {
                Some(fm) => {
                    self.app_event_tx.send(AppEvent::BrowseFiles(fm));
                }
                None => {
                    self.add_error_message(
                        "No file manager configured. Use /settings to set one.".to_string(),
                    );
                }
            },
            SlashCommand::Diff => {
                self.add_diff_in_progress();
                let tx = self.app_event_tx.clone();
                let dir = self
                    .effective_cwd_tracker
                    .effective_cwd()
                    .cloned()
                    .unwrap_or_else(|| self.config.cwd.clone());
                tokio::spawn(async move {
                    let text = match get_git_diff(Some(&dir)).await {
                        Ok((is_git_repo, diff_text)) => {
                            if is_git_repo {
                                diff_text
                            } else {
                                "`/diff` — _not inside a git repository_".to_string()
                            }
                        }
                        Err(e) => format!("Failed to compute diff: {e}"),
                    };
                    tx.send(AppEvent::DiffResult(text));
                });
            }
            SlashCommand::Mention => {
                self.insert_str("@");
            }
            SlashCommand::Status => {
                self.add_status_output();
            }
            SlashCommand::Memory => {
                self.add_memory_output();
            }
            SlashCommand::FirstPrompt => {
                if let Some(text) = &self.first_prompt_text {
                    self.add_info_message(text.clone(), None);
                } else {
                    self.add_info_message("No prompt has been submitted yet.".to_string(), None);
                }
            }
            SlashCommand::Mcp => {
                self.open_mcp_servers_popup();
            }
            SlashCommand::SwitchSkillset => {
                self.handle_switch_skillset_command();
            }
            SlashCommand::Fork => {
                self.app_event_tx.send(AppEvent::OpenForkPicker);
            }
            #[cfg(unix)]
            SlashCommand::Browser => {
                if let Some((ws_url, cdp_port)) =
                    nori_harness::backend::browser_session::active_session_info()
                {
                    self.add_info_message(
                        format!("Browser already running on CDP port {cdp_port} ({ws_url})"),
                        None,
                    );
                    return;
                }
                self.add_info_message("Launching browser...".to_string(), None);
                let tx = self.app_event_tx.clone();
                tokio::spawn(async move {
                    match nori_harness::backend::browser_session::BrowserSession::launch_and_store()
                        .await
                    {
                        Ok((ws_url, cdp_port)) => {
                            tx.send(AppEvent::BrowserLaunched { ws_url, cdp_port });
                        }
                        Err(err) => {
                            tx.send(AppEvent::BrowserLaunchFailed(format!("{err:#}")));
                        }
                    }
                });
            }
            #[cfg(not(unix))]
            SlashCommand::Browser => {
                self.add_info_message(
                    "/browser is only available on Unix systems".to_string(),
                    None,
                );
            }
        }
    }

    pub(crate) fn handle_paste(&mut self, text: String) {
        self.bottom_pane.handle_paste(text);
    }

    // Returns true if caller should skip rendering this frame (a future frame is scheduled).
    pub(crate) fn handle_paste_burst_tick(&mut self, frame_requester: FrameRequester) -> bool {
        if self.bottom_pane.flush_paste_burst_if_due() {
            // A paste just flushed; request an immediate redraw and skip this frame.
            self.request_redraw();
            true
        } else if self.bottom_pane.is_in_paste_burst() {
            // While capturing a burst, schedule a follow-up tick and skip this frame
            // to avoid redundant renders between ticks.
            frame_requester.schedule_frame_in(
                crate::bottom_pane::ChatComposer::recommended_paste_flush_delay(),
            );
            true
        } else {
            false
        }
    }
}
