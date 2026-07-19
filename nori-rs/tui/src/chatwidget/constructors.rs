use super::agent::SpawnAgentResult;
use super::*;

impl ChatWidget {
    pub(crate) fn new(common: ChatWidgetInit) -> Self {
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
            deferred_spawn,
            fork_context,
        } = common;
        let mut rng = rand::rng();
        let placeholder = PROMPT_MODE_PLACEHOLDERS
            [rng.random_range(0..PROMPT_MODE_PLACEHOLDERS.len())]
        .to_string();
        let spawn_result = if deferred_spawn {
            SpawnAgentResult { handle: None }
        } else {
            spawn_agent(config.clone(), app_event_tx.clone(), fork_context)
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
            }),
            active_cell: None,
            config: config.clone(),
            auth_manager,
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            stream_controller: None,
            completed_client_tool_calls: HashSet::new(),
            client_event_normalizer: Default::default(),
            replay_in_progress: false,
            replay_message: None,
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: crate::status_indicator_widget::pick_status_message(
                config.custom_working_messages,
                &config.custom_working_message_list,
            ),
            conversation_id: None,
            show_welcome_banner: true,
            suppress_session_configured_redraw: false,
            pending_notification: None,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            current_rollout_path: None,
            pending_client_tool_cells: HashMap::new(),
            effective_cwd_tracker: EffectiveCwdTracker::with_initial_cwd(config.cwd),
            pending_agent: None,
            session_configured_received: false,
            harness_handle: spawn_result.handle,
            session_close_in_flight: false,
            exiting: false,
            acp_config_option_snapshot: None,
            acp_mode_config: None,
            acp_mode_config_generation: super::session_config_mode::next_acp_mode_config_generation(
            ),
            session_stats: SessionStats::new(),
            assistant_stream_seen_for_stats: false,
            login_handler: None,
            active_resume_picker_generation: None,
            first_prompt_text,
            current_goal: None,
            session_agent_capabilities: crate::presentation::AgentCapabilitiesView::default(),
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
        };

        widget
            .bottom_pane
            .set_acp_wire_recording_enabled(acp_wire_recording_enabled);
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
            deferred_spawn: _,
            fork_context: _,
        } = common;
        let mut rng = rand::rng();
        let placeholder = PROMPT_MODE_PLACEHOLDERS
            [rng.random_range(0..PROMPT_MODE_PLACEHOLDERS.len())]
        .to_string();
        let spawn_result = spawn_acp_agent_resume(
            config.clone(),
            acp_session_id.clone(),
            transcript,
            app_event_tx.clone(),
        );

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
            }),
            active_cell: None,
            config: config.clone(),
            auth_manager,
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            stream_controller: None,
            completed_client_tool_calls: HashSet::new(),
            client_event_normalizer: Default::default(),
            replay_in_progress: false,
            replay_message: None,
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: crate::status_indicator_widget::pick_status_message(
                config.custom_working_messages,
                &config.custom_working_message_list,
            ),
            conversation_id: None,
            show_welcome_banner: false,
            suppress_session_configured_redraw: false,
            pending_notification: None,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            current_rollout_path: None,
            pending_client_tool_cells: HashMap::new(),
            effective_cwd_tracker: EffectiveCwdTracker::with_initial_cwd(config.cwd),
            pending_agent: None,
            session_configured_received: false,
            harness_handle: spawn_result.handle,
            session_close_in_flight: false,
            exiting: false,
            acp_config_option_snapshot: None,
            acp_mode_config: None,
            acp_mode_config_generation: super::session_config_mode::next_acp_mode_config_generation(
            ),
            session_stats: SessionStats::new(),
            assistant_stream_seen_for_stats: false,
            login_handler: None,
            active_resume_picker_generation: None,
            first_prompt_text,
            current_goal: None,
            session_agent_capabilities: crate::presentation::AgentCapabilitiesView::default(),
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
        };

        widget
            .bottom_pane
            .set_acp_wire_recording_enabled(acp_wire_recording_enabled);
        widget
    }

    /// Set a pending agent to switch to on the next prompt submission.
    pub(crate) fn set_pending_agent(&mut self, agent_name: String, display_name: String) {
        // Update the bottom pane's model display name for approval dialogs
        self.bottom_pane
            .set_agent_display_name(display_name.clone());
        self.bottom_pane.set_agent_slug(agent_name.clone());
        self.pending_agent = Some(PendingAgentInfo {
            agent_name,
            display_name,
        });
    }

    /// Spawn the agent that was deferred during construction.
    ///
    /// This should be called after pre-session setup (e.g., skillset switch)
    /// is complete, so that the agent sees the correct `.claude/CLAUDE.md`.
    pub(crate) fn spawn_deferred_agent(&mut self, config: Config, app_event_tx: AppEventSender) {
        let spawn_result = spawn_agent(config, app_event_tx, None);
        self.harness_handle = spawn_result.handle;
    }
}
