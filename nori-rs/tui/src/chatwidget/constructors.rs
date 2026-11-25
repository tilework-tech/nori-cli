use super::agent::SpawnAgentResult;
use super::agent::next_session_generation;
use super::*;

impl ChatWidget {
    pub(crate) fn new(common: ChatWidgetInit) -> Self {
        Self::new_inner(common)
    }

    /// Display a session already resumed by the manager, retaining its registry identity.
    pub(crate) fn from_managed_session(
        mut common: ChatWidgetInit,
        session: nori_harness::session_manager::ManagedSession,
    ) -> Self {
        common.deferred_spawn = true;
        let analytics = common.analytics.clone();
        let delegated_child = common
            .session_manager
            .get(&session.conversation_id)
            .is_some_and(|record| {
                record.source == nori_harness::session_manager::SessionSource::Session
            });
        let mut widget = Self::new(common);
        widget.delegated_child = delegated_child;
        let spawn = super::agent::forward_managed_session(
            session,
            widget.app_event_tx.clone(),
            widget.session_generation,
            analytics,
        );
        widget.harness_handle = spawn.handle;
        widget.managed_conversation_id = spawn.conversation_id;
        widget
    }

    /// Observe a child using the manager's atomic replay and live subscription.
    pub(crate) fn from_managed_view(
        mut common: ChatWidgetInit,
        id: ConversationId,
        handle: Option<HarnessHandle>,
        subscription: nori_harness::remote_agent::ManagedSubscription,
    ) -> Self {
        common.deferred_spawn = true;
        let mut widget = Self::new(common);
        widget.delegated_child = true;
        widget.managed_conversation_id = Some(id);
        widget.harness_handle = handle;
        widget.set_foreground(false);
        widget.suppress_welcome_card();
        let tx = widget.app_event_tx.clone();
        let generation = widget.session_generation;
        super::agent::forward_managed_view(id, subscription, tx, generation);
        widget
    }

    /// Follow a conversation the host started for this terminal, from its
    /// first journal entry, so history replayed before the host's answer is
    /// shown. `saved` names the agent's saved session it restores.
    pub(crate) fn from_host_activation(
        mut common: ChatWidgetInit,
        id: ConversationId,
        handle: Option<HarnessHandle>,
        subscription: nori_harness::remote_agent::ManagedSubscription,
        saved: Option<(String, Option<String>)>,
    ) -> Self {
        common.deferred_spawn = true;
        let analytics = common.analytics.clone();
        let mut widget = Self::new_inner(common);
        widget.harness_handle = match (analytics, handle) {
            (Some(reporter), Some(handle)) => Some(reporter.attach(handle)),
            (_, handle) => handle,
        };
        widget.managed_conversation_id = Some(id);
        if let Some((acp_session_id, title)) = saved {
            widget.acp_session_id = Some(acp_session_id);
            widget.cloud_session_title = title;
        }
        let tx = widget.app_event_tx.clone();
        let generation = widget.session_generation;
        super::agent::forward_managed_view(id, subscription, tx, generation);
        widget
    }

