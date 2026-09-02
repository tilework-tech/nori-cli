use super::*;

impl App {
    /// Clear and report a due deferred agent spawn.
    ///
    /// Per-session startup defers the agent spawn until the chosen skillset's
    /// files are on disk. Returns `true` (clearing the pending flag) when a
    /// spawn is now due; `false` while agent preparation is still in
    /// flight or no spawn was deferred. Shared by the skillset-applied and
    /// picker-dismissed paths so their guard cannot drift.
    pub(super) fn take_deferred_spawn(&mut self) -> bool {
        if self.deferred_spawn_pending && self.primary_agent_preparation.is_none() {
            self.deferred_spawn_pending = false;
            true
        } else {
            false
        }
    }

    /// Resolve a deferred agent spawn and refresh system info after a skillset
    /// install or switch completes. Both `SkillsetInstallResult` and
    /// `SkillsetSwitchResult` route here: a per-session startup that lands on a
    /// non-worktree cwd installs (rather than switches), so the deferred spawn
    /// must resolve on either result or the agent never starts.
    fn on_skillset_applied(&mut self, success: bool) {
        if !success {
            return;
        }
        if self.take_deferred_spawn() {
            self.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Idle);
        }
        self.request_system_info_refresh(
            self.config.cwd.clone(),
            self.config.active_agent.clone().into(),
            self.chat_widget.first_prompt_text(),
        );
    }

    pub(crate) async fn handle_tui_event(
        &mut self,
        tui: &mut tui::Tui,
        event: TuiEvent,
    ) -> Result<bool> {
        if matches!(event, TuiEvent::Draw) {
            self.handle_resize_reflow_draw(tui)?;
        }
        if self.overlay.is_some() {
            let _ = self.handle_backtrack_overlay_event(tui, event).await?;
        } else {
            match event {
                TuiEvent::Key(key_event) => {
                    self.handle_key_event(tui, key_event).await;
                }
                TuiEvent::Paste(pasted) => {
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
        if !matches!(sandbox, nori_config::SandboxPolicy::ReadOnly)
            || codex_sandbox::get_platform_sandbox().is_some()
        {
            self.config.forced_auto_mode_downgraded_on_windows = false;
        }
        self.chat_widget.set_approval_policy(approval);
        self.chat_widget.set_sandbox_policy(sandbox);
    }

    pub(super) async fn handle_event(
        &mut self,
        tui: &mut tui::Tui,
        event: AppEvent,
    ) -> Result<bool> {
        match event {
            AppEvent::NewSession => {
                if matches!(self.candidate_agent, Some(CandidateAgent::Prepared { .. })) {
                    let Some(CandidateAgent::Prepared {
                        agent_name,
                        display_name,
                        mut agent,
                        ..
                    }) = self.candidate_agent.take()
                    else {
                        unreachable!("candidate variant checked above")
                    };
                    let config = self.config_for_agent(&agent_name);
                    if let Err(error) = nori_harness::runtime::refresh_prepared_agent(
                        &mut agent,
                        crate::chatwidget::agent::agent_prepare_spec(config.clone(), None),
                    ) {
                        tokio::spawn((*agent).shutdown());
                        self.chat_widget.set_login_agent_override(Some(agent_name));
                        self.chat_widget.add_error_message(format!(
                            "Couldn't activate {display_name}: {error}"
                        ));
                        return Ok(true);
                    }
                    let mut init = self.chat_widget_init(
                        tui.frame_requester(),
                        None,
                        Vec::new(),
                        Some(config.active_agent.clone()),
                        false,
                        None,
                    );
                    init.config = config;
                    init.prepared_agent = Some(*agent);
                    let widget = ChatWidget::new_candidate(init);
                    self.candidate_agent = Some(CandidateAgent::Activating {
                        agent_name,
                        display_name,
                        widget: Box::new(widget),
                    });
                    tui.frame_requester().schedule_frame();
                    return Ok(true);
                }

                self.discard_candidate();
                if let Some(preparation) = &mut self.primary_agent_preparation {
                    preparation.intent = crate::app_event::AgentPrepareIntent::Idle;
                    self.pending_session_activation = Some(PendingSessionActivation::New);
                    return Ok(true);
                }
                if self.prepared_agent.is_some() {
                    self.pending_session_activation = Some(PendingSessionActivation::New);
                    let agent = match self.take_refreshed_prepared_agent() {
                        Ok(Some(agent)) => agent,
                        Ok(None) => return Ok(true),
                        Err(error) => {
                            self.chat_widget.add_error_message(format!(
                                "Couldn't activate prepared agent: {error}"
                            ));
                            return Ok(true);
                        }
                    };
                    self.pending_session_activation = None;
                    self.deferred_spawn_pending = false;
                    let (initial_prompt, initial_images) = self.chat_widget.take_initial_input();
                    let composer_text = self.chat_widget.composer_text();
                    let loop_state = self.chat_widget.loop_state();
                    self.shutdown_current_conversation();
                    let mut init = self.chat_widget_init(
                        tui.frame_requester(),
                        initial_prompt,
                        initial_images,
                        None,
                        false,
                        None,
                    );
                    init.prepared_agent = Some(agent);
                    self.chat_widget = ChatWidget::new(init);
                    self.configure_new_chat_widget();
                    if !composer_text.is_empty() {
                        self.chat_widget.set_composer_text(composer_text);
                    }
                    if let Some((remaining, total)) = loop_state {
                        self.chat_widget.set_loop_state(remaining, total);
                    }
                    tui.frame_requester().schedule_frame();
                    return Ok(true);
                }

                if !self.chat_widget.has_harness_session() {
                    self.pending_session_activation = Some(PendingSessionActivation::New);
                    self.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Idle);
                    return Ok(true);
                }

                self.pending_session_activation = Some(PendingSessionActivation::New);
                self.deferred_spawn_pending = false;
                let summary = session_summary(
                    self.chat_widget.token_usage(),
                    self.chat_widget.conversation_id(),
                    self.chat_widget.session_stats().has_activity(),
                );
                let (initial_prompt, initial_images) = self.chat_widget.take_initial_input();
                self.shutdown_current_conversation();
                let init = self.chat_widget_init(
                    tui.frame_requester(),
                    initial_prompt,
                    initial_images,
                    None,
                    true,
                    None,
                );
                self.chat_widget = ChatWidget::new(init);
                self.configure_new_chat_widget();
                self.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Idle);
                if let Some(summary) = summary {
                    let mut lines: Vec<Line<'static>> = Vec::new();
                    if let Some(usage_line) = summary.usage_line {
                        lines.push(usage_line.into());
                    }
                    if let Some(command) = summary.resume_command {
                        lines.push(RESUME_HINT_LEAD.into());
                        lines.push(command.cyan().into());
                    }
                    self.chat_widget.add_plain_history_lines(lines);
                }
                tui.frame_requester().schedule_frame();
            }
            AppEvent::SessionCloseFailed { message } => {
                self.chat_widget.on_session_close_failed(message);
            }
            AppEvent::SessionClosed => {
                // The session was released; land back on the session picker
                // with a fresh deferred widget — never auto-claim a new
                // session (on a cloud agent that would boot a new VM).
                self.cancel_primary_agent_preparation();
                self.discard_candidate();
                self.pending_session_activation = None;
                if let Some(agent) = self.prepared_agent.take() {
                    tokio::spawn(agent.shutdown());
                }
                self.prepared_agent_initial_context = None;
                let init = self.chat_widget_init(
                    tui.frame_requester(),
                    None,
                    Vec::new(),
                    None,
                    true,
                    None,
                );
                self.chat_widget = ChatWidget::new(init);
                self.configure_new_chat_widget();
                self.deferred_spawn_pending = true;
                self.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Picker {
                    fallback_to_spawn: false,
                });
                tui.frame_requester().schedule_frame();
            }
            AppEvent::RemoteControlRequested(request) => {
                let tailnet = match request {
                    crate::remote_control::RemoteControlRequest::EnableLocal
                    | crate::remote_control::RemoteControlRequest::EnableTailnet
                    | crate::remote_control::RemoteControlRequest::Status => {
                        crate::remote_control::detect_tailnet_ipv4().await
                    }
                    crate::remote_control::RemoteControlRequest::EnableExplicit(_)
                    | crate::remote_control::RemoteControlRequest::Disable => {
                        Err("Tailscale was not queried.".to_string())
                    }
                };
                let outcome = self
                    .remote_control
                    .execute_request_with_detection(request, tailnet)
                    .await;
                match outcome {
                    crate::remote_control::RemoteControlOutcome::Report(report) => {
                        self.chat_widget.add_boxed_history(Box::new(
                            crate::history_cell::new_remote_control_event(report.lines()),
                        ));
                    }
                    crate::remote_control::RemoteControlOutcome::ConfirmationRequired(addr) => {
                        self.chat_widget.open_remote_control_confirmation(addr);
                    }
                    crate::remote_control::RemoteControlOutcome::Error(message) => {
                        self.chat_widget.add_error_message(message);
                    }
                }
            }
            AppEvent::ConfirmRemoteControlExplicit(addr) => {
                let outcome = self.remote_control.confirm_explicit(addr).await;
                match outcome {
                    crate::remote_control::RemoteControlOutcome::Report(report) => {
                        self.chat_widget.add_boxed_history(Box::new(
                            crate::history_cell::new_remote_control_event(report.lines()),
                        ));
                    }
                    crate::remote_control::RemoteControlOutcome::Error(message) => {
                        self.chat_widget.add_error_message(message);
                    }
                    crate::remote_control::RemoteControlOutcome::ConfirmationRequired(_) => {
                        unreachable!("a confirmed address cannot request confirmation again")
                    }
                }
            }
            AppEvent::AgentPrepared {
                generation,
                agent,
                intent,
            } => {
                if let crate::app_event::AgentPrepareIntent::Candidate {
                    agent_name,
                    display_name,
                } = intent
                {
                    let is_current = matches!(
                        &self.candidate_agent,
                        Some(CandidateAgent::Preparing {
                            generation: current,
                            agent_name: current_agent,
                            ..
                        }) if *current == generation && *current_agent == agent_name
                    );
                    if !is_current {
                        if let Ok(agent) = agent {
                            tokio::spawn(agent.shutdown());
                        }
                        return Ok(true);
                    }

                    match agent {
                        Ok(agent) => {
                            let automatic_resume = super::session_setup::automatic_resume_event(
                                agent.automatic_session_id(),
                            );
                            let sessions = match agent.catalog() {
                                nori_harness::runtime::SessionCatalog::Unsupported => Vec::new(),
                                nori_harness::runtime::SessionCatalog::Listed(sessions) => {
                                    sessions.clone()
                                }
                            };
                            let Some(CandidateAgent::Preparing { agent_name, .. }) =
                                self.candidate_agent.take()
                            else {
                                unreachable!("candidate generation checked above")
                            };
                            self.candidate_agent = Some(CandidateAgent::Prepared {
                                agent_name,
                                display_name,
                                agent: Box::new(agent),
                            });
                            if let Some(event) = automatic_resume {
                                self.app_event_tx.send(event);
                            } else {
                                self.chat_widget
                                    .show_acp_resume_session_picker(sessions, true);
                            }
                        }
                        Err(error) => {
                            self.candidate_agent = None;
                            self.chat_widget.set_login_agent_override(Some(agent_name));
                            self.chat_widget.add_error_message(format!(
                                "Couldn't prepare {display_name}: {error}"
                            ));
                        }
                    }
                    tui.frame_requester().schedule_frame();
                    return Ok(true);
                }

                let is_current = matches!(
                    &self.primary_agent_preparation,
                    Some(preparation) if preparation.generation == generation
                );
                if !is_current {
                    if let Ok(agent) = agent {
                        tokio::spawn(agent.shutdown());
                    }
                    return Ok(true);
                }
                let Some(primary_preparation) = self.primary_agent_preparation.as_ref() else {
                    return Ok(true);
                };
                let prepare_intent = primary_preparation.intent.clone();
                let prepared_initial_context = primary_preparation.initial_context.clone();
                self.primary_agent_preparation = None;

                match agent {
                    Ok(agent) => {
                        let automatic_resume = super::session_setup::automatic_resume_event(
                            agent.automatic_session_id(),
                        );
                        // Seed the deferred widget with the prepared agent
                        // capabilities so capability-gated behavior (e.g. the
                        // detach wording on quit) is right before any session
                        // exists.
                        self.chat_widget.handle_client_event(
                            crate::presentation::ClientEvent::SessionCapabilitiesChanged(
                                crate::presentation::SessionCapabilitiesView {
                                    agent: crate::presentation::AgentCapabilitiesView {
                                        http_mcp: agent.capabilities().mcp_capabilities.http,
                                        load_session: agent.capabilities().load_session,
                                        session_list: agent
                                            .capabilities()
                                            .session_capabilities
                                            .list
                                            .is_some(),
                                        session_resume: agent
                                            .capabilities()
                                            .session_capabilities
                                            .resume
                                            .is_some(),
                                        session_close: agent
                                            .capabilities()
                                            .session_capabilities
                                            .close
                                            .is_some(),
                                        session_fork: agent
                                            .capabilities()
                                            .session_capabilities
                                            .fork
                                            .is_some(),
                                    },
                                    ..Default::default()
                                },
                            ),
                        );
                        let can_resume_catalog = agent.capabilities().load_session
                            || agent.capabilities().session_capabilities.resume.is_some();
                        let sessions = match agent.catalog() {
                            nori_harness::runtime::SessionCatalog::Unsupported => None,
                            nori_harness::runtime::SessionCatalog::Listed(sessions) => {
                                Some(sessions.clone())
                            }
                        };
                        self.prepared_agent = Some(agent);
                        self.prepared_agent_initial_context = prepared_initial_context;
                        if let Some(event) = automatic_resume {
                            self.pending_session_activation = None;
                            self.app_event_tx.send(event);
                        } else {
                            if let Some(activation) = self.pending_session_activation.take() {
                                match activation {
                                    PendingSessionActivation::New => {
                                        self.app_event_tx.send(AppEvent::NewSession)
                                    }
                                    PendingSessionActivation::Resume {
                                        acp_session_id,
                                        title,
                                        transcript,
                                    } => self.app_event_tx.send(AppEvent::ActivatePreparedResume {
                                        acp_session_id,
                                        title,
                                        transcript: transcript.map(|transcript| *transcript),
                                    }),
                                }
                                tui.frame_requester().schedule_frame();
                                return Ok(true);
                            }
                            match prepare_intent {
                                crate::app_event::AgentPrepareIntent::Idle => {}
                                crate::app_event::AgentPrepareIntent::Onboarding => {
                                    if let Some(event) = sessions
                                        .and_then(super::session_setup::onboarding_resume_event)
                                    {
                                        self.app_event_tx.send(event);
                                    } else {
                                        self.app_event_tx.send(AppEvent::NewSession);
                                    }
                                }
                                crate::app_event::AgentPrepareIntent::Picker {
                                    fallback_to_spawn,
                                } => match sessions {
                                    Some(sessions) => self
                                        .chat_widget
                                        .show_acp_resume_session_picker(sessions, false),
                                    None if fallback_to_spawn => {
                                        self.app_event_tx.send(AppEvent::NewSession)
                                    }
                                    None => self
                                        .chat_widget
                                        .show_acp_resume_session_picker(Vec::new(), false),
                                },
                                crate::app_event::AgentPrepareIntent::ResumePicker => {
                                    if can_resume_catalog && let Some(sessions) = sessions {
                                        self.chat_widget
                                            .show_acp_resume_session_picker(sessions, false);
                                    } else {
                                        self.chat_widget.open_local_resume_session_picker();
                                    }
                                }
                                crate::app_event::AgentPrepareIntent::Candidate { .. } => {
                                    unreachable!("candidate handled above")
                                }
                            }
                        }
                    }
                    Err(error) => {
                        self.prepared_agent_initial_context = None;
                        self.deferred_spawn_pending = false;
                        self.chat_widget
                            .add_error_message(format!("Couldn't prepare agent: {error}"));
                        self.chat_widget.add_info_message(
                            "No session is active - /resume retries the picker, /new starts a \
                             fresh session."
                                .to_string(),
                            None,
                        );
                        if !self.cloud_mode && self.candidate_agent.is_none() {
                            self.chat_widget.open_agent_recovery_popup(&error);
                        }
                    }
                }
                tui.frame_requester().schedule_frame();
            }
            AppEvent::ActivatePreparedResume {
                acp_session_id,
                title,
                transcript,
            } => {
                if self.prepared_agent.is_none() {
                    self.pending_session_activation = Some(PendingSessionActivation::Resume {
                        acp_session_id,
                        title,
                        transcript: transcript.map(Box::new),
                    });
                    self.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Idle);
                    return Ok(true);
                }
                self.pending_session_activation = Some(PendingSessionActivation::Resume {
                    acp_session_id: acp_session_id.clone(),
                    title: title.clone(),
                    transcript: transcript.clone().map(Box::new),
                });
                let agent = match self.take_refreshed_prepared_agent() {
                    Ok(Some(agent)) => agent,
                    Ok(None) => return Ok(true),
                    Err(error) => {
                        self.chat_widget
                            .add_error_message(format!("Couldn't resume prepared agent: {error}"));
                        return Ok(true);
                    }
                };
                self.pending_session_activation = None;
                let (initial_prompt, initial_images) = self.chat_widget.take_initial_input();
                self.shutdown_current_conversation();
                let mut init = self.chat_widget_init(
                    tui.frame_requester(),
                    initial_prompt,
                    initial_images,
                    None,
                    false,
                    None,
                );
                init.prepared_agent = Some(agent);
                self.chat_widget =
                    ChatWidget::new_resumed_acp(init, acp_session_id, title, transcript);
                self.configure_new_chat_widget();
                tui.frame_requester().schedule_frame();
            }
            AppEvent::OpenAgentSessionPicker => {
                self.discard_candidate();
                self.pending_session_activation = None;
                if let Some(agent) = &self.prepared_agent {
                    let can_resume_catalog = agent.capabilities().load_session
                        || agent.capabilities().session_capabilities.resume.is_some();
                    match agent.catalog() {
                        nori_harness::runtime::SessionCatalog::Listed(sessions)
                            if can_resume_catalog =>
                        {
                            self.chat_widget
                                .show_acp_resume_session_picker(sessions.clone(), false);
                        }
                        _ => self.chat_widget.open_local_resume_session_picker(),
                    }
                } else {
                    self.begin_agent_preparation(
                        crate::app_event::AgentPrepareIntent::ResumePicker,
                    );
                }
            }
            AppEvent::BeginExit => {
                self.cancel_primary_agent_preparation();
                if let Some(agent) = self.prepared_agent.take() {
                    tokio::spawn(agent.shutdown());
                }
                self.prepared_agent_initial_context = None;
                self.discard_candidate();
                self.chat_widget.begin_exit();
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
            AppEvent::ConsolidateAgentMessage { source, cwd } => {
                let consolidation = crate::transcript_reflow::consolidate_agent_message_cells(
                    &mut self.transcript_cells,
                    source,
                    &cwd,
                );
                if let Some((range, replacement)) = consolidation
                    && let Some(Overlay::Transcript(transcript)) = &mut self.overlay
                {
                    transcript.replace_cells(range, replacement);
                    tui.frame_requester().schedule_frame();
                }
                if self.transcript_reflow.take_stream_finish_reflow_needed() {
                    self.transcript_reflow.schedule_immediate();
                    tui.frame_requester().schedule_frame();
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
            AppEvent::SessionEvent { generation, event } => {
                let candidate_matches = matches!(
                    &self.candidate_agent,
                    Some(CandidateAgent::Activating { widget, .. })
                        if widget.session_generation() == generation
                );
                if candidate_matches {
                    let started = match &event {
                        nori_protocol::SessionEvent::Nori(
                            nori_protocol::NoriEvent::SessionStarted(started),
                        ) => Some(started.clone()),
                        _ => None,
                    };
                    let ended_message = match &event {
                        nori_protocol::SessionEvent::Nori(
                            nori_protocol::NoriEvent::SessionEnded(ended),
                        ) => Some(ended.message.clone().unwrap_or_else(|| {
                            "Candidate session ended during activation".to_string()
                        })),
                        _ => None,
                    };
                    if let Some(CandidateAgent::Activating { widget, .. }) =
                        self.candidate_agent.as_mut()
                    {
                        widget.handle_session_event(generation, event);
                    }

                    if let Some(started) = started {
                        let Some(CandidateAgent::Activating {
                            agent_name,
                            display_name,
                            widget,
                        }) = self.candidate_agent.take()
                        else {
                            unreachable!("candidate generation checked above")
                        };
                        let mut old_widget = std::mem::replace(&mut self.chat_widget, *widget);
                        self.config.active_agent = agent_name.clone();
                        self.config.agent = agent_name;
                        self.configure_new_chat_widget();
                        if let Some(handle) = self.chat_widget.harness_handle()
                            && let Err(error) = self
                                .remote_control
                                .attach_started(handle, self.config.nori_home.clone(), started)
                                .await
                        {
                            tracing::warn!(%error, "failed to attach committed candidate to remote ACP host");
                        }
                        let (initial_prompt, initial_images) = old_widget.take_initial_input();
                        self.chat_widget
                            .submit_launch_input(initial_prompt, initial_images);
                        self.cancel_primary_agent_preparation();
                        if let Some(agent) = self.prepared_agent.take() {
                            tokio::spawn(agent.shutdown());
                        }
                        self.prepared_agent_initial_context = None;
                        old_widget.shutdown_harness_session();
                        if let Err(error) = ConfigEditsBuilder::new(&self.config.nori_home)
                            .set_agent(&self.config.active_agent)
                            .apply()
                            .await
                        {
                            tracing::error!(%error, "failed to persist activated agent selection");
                        }
                        self.chat_widget.add_info_message(
                            format!("Started new conversation with agent: {display_name}"),
                            None,
                        );
                    } else if let Some(message) = ended_message {
                        let candidate = self.candidate_agent.take();
                        if let Some(CandidateAgent::Activating {
                            agent_name, widget, ..
                        }) = candidate
                        {
                            widget.shutdown_harness_session();
                            self.chat_widget.set_login_agent_override(Some(agent_name));
                        }
                        self.chat_widget.add_error_message(message);
                    }
                } else {
                    let started = match &event {
                        nori_protocol::SessionEvent::Nori(
                            nori_protocol::NoriEvent::SessionStarted(started),
                        ) if self.chat_widget.session_generation() == generation => {
                            Some(started.clone())
                        }
                        _ => None,
                    };
                    self.chat_widget.handle_session_event(generation, event);
                    if let Some(started) = started
                        && let Some(handle) = self.chat_widget.harness_handle()
                        && let Err(error) = self
                            .remote_control
                            .attach_started(handle, self.config.nori_home.clone(), started)
                            .await
                    {
                        tracing::warn!(%error, "failed to attach session to remote ACP host");
                    }
                }
            }
            AppEvent::ConversationHistory(ev) => {
                self.on_conversation_history_for_backtrack(tui, ev)?;
            }
            AppEvent::ExitRequest => {
                self.cancel_primary_agent_preparation();
                if let Some(agent) = self.prepared_agent.take() {
                    agent.shutdown().await;
                }
                self.prepared_agent_initial_context = None;
                self.discard_candidate();
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
            AppEvent::HarnessAction(action) => {
                self.chat_widget.submit_harness_action(action);
            }
            AppEvent::HistoryEntryLoaded {
                log_id,
                offset,
                entry,
            } => self
                .chat_widget
                .on_history_entry_loaded(log_id, offset, entry),
            AppEvent::HistorySearchLoaded(entries) => {
                self.chat_widget.on_history_search_loaded(entries);
            }
            AppEvent::CustomPromptsLoaded(prompts) => {
                self.chat_widget.on_custom_prompts_loaded(prompts);
            }
            AppEvent::UndoSnapshotsLoaded(snapshots) => {
                self.chat_widget.on_undo_snapshots_loaded(snapshots);
            }
            AppEvent::GoalLoaded(Some(goal)) => self.chat_widget.handle_thread_goal_updated(goal),
            AppEvent::GoalLoaded(None) => self.chat_widget.handle_thread_goal_cleared(),
            AppEvent::HarnessActionFailed(message) => self.chat_widget.add_error_message(message),
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
                    match ConfigEditsBuilder::new(&self.config.nori_home)
                        .set_path(&["features", "enable_experimental_windows_sandbox"], true)
                        .apply()
                        .await
                    {
                        Ok(()) => {
                            codex_sandbox::set_windows_sandbox_enabled(true);
                            self.config.windows_sandbox_enabled = true;
                            self.config.forced_auto_mode_downgraded_on_windows = false;
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
            AppEvent::ApplyApprovalPreset { approval, sandbox } => {
                #[cfg(target_os = "windows")]
                let sandbox_is_workspace_write_or_ro = matches!(
                    sandbox,
                    nori_config::SandboxPolicy::WorkspaceWrite { .. }
                        | nori_config::SandboxPolicy::ReadOnly
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

                    let should_check = codex_sandbox::get_platform_sandbox().is_some()
                        && sandbox_is_workspace_write_or_ro
                        && !self.chat_widget.world_writable_warning_hidden();
                    if should_check {
                        let cwd = self.config.cwd.clone();
                        let env_map: std::collections::HashMap<String, String> =
                            std::env::vars().collect();
                        let tx = self.app_event_tx.clone();
                        let logs_base_dir = self.config.nori_home.clone();
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
                if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
                    .set_path(&["notice", "hide_full_access_warning"], true)
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
                if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
                    .set_path(&["notice", "hide_world_writable_warning"], true)
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
            AppEvent::OpenApprovalsPopup => {
                self.chat_widget.open_approvals_popup();
            }
            AppEvent::FullScreenApprovalRequest(request) => {
                let ApprovalRequest {
                    title,
                    kind,
                    cwd,
                    snapshot,
                    ..
                } = request;
                let _ = tui.enter_alt_screen();

                let edit_changes = if matches!(
                    kind,
                    crate::presentation::ToolKind::Create
                        | crate::presentation::ToolKind::Edit
                        | crate::presentation::ToolKind::Delete
                        | crate::presentation::ToolKind::Move
                ) {
                    let mut changes =
                        client_tool_cell::diff_changes_from_artifacts(&snapshot.artifacts, &cwd);
                    if changes.is_empty() {
                        changes =
                            client_tool_cell::changes_from_invocation(&snapshot.invocation, &cwd);
                    }
                    if changes.is_empty() {
                        None
                    } else {
                        Some(changes)
                    }
                } else {
                    None
                };

                if let Some(changes) = edit_changes {
                    let diff_summary = DiffSummary::new(changes, cwd);
                    self.overlay = Some(Overlay::new_static_with_renderables(
                        vec![diff_summary.into()],
                        "P A T C H".to_string(),
                    ));
                } else {
                    let rel_title = client_event_format::relativize_paths_in_text(&title, &cwd);
                    let mut lines = vec![Line::from(rel_title.clone())];
                    if let Some(inv_text) =
                        client_event_format::format_invocation(&snapshot.invocation)
                    {
                        let rel_inv =
                            client_event_format::relativize_paths_in_text(&inv_text, &cwd);
                        if !client_event_format::is_invocation_redundant(&rel_inv, &rel_title) {
                            lines.push(Line::from(rel_inv));
                        }
                    }
                    for text in client_event_format::format_artifacts(&snapshot.artifacts) {
                        lines.push(Line::from(text));
                    }
                    self.overlay =
                        Some(Overlay::new_static_with_lines(lines, "T O O L".to_string()));
                }
            }
            AppEvent::PrepareAgentCandidate {
                agent_name,
                display_name,
            } => {
                self.begin_agent_candidate(agent_name, display_name);
            }
            AppEvent::CancelAgentCandidate => {
                self.discard_candidate();
                self.chat_widget
                    .add_info_message("Agent switch cancelled.".to_string(), None);
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
            AppEvent::OpenAcpModelPickerUnsupported => {
                self.chat_widget.open_model_unsupported_popup();
            }
            AppEvent::OpenAcpSessionConfigPicker {
                config_options,
                focus_config_id,
            } => {
                self.chat_widget
                    .open_acp_session_config_picker(config_options, focus_config_id);
            }
            AppEvent::OpenAcpSessionConfigValuePicker { option } => {
                self.chat_widget
                    .open_acp_session_config_value_picker(option);
            }
            AppEvent::OpenCustomModelInput {
                config_id,
                option_name,
            } => {
                self.chat_widget
                    .open_custom_model_input(config_id, option_name);
            }
            AppEvent::SetAcpSessionConfigOption {
                config_id,
                value,
                option_name,
                value_name,
                is_custom_model,
            } => {
                self.chat_widget.set_acp_session_config_option(
                    config_id,
                    value,
                    option_name,
                    value_name,
                    is_custom_model,
                );
            }
            AppEvent::AcpSessionConfigSetResult {
                success,
                agent,
                config_id,
                value,
                option_name,
                value_name,
                is_custom_model,
                config_options,
                error,
            } => {
                if success {
                    let saved_as_default = match self
                        .persist_default_model_selection(
                            &agent,
                            &config_id,
                            &value,
                            config_options.as_deref().unwrap_or_default(),
                        )
                        .await
                    {
                        Ok(persisted) => persisted,
                        Err(err) => {
                            tracing::error!(
                                error = %err,
                                "failed to persist default model selection"
                            );
                            false
                        }
                    };
                    self.chat_widget.add_acp_session_config_set_message(
                        &option_name,
                        &value_name,
                        saved_as_default,
                    );
                    if let Some(config_options) = config_options {
                        self.chat_widget
                            .sync_acp_session_config_snapshot(&config_options);
                    }
                } else if is_custom_model
                    && nori_harness::get_agent_config(&agent)
                        .map(|config| config.supports_model_injection())
                        .unwrap_or(false)
                {
                    // The agent rejected this model from its live picker, but we
                    // can still run it: persist it as the default and restart so
                    // it is forced through the agent's spawn-time channel.
                    if self.persist_custom_default_model(&agent, &value).await {
                        self.chat_widget.add_info_message(
                            format!(
                                "Saved '{value}' as the default model for {agent} - restarting the session to apply it."
                            ),
                            None,
                        );
                        self.app_event_tx.send(AppEvent::NewSession);
                    }
                } else {
                    let error_msg = error.unwrap_or_else(|| "Unknown error".to_string());
                    self.chat_widget.add_info_message(
                        format!("Failed to set {option_name}: {error_msg}"),
                        None,
                    );
                }
            }
            AppEvent::AcpSessionConfigSnapshot {
                generation,
                config_options,
            } => {
                self.chat_widget
                    .handle_acp_session_config_snapshot(generation, &config_options);
            }
            AppEvent::AcpModeConfigSnapshot { generation, mode } => {
                self.chat_widget
                    .apply_acp_mode_config_snapshot(generation, mode);
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
            AppEvent::OpenNotifyAfterIdlePicker => {
                self.chat_widget
                    .open_notify_after_idle_picker(self.config.notify_after_idle);
            }
            AppEvent::SetConfigNotifyAfterIdle(value) => {
                self.persist_notify_after_idle_setting(value).await;
            }
            AppEvent::OpenScriptTimeoutPicker => {
                self.chat_widget
                    .open_script_timeout_picker(self.config.script_timeout.clone());
            }
            AppEvent::SetConfigScriptTimeout(value) => {
                self.persist_script_timeout_setting(value).await;
            }
            AppEvent::OpenLoopCountPicker => {
                let current = match self.loop_count_override {
                    Some(overridden) => overridden,
                    None => self.config.loop_count,
                };
                self.chat_widget.open_loop_count_picker(current);
            }
            AppEvent::SetConfigLoopCount(value) => {
                self.set_session_loop_count(value);
            }
            AppEvent::OpenVimModePicker => {
                self.chat_widget
                    .open_vim_mode_picker(self.config.vim_mode, true);
            }
            AppEvent::OpenAutoWorktreePicker => {
                self.chat_widget
                    .open_auto_worktree_picker(self.config.auto_worktree);
            }
            AppEvent::SetConfigAutoWorktree(value) => {
                self.persist_auto_worktree_setting(value).await;
            }
            AppEvent::SetConfigSkillsetPerSession(enabled) => {
                self.persist_skillset_per_session_setting(enabled).await;
            }
            AppEvent::SetConfigPinnedPlanDrawer(enabled) => {
                self.persist_pinned_plan_drawer_setting(enabled).await;
            }
            AppEvent::SetConfigResizeReflow(enabled) => {
                self.persist_resize_reflow_setting(enabled, tui).await;
            }
            AppEvent::SetConfigAcpWireRecording(enabled) => {
                self.persist_acp_wire_recording_setting(enabled).await;
            }
            AppEvent::SetConfigCustomWorkingMessages(enabled) => {
                self.persist_custom_working_messages_setting(enabled).await;
            }
            AppEvent::OpenSkillsetPerSessionWorktreeChoice => {
                self.chat_widget.open_skillset_worktree_choice_picker();
            }
            AppEvent::OpenFooterSegmentsPicker => {
                self.chat_widget
                    .open_footer_segments_picker(&self.footer_segment_config);
            }
            AppEvent::SetConfigFooterSegment(segment, enabled) => {
                self.persist_footer_segment_setting(segment, enabled).await;
            }
            AppEvent::BrowseFiles(fm) => {
                self.browse_files(fm, tui);
            }
            AppEvent::SetConfigFileManager(value) => {
                self.persist_file_manager_setting(value).await;
            }
            AppEvent::OpenFileManagerPicker => {
                self.chat_widget
                    .open_file_manager_picker(self.config.file_manager);
            }
            AppEvent::LoopIteration {
                prompt,
                remaining,
                total,
            } => {
                let iteration = total - remaining;
                tracing::info!("Loop iteration {iteration}/{total} (remaining: {remaining})");

                self.shutdown_current_conversation();

                let init = self.chat_widget_init(
                    tui.frame_requester(),
                    Some(prompt),
                    Vec::new(),
                    None,
                    true,
                    None,
                );
                self.chat_widget = ChatWidget::new(init);
                self.configure_new_chat_widget();
                self.chat_widget.set_loop_state(remaining, total);
                self.pending_session_activation = Some(PendingSessionActivation::New);
                self.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Idle);

                self.chat_widget
                    .add_info_message(format!("Loop iteration {iteration} of {total}"), None);
            }
            AppEvent::SetConfigVimMode {
                value,
                from_settings,
            } => {
                self.persist_vim_mode_setting(value, from_settings).await;
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
                self.on_skillset_applied(success);
            }
            AppEvent::SkillsetSwitchResult {
                name,
                success,
                message,
            } => {
                self.chat_widget
                    .on_skillset_switch_result(&name, success, &message);
                self.on_skillset_applied(success);
            }
            AppEvent::SkillsetPickerDismissed => {
                // The skillset picker was dismissed without selection. If the
                // agent spawn was deferred, spawn it now without a skillset
                // (behaves as if skillset_per_session is disabled).
                if self.take_deferred_spawn() {
                    self.begin_agent_preparation(crate::app_event::AgentPrepareIntent::Idle);
                }
            }
            AppEvent::ExecuteScript { prompt, args } => {
                let tx = self.app_event_tx.clone();
                let timeout = self.config.script_timeout.as_duration();
                let name = prompt.name.clone();
                self.chat_widget
                    .add_info_message(format!("Running script '{name}'..."), None);
                tokio::spawn(async move {
                    let result =
                        nori_harness::custom_prompts::execute_script(&prompt, &args, timeout).await;
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
                    let loader = nori_harness::transcript::TranscriptLoader::new(nori_home);
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
                generation,
            } => {
                let params =
                    crate::nori::resume_session_picker::resume_session_component_picker_params(
                        sessions, nori_home,
                    );
                self.chat_widget
                    .show_resume_session_picker(params, generation);
            }
            AppEvent::ShowAcpResumeSessionPicker { sessions } => {
                self.chat_widget
                    .show_acp_resume_session_picker(sessions, false);
            }
            AppEvent::ResumeSessionSummaryReady {
                generation,
                session_id,
                started_at,
                first_message_preview,
                user_turn_count,
            } => {
                self.chat_widget.update_resume_session_picker_item(
                    generation,
                    &session_id,
                    &started_at,
                    first_message_preview.as_deref(),
                    user_turn_count,
                );
            }
            AppEvent::ResumeSession {
                nori_home,
                project_id,
                session_id,
            } => {
                let loader = nori_harness::transcript::TranscriptLoader::new(nori_home);
                match loader.load_transcript(&project_id, &session_id).await {
                    Ok(transcript) => {
                        let acp_session_id = transcript.meta.acp_session_id.clone();
                        let display_name =
                            crate::nori::agent_picker::get_agent_info(&self.config.active_agent)
                                .map(|info| info.display_name)
                                .unwrap_or_else(|| self.config.active_agent.clone());

                        if self.prepared_agent.is_none() {
                            self.defer_resume_activation(
                                tui.frame_requester(),
                                acp_session_id,
                                None,
                                Some(transcript),
                            );
                            self.chat_widget.add_info_message(
                                format!("Resuming session with {display_name}..."),
                                None,
                            );
                            tui.frame_requester().schedule_frame();
                            return Ok(true);
                        }

                        self.pending_session_activation = Some(PendingSessionActivation::Resume {
                            acp_session_id: acp_session_id.clone(),
                            title: None,
                            transcript: Some(Box::new(transcript.clone())),
                        });
                        let agent = match self.take_refreshed_prepared_agent() {
                            Ok(Some(agent)) => agent,
                            Ok(None) => return Ok(true),
                            Err(error) => {
                                self.chat_widget.add_error_message(format!(
                                    "Couldn't resume prepared agent: {error}"
                                ));
                                return Ok(true);
                            }
                        };
                        self.pending_session_activation = None;

                        let (initial_prompt, initial_images) =
                            self.chat_widget.take_initial_input();
                        self.shutdown_current_conversation();

                        let mut init = self.chat_widget_init(
                            tui.frame_requester(),
                            initial_prompt,
                            initial_images,
                            None,
                            false,
                            None,
                        );
                        init.prepared_agent = Some(agent);
                        self.chat_widget = ChatWidget::new_resumed_acp(
                            init,
                            acp_session_id,
                            None,
                            Some(transcript),
                        );
                        self.configure_new_chat_widget();

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
            AppEvent::ResumeAcpSession {
                acp_session_id,
                title,
            } => {
                if matches!(self.candidate_agent, Some(CandidateAgent::Prepared { .. })) {
                    let Some(CandidateAgent::Prepared {
                        agent_name,
                        display_name,
                        mut agent,
                        ..
                    }) = self.candidate_agent.take()
                    else {
                        unreachable!("candidate variant checked above")
                    };
                    let config = self.config_for_agent(&agent_name);
                    if let Err(error) = nori_harness::runtime::refresh_prepared_agent(
                        &mut agent,
                        crate::chatwidget::agent::agent_prepare_spec(config.clone(), None),
                    ) {
                        tokio::spawn((*agent).shutdown());
                        self.chat_widget.set_login_agent_override(Some(agent_name));
                        self.chat_widget.add_error_message(format!(
                            "Couldn't activate {display_name}: {error}"
                        ));
                        return Ok(true);
                    }
                    let mut init = self.chat_widget_init(
                        tui.frame_requester(),
                        None,
                        Vec::new(),
                        Some(config.active_agent.clone()),
                        false,
                        None,
                    );
                    init.config = config;
                    init.prepared_agent = Some(*agent);
                    let widget = ChatWidget::new_resumed_acp_candidate(
                        init,
                        Some(acp_session_id),
                        title,
                        None,
                    );
                    self.candidate_agent = Some(CandidateAgent::Activating {
                        agent_name,
                        display_name,
                        widget: Box::new(widget),
                    });
                    tui.frame_requester().schedule_frame();
                    return Ok(true);
                }

                self.discard_candidate();
                self.cancel_primary_agent_preparation();
                let display_name =
                    crate::nori::agent_picker::get_agent_info(&self.config.active_agent)
                        .map(|info| info.display_name)
                        .unwrap_or_else(|| self.config.active_agent.clone());
                if self.prepared_agent.is_none() {
                    self.defer_resume_activation(
                        tui.frame_requester(),
                        Some(acp_session_id.clone()),
                        title.clone(),
                        None,
                    );
                    self.chat_widget.add_info_message(
                        reattach_info_message(
                            &acp_session_id,
                            title.as_deref(),
                            self.cloud_mode,
                            &display_name,
                        ),
                        None,
                    );
                    tui.frame_requester().schedule_frame();
                    return Ok(true);
                }
                self.pending_session_activation = Some(PendingSessionActivation::Resume {
                    acp_session_id: Some(acp_session_id.clone()),
                    title: title.clone(),
                    transcript: None,
                });
                let agent = match self.take_refreshed_prepared_agent() {
                    Ok(Some(agent)) => agent,
                    Ok(None) => return Ok(true),
                    Err(error) => {
                        self.chat_widget
                            .add_error_message(format!("Couldn't resume prepared agent: {error}"));
                        return Ok(true);
                    }
                };
                self.pending_session_activation = None;
                let (initial_prompt, initial_images) = self.chat_widget.take_initial_input();
                self.deferred_spawn_pending = false;
                self.shutdown_current_conversation();

                let mut init = self.chat_widget_init(
                    tui.frame_requester(),
                    initial_prompt,
                    initial_images,
                    None,
                    false,
                    None,
                );
                init.prepared_agent = Some(agent);
                self.chat_widget = ChatWidget::new_resumed_acp(
                    init,
                    Some(acp_session_id.clone()),
                    title.clone(),
                    None,
                );
                self.configure_new_chat_widget();

                self.chat_widget.add_info_message(
                    reattach_info_message(
                        &acp_session_id,
                        title.as_deref(),
                        self.cloud_mode,
                        &display_name,
                    ),
                    None,
                );
                tui.frame_requester().schedule_frame();
            }
            #[cfg(unix)]
            AppEvent::BrowserLaunched { ws_url, cdp_port } => {
                let prompt =
                    nori_harness::backend::browser_session::compose_agent_prompt(&ws_url, cdp_port);
                self.chat_widget.add_info_message(
                    format!("Browser launched (CDP port {cdp_port}). Notifying agent..."),
                    None,
                );
                self.chat_widget.submit_user_message_text(prompt);
            }
            AppEvent::BrowserLaunchFailed(err) => {
                self.chat_widget
                    .add_error_message(format!("Failed to launch browser: {err}"));
            }
            #[cfg(unix)]
            AppEvent::SetBrowserProfile(mode) => {
                // Persist the choice as the new default, then launch with it.
                self.persist_browser_profile_setting(mode).await;
                self.chat_widget.launch_browser_session(mode);
            }
            #[cfg(not(unix))]
            AppEvent::SetBrowserProfile(_mode) => {}
            AppEvent::OpenForkPicker => {
                let messages =
                    crate::app_backtrack::collect_all_user_messages(&self.transcript_cells);
                // Only agents that advertise ACP `session/fork` can branch at the
                // current head; without it, the picker only rewinds to a message.
                let supports_fork = self.chat_widget.agent_capabilities().session_fork;
                if !supports_fork && messages.is_empty() {
                    self.chat_widget
                        .add_info_message("No messages to fork from.".to_string(), None);
                } else {
                    let params = crate::nori::fork_picker::fork_picker_params(
                        messages,
                        supports_fork,
                        self.app_event_tx.clone(),
                    );
                    self.chat_widget.show_selection_view(params);
                }
                tui.frame_requester().schedule_frame();
            }
            AppEvent::BranchFromCurrent => {
                self.chat_widget
                    .submit_harness_action(crate::app_event::HarnessAction::Branch);
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
                let init = self.chat_widget_init(
                    tui.frame_requester(),
                    None,
                    Vec::new(),
                    None,
                    true,
                    None,
                );
                self.chat_widget = ChatWidget::new(init);
                self.configure_new_chat_widget();
                self.pending_session_activation = Some(PendingSessionActivation::New);
                self.begin_agent_preparation_with_context(
                    crate::app_event::AgentPrepareIntent::Idle,
                    fork_context,
                );

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
            AppEvent::McpOAuthLogin {
                server_name,
                server_url,
                http_headers,
                env_http_headers,
                client_id,
                client_secret_env_var,
            } => {
                self.perform_mcp_oauth_login(
                    tui,
                    server_name,
                    server_url,
                    http_headers,
                    env_http_headers,
                    client_id,
                    client_secret_env_var,
                )
                .await;
            }
            AppEvent::McpOAuthLoginCancel { server_name } => {
                tracing::info!("MCP OAuth login cancelled for {server_name}");
                self.cancel_mcp_oauth_login();
                self.chat_widget.add_info_message(
                    format!("OAuth cancelled for `{server_name}`. Press `l` in /mcp to try again."),
                    None,
                );
            }
            AppEvent::McpOAuthLoginComplete {
                server_name,
                success,
                error,
            } => {
                if success {
                    self.chat_widget.add_info_message(
                        format!(
                            "Successfully authenticated with `{server_name}`. Restart to apply."
                        ),
                        None,
                    );
                } else {
                    let msg = match error {
                        Some(err) => {
                            format!("OAuth login for `{server_name}` failed: {err}")
                        }
                        None => format!("OAuth login for `{server_name}` failed."),
                    };
                    self.chat_widget.add_error_message(msg);
                }
                self.chat_widget
                    .handle_mcp_oauth_complete(&server_name, success);
            }
            AppEvent::ComputeMcpAuthStatuses => {
                let servers = self.chat_widget.config_ref().mcp_servers.clone();
                let tx = self.app_event_tx.clone();
                tokio::spawn(async move {
                    let mut statuses = std::collections::HashMap::new();
                    for (name, config) in servers {
                        let status = match config.transport {
                            nori_config::McpServerTransportConfig::Stdio {
                                ..
                            } => codex_rmcp_client::McpAuthStatus::Unsupported,
                            nori_config::McpServerTransportConfig::StreamableHttp {
                                url,
                                bearer_token_env_var,
                                http_headers,
                                env_http_headers,
                                ..
                            } => codex_rmcp_client::determine_streamable_http_auth_status(
                                &name,
                                &url,
                                bearer_token_env_var.as_deref(),
                                http_headers,
                                env_http_headers,
                                codex_rmcp_client::OAuthCredentialsStoreMode::Auto,
                            )
                            .await
                            .unwrap_or_else(|err| {
                                tracing::warn!(
                                    "failed to determine auth status for MCP server `{name}`: {err:?}"
                                );
                                codex_rmcp_client::McpAuthStatus::Unsupported
                            }),
                        };
                        statuses.insert(name, status);
                    }
                    tx.send(AppEvent::McpAuthStatusesReady(statuses));
                });
            }
            AppEvent::McpAuthStatusesReady(statuses) => {
                self.chat_widget.update_mcp_auth_statuses(&statuses);
                tui.frame_requester().schedule_frame();
            }
        }
        Ok(true)
    }

    pub(super) async fn handle_key_event(&mut self, tui: &mut tui::Tui, key_event: KeyEvent) {
        use crate::nori::hotkey_match::matches_binding;
        use nori_config::HotkeyAction;

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
                if self.should_handle_backtrack_esc(key_event) {
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

    pub(super) fn should_handle_backtrack_esc(&self, key_event: KeyEvent) -> bool {
        self.chat_widget.is_normal_backtrack_mode()
            && self.chat_widget.composer_is_empty()
            && !self.chat_widget.should_handle_vim_insert_escape(key_event)
    }
}

/// Compose the info-cell message shown when resuming/reattaching to a session.
///
/// Cloud reattach wording names the selected broker session. It deliberately
/// makes no claim about replay: that depends on the facade's capabilities.
pub(super) fn reattach_info_message(
    acp_session_id: &str,
    title: Option<&str>,
    cloud_mode: bool,
    display_name: &str,
) -> String {
    if cloud_mode {
        let session_label = match title {
            Some(title) => format!("{acp_session_id} ({title})"),
            None => acp_session_id.to_string(),
        };
        format!("Reattaching to {session_label}...")
    } else {
        format!("Resuming session with {display_name}...")
    }
}

#[cfg(test)]
mod tests {
    use super::reattach_info_message;

    #[test]
    fn cloud_reattach_info_message_snapshot() {
        insta::assert_snapshot!(
            "cloud_reattach_info_message",
            reattach_info_message("session-123", Some("Fix reconnect"), true, "Nori Cloud")
        );
    }
}
