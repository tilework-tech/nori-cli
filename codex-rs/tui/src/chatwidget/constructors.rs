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
            expected_agent,
            deferred_spawn,
            fork_context,
        } = common;
        let mut rng = rand::rng();
        let placeholder = EXAMPLE_PROMPTS[rng.random_range(0..EXAMPLE_PROMPTS.len())].to_string();
        let spawn_result = if deferred_spawn {
            // Deferred spawn: create a dummy channel. The real agent will be
            // spawned later via `spawn_deferred_agent()`.
            let (op_tx, _) = tokio::sync::mpsc::unbounded_channel();
            SpawnAgentResult {
                op_tx,
                #[cfg(feature = "unstable")]
                acp_handle: None,
            }
        } else {
            spawn_agent(config.clone(), app_event_tx.clone(), fork_context)
        };

        let first_prompt_text = initial_prompt.clone();
        let mut widget = Self {
            app_event_tx: app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            codex_op_tx: spawn_result.op_tx,
            bottom_pane: BottomPane::new(BottomPaneParams {
                frame_requester,
                app_event_tx,
                has_input_focus: true,
                enhanced_keys_supported,
                placeholder_text: placeholder,
                disable_paste_burst: config.disable_paste_burst,
                animations_enabled: config.animations,
                vertical_footer,
                agent_display_name: crate::nori::agent_picker::get_agent_info(&config.model)
                    .map(|info| info.display_name)
                    .unwrap_or_else(|| config.model.clone()),
            }),
            active_cell: None,
            config: config.clone(),
            auth_manager,
            session_header: SessionHeader::new(config.model),
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            token_info: None,
            rate_limit_snapshot: None,
            rate_limit_warnings: RateLimitWarningState::default(),

            rate_limit_poller: None,
            stream_controller: None,
            running_commands: HashMap::new(),
            suppressed_exec_calls: HashSet::new(),
            last_unified_wait: None,
            task_complete_pending: false,
            mcp_startup_status: None,
            interrupts: InterruptManager::new(),
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: crate::status_indicator_widget::random_status_message(),
            retry_status_header: None,
            conversation_id: None,
            queued_user_messages: VecDeque::new(),
            show_welcome_banner: true,
            suppress_session_configured_redraw: false,
            pending_notification: None,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            current_rollout_path: None,
            pending_exec_cells: PendingExecCellTracker::new(),
            effective_cwd_tracker: EffectiveCwdTracker::with_initial_cwd(config.cwd),
            pending_agent: None,
            expected_agent,
            session_configured_received: false,
            #[cfg(feature = "unstable")]
            acp_handle: spawn_result.acp_handle,
            session_stats: SessionStats::new(),
            login_handler: None,
            first_prompt_text,
            loop_remaining: None,
            loop_total: None,
            #[cfg(feature = "nori-config")]
            loop_count_override: None,
            turn_finished: false,
            plan_drawer_mode: PlanDrawerMode::Off,
            pinned_plan: None,
        };

        widget.prefetch_rate_limits();

        widget
    }

    /// Create a ChatWidget that resumes an ACP session via `session/load`
    /// or client-side replay when the agent doesn't support `session/load`.
    pub(crate) fn new_resumed_acp(
        common: ChatWidgetInit,
        acp_session_id: Option<String>,
        transcript: codex_acp::transcript::Transcript,
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
            expected_agent,
            deferred_spawn: _,
            fork_context: _,
        } = common;
        let mut rng = rand::rng();
        let placeholder = EXAMPLE_PROMPTS[rng.random_range(0..EXAMPLE_PROMPTS.len())].to_string();
        let spawn_result = spawn_acp_agent_resume(
            config.clone(),
            acp_session_id,
            transcript,
            app_event_tx.clone(),
        );

        let first_prompt_text = initial_prompt.clone();
        let mut widget = Self {
            app_event_tx: app_event_tx.clone(),
            frame_requester: frame_requester.clone(),
            codex_op_tx: spawn_result.op_tx,
            bottom_pane: BottomPane::new(BottomPaneParams {
                frame_requester,
                app_event_tx,
                has_input_focus: true,
                enhanced_keys_supported,
                placeholder_text: placeholder,
                disable_paste_burst: config.disable_paste_burst,
                animations_enabled: config.animations,
                vertical_footer,
                agent_display_name: crate::nori::agent_picker::get_agent_info(&config.model)
                    .map(|info| info.display_name)
                    .unwrap_or_else(|| config.model.clone()),
            }),
            active_cell: None,
            config: config.clone(),
            auth_manager,
            session_header: SessionHeader::new(config.model),
            initial_user_message: create_initial_user_message(
                initial_prompt.unwrap_or_default(),
                initial_images,
            ),
            token_info: None,
            rate_limit_snapshot: None,
            rate_limit_warnings: RateLimitWarningState::default(),

            rate_limit_poller: None,
            stream_controller: None,
            running_commands: HashMap::new(),
            suppressed_exec_calls: HashSet::new(),
            last_unified_wait: None,
            task_complete_pending: false,
            mcp_startup_status: None,
            interrupts: InterruptManager::new(),
            reasoning_buffer: String::new(),
            full_reasoning_buffer: String::new(),
            current_status_header: crate::status_indicator_widget::random_status_message(),
            retry_status_header: None,
            conversation_id: None,
            queued_user_messages: VecDeque::new(),
            show_welcome_banner: false,
            suppress_session_configured_redraw: false,
            pending_notification: None,
            needs_final_message_separator: false,
            last_rendered_width: std::cell::Cell::new(None),
            current_rollout_path: None,
            pending_exec_cells: PendingExecCellTracker::new(),
            effective_cwd_tracker: EffectiveCwdTracker::with_initial_cwd(config.cwd),
            pending_agent: None,
            expected_agent,
            session_configured_received: false,
            #[cfg(feature = "unstable")]
            acp_handle: spawn_result.acp_handle,
            session_stats: SessionStats::new(),
            login_handler: None,
            first_prompt_text,
            loop_remaining: None,
            loop_total: None,
            #[cfg(feature = "nori-config")]
            loop_count_override: None,
            turn_finished: false,
            plan_drawer_mode: PlanDrawerMode::Off,
            pinned_plan: None,
        };

        widget.prefetch_rate_limits();

        widget
    }

    /// Set a pending agent to switch to on the next prompt submission.
    pub(crate) fn set_pending_agent(&mut self, agent_name: String, display_name: String) {
        // Update the bottom pane's model display name for approval dialogs
        self.bottom_pane
            .set_agent_display_name(display_name.clone());
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
        self.codex_op_tx = spawn_result.op_tx;
        #[cfg(feature = "unstable")]
        {
            self.acp_handle = spawn_result.acp_handle;
        }
    }
}