    fn new_inner(common: ChatWidgetInit) -> Self {
        let ChatWidgetInit {
            config,
            frame_requester,
            app_event_tx,
            initial_prompt,
            initial_images,
            enhanced_keys_supported,
            auth_manager,
            vertical_footer,
            footer_segment_config,
            footer_layout_config,
            cloud_mode,
            deferred_spawn,
            fork_context,
            prepared_agent,
            analytics,
            session_manager,
            clock,
        } = common;
        let activating_prepared_cloud_session = cloud_mode && prepared_agent.is_some();
        let mut rng = rand::rng();
        let placeholder = PROMPT_MODE_PLACEHOLDERS
            [rng.random_range(0..PROMPT_MODE_PLACEHOLDERS.len())]
        .to_string();
        let session_generation = next_session_generation();
        let app_event_tx = app_event_tx.for_session(session_generation);
        let spawn_result = if let Some(agent) = prepared_agent {
            super::agent::launch_prepared_agent(
                &session_manager,
                agent,
                nori_harness::runtime::SessionStart::New,
                app_event_tx.clone(),
                session_generation,
                analytics,
            )
        } else if deferred_spawn {
            SpawnAgentResult {
                handle: None,
                conversation_id: None,
            }
        } else {
            spawn_agent(
                &session_manager,
                config.clone(),
                app_event_tx.clone(),
                session_generation,
                fork_context,
                analytics,
            )
        };

        let first_prompt_text = initial_prompt.clone();
        let acp_wire_recording_enabled = config.acp_proxy.enabled;
        let mut widget = Self {
            app_event_tx: app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            bottom_pane: BottomPane::new(BottomPaneParams {
                frame_requester,
                app_event_tx,
                has_input_focus: true,
                enhanced_keys_supported,
                placeholder_text: placeholder,
                disable_paste_burst: config.disable_paste_burst,
                animations_enabled: config.animations,
                custom_working_messages: config.custom_working_messages,
                custom_working_message_list: config.custom_working_message_list.clone(),
                vertical_footer,
                footer_segment_config,
                footer_layout_config,
                agent_display_name: crate::nori::agent_picker::get_agent_info(&config.active_agent)
                    .map(|info| info.display_name)
                    .unwrap_or_else(|| config.active_agent.clone()),
                agent_slug: config.active_agent.clone(),
                clock,
            }),
            active_cell: None,
            active_user_message_id: None,
            config: config.clone(),
            auth_manager,
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            stream_controller: None,
            session_generation,
            submitted_prompt: None,
            owned_prompt_request_id: None,
            proactive_turn_active: false,
            external_turn: None,
            unpaired_prompt_error_ids: HashSet::new(),
            completed_client_tool_calls: HashSet::new(),
            user_shell_runs: Default::default(),
            client_event_normalizer: Default::default(),
            replay_source: None,
            replay_message: None,
            reasoning_buffer: String::new(),
            thought_status_text: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: crate::status_indicator_widget::pick_status_message(
                config.custom_working_messages,
                &config.custom_working_message_list,
            ),
            conversation_id: None,
            forked_from: None,
            show_welcome_banner: true,
            suppress_session_configured_redraw: false,
            pending_notification: None,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            current_rollout_path: None,
            pending_client_tool_cells: HashMap::new(),
            effective_cwd_tracker: EffectiveCwdTracker::with_initial_cwd(config.cwd),
            session_configured_received: false,
            harness_handle: spawn_result.handle,
            session_manager,
            managed_conversation_id: spawn_result.conversation_id,
            approval_mode: None,
            available_approval_modes: None,
            session_close_in_flight: false,
            close_refusal: Default::default(),
            closed_by_user: false,
            delegated_child: false,
            delegations: HashMap::new(),
            delegation_approval: None,
            exiting: false,
            agent_config: crate::nori::agent_config_state::AgentConfigState::default(),
            agent_status: crate::nori::session_header::AgentStatusHandle::new(
                crate::nori::session_header::AgentStatus::new(&config.active_agent),
            ),
            acp_mode_config: None,
            acp_mode_config_generation: super::session_config_mode::next_acp_mode_config_generation(
            ),
            session_stats: SessionStats::new(),
            assistant_stream_seen_for_stats: false,
            login_handler: None,
            login_agent_override: None,
            active_resume_picker_generation: None,
            first_prompt_text,
            current_goal: None,
            session_agent_capabilities: crate::presentation::AgentCapabilitiesView::default(),
            cloud_mode,
            session_agent_info: None,
            follow_only_attachment: false,
            session_info_state: Default::default(),
            session_info_detail: crate::nori::session_info::SessionInfoDetail::for_build(),
            acp_session_id: None,
            cloud_session_title: None,
            builtin_command_availability: HashMap::new(),
            pending_goal_status: false,
            pending_goal_edit: false,
            loop_remaining: None,
            loop_total: None,
            loop_count_override: None,
            acp_session_phase: None,
            plan_drawer_mode: PlanDrawerMode::Off,
            pinned_plan: None,
            terminal_title_animation_origin: std::time::Instant::now(),
            last_terminal_title: None,
            program_status: Default::default(),
            foreground: true,
            session_lines: Vec::new(),
        };

        widget
            .bottom_pane
            .set_acp_wire_recording_enabled(acp_wire_recording_enabled);
        if activating_prepared_cloud_session {
            widget.show_connecting_status("Nori Cloud");
        }
        widget
    }

    /// Create a ChatWidget that resumes an ACP session via `session/load`
    /// or client-side replay when the agent doesn't support `session/load`.
    /// `title` is the broker-reported session title, when known.
    pub(crate) fn new_resumed_acp(
        common: ChatWidgetInit,
        acp_session_id: Option<String>,
        title: Option<String>,
        transcript: Option<nori_harness::transcript::Transcript>,
    ) -> Self {
        Self::new_resumed_acp_inner(common, acp_session_id, title, transcript)
    }

    fn new_resumed_acp_inner(
        common: ChatWidgetInit,
        acp_session_id: Option<String>,
        title: Option<String>,
        transcript: Option<nori_harness::transcript::Transcript>,
    ) -> Self {
        let ChatWidgetInit {
            config,
            frame_requester,
            app_event_tx,
            initial_prompt,
            initial_images,
            enhanced_keys_supported,
            auth_manager,
            vertical_footer,
            footer_segment_config,
            footer_layout_config,
            cloud_mode,
            deferred_spawn: _,
            fork_context: _,
            prepared_agent,
            analytics,
            session_manager,
            clock,
        } = common;
        let activating_prepared_cloud_session = cloud_mode && prepared_agent.is_some();
        let mut rng = rand::rng();
        let placeholder = PROMPT_MODE_PLACEHOLDERS
            [rng.random_range(0..PROMPT_MODE_PLACEHOLDERS.len())]
        .to_string();
        let session_generation = next_session_generation();
        let app_event_tx = app_event_tx.for_session(session_generation);
        let spawn_result = if let Some(agent) = prepared_agent {
            super::agent::launch_prepared_agent(
                &session_manager,
                agent,
                nori_harness::runtime::SessionStart::Resume(nori_harness::runtime::SessionResume {
                    acp_session_id: acp_session_id.clone(),
                    transcript,
                }),
                app_event_tx.clone(),
                session_generation,
                analytics,
            )
        } else {
            spawn_acp_agent_resume(
                &session_manager,
                config.clone(),
                acp_session_id.clone(),
                transcript,
                app_event_tx.clone(),
                session_generation,
                analytics,
            )
        };

        let first_prompt_text = initial_prompt.clone();
        let acp_wire_recording_enabled = config.acp_proxy.enabled;
        let mut widget = Self {
            app_event_tx: app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            bottom_pane: BottomPane::new(BottomPaneParams {
                frame_requester,
                app_event_tx,
                has_input_focus: true,
                enhanced_keys_supported,
                placeholder_text: placeholder,
                disable_paste_burst: config.disable_paste_burst,
                animations_enabled: config.animations,
                custom_working_messages: config.custom_working_messages,
                custom_working_message_list: config.custom_working_message_list.clone(),
                vertical_footer,
                footer_segment_config,
                footer_layout_config,
                agent_display_name: crate::nori::agent_picker::get_agent_info(&config.active_agent)
                    .map(|info| info.display_name)
                    .unwrap_or_else(|| config.active_agent.clone()),
                agent_slug: config.active_agent.clone(),
                clock,
            }),
            active_cell: None,
            active_user_message_id: None,
            config: config.clone(),
            auth_manager,
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            stream_controller: None,
            session_generation,
            submitted_prompt: None,
            owned_prompt_request_id: None,
            proactive_turn_active: false,
            external_turn: None,
            unpaired_prompt_error_ids: HashSet::new(),
            completed_client_tool_calls: HashSet::new(),
            user_shell_runs: Default::default(),
            client_event_normalizer: Default::default(),
            replay_source: None,
            replay_message: None,
            reasoning_buffer: String::new(),
            thought_status_text: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: crate::status_indicator_widget::pick_status_message(
                config.custom_working_messages,
                &config.custom_working_message_list,
            ),
            conversation_id: None,
            forked_from: None,
            show_welcome_banner: false,
            suppress_session_configured_redraw: false,
            pending_notification: None,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            current_rollout_path: None,
            pending_client_tool_cells: HashMap::new(),
            effective_cwd_tracker: EffectiveCwdTracker::with_initial_cwd(config.cwd),
            session_configured_received: false,
            harness_handle: spawn_result.handle,
            session_manager,
            managed_conversation_id: spawn_result.conversation_id,
            approval_mode: None,
            available_approval_modes: None,
            session_close_in_flight: false,
            close_refusal: Default::default(),
            closed_by_user: false,
            delegated_child: false,
            delegations: HashMap::new(),
            delegation_approval: None,
            exiting: false,
            agent_config: crate::nori::agent_config_state::AgentConfigState::default(),
            agent_status: crate::nori::session_header::AgentStatusHandle::new(
                crate::nori::session_header::AgentStatus::new(&config.active_agent),
            ),
            acp_mode_config: None,
            acp_mode_config_generation: super::session_config_mode::next_acp_mode_config_generation(
            ),
            session_stats: SessionStats::new(),
            assistant_stream_seen_for_stats: false,
            login_handler: None,
            login_agent_override: None,
            active_resume_picker_generation: None,
            first_prompt_text,
            current_goal: None,
            session_agent_capabilities: crate::presentation::AgentCapabilitiesView::default(),
            cloud_mode,
            session_agent_info: None,
            follow_only_attachment: false,
            session_info_state: Default::default(),
            session_info_detail: crate::nori::session_info::SessionInfoDetail::for_build(),
            acp_session_id,
            cloud_session_title: title,
            builtin_command_availability: HashMap::new(),
            pending_goal_status: false,
            pending_goal_edit: false,
            loop_remaining: None,
            loop_total: None,
            loop_count_override: None,
            acp_session_phase: None,
            plan_drawer_mode: PlanDrawerMode::Off,
            pinned_plan: None,
            terminal_title_animation_origin: std::time::Instant::now(),
            last_terminal_title: None,
            program_status: Default::default(),
            foreground: true,
            session_lines: Vec::new(),
        };

        widget
            .bottom_pane
            .set_acp_wire_recording_enabled(acp_wire_recording_enabled);
        if activating_prepared_cloud_session {
            widget.show_connecting_status("Nori Cloud");
        }
        widget
    }

    pub(crate) fn session_generation(&self) -> crate::app_event::SessionGeneration {
        self.session_generation
    }
}
