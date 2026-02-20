use super::*;

impl Session {
    pub(super) fn make_turn_context(
        auth_manager: Option<Arc<AuthManager>>,
        otel_event_manager: &OtelEventManager,
        provider: ModelProviderInfo,
        session_configuration: &SessionConfiguration,
        conversation_id: ConversationId,
        sub_id: String,
    ) -> TurnContext {
        let config = session_configuration.original_config_do_not_use.clone();
        let model_family = find_family_for_model(&session_configuration.model)
            .unwrap_or_else(|| config.model_family.clone());
        let mut per_turn_config = (*config).clone();
        per_turn_config.model = session_configuration.model.clone();
        per_turn_config.model_family = model_family.clone();
        per_turn_config.model_reasoning_effort = session_configuration.model_reasoning_effort;
        per_turn_config.model_reasoning_summary = session_configuration.model_reasoning_summary;
        if let Some(model_info) = get_model_info(&model_family) {
            per_turn_config.model_context_window = Some(model_info.context_window);
        }

        let otel_event_manager = otel_event_manager.clone().with_model(
            session_configuration.model.as_str(),
            session_configuration.model.as_str(),
        );

        let client = ModelClient::new(
            Arc::new(per_turn_config.clone()),
            auth_manager,
            otel_event_manager,
            provider,
            session_configuration.model_reasoning_effort,
            session_configuration.model_reasoning_summary,
            conversation_id,
            session_configuration.session_source.clone(),
        );

        let tools_config = ToolsConfig::new(&ToolsConfigParams {
            model_family: &model_family,
            features: &config.features,
        });

        TurnContext {
            sub_id,
            client,
            cwd: session_configuration.cwd.clone(),
            developer_instructions: session_configuration.developer_instructions.clone(),
            base_instructions: session_configuration.base_instructions.clone(),
            compact_prompt: session_configuration.compact_prompt.clone(),
            user_instructions: session_configuration.user_instructions.clone(),
            approval_policy: session_configuration.approval_policy,
            sandbox_policy: session_configuration.sandbox_policy.clone(),
            shell_environment_policy: config.shell_environment_policy.clone(),
            tools_config,
            final_output_json_schema: None,
            codex_linux_sandbox_exe: config.codex_linux_sandbox_exe.clone(),
            tool_call_gate: Arc::new(ReadinessFlag::new()),
            exec_policy: session_configuration.exec_policy.clone(),
            truncation_policy: TruncationPolicy::new(&per_turn_config),
        }
    }

    pub(super) async fn new(
        session_configuration: SessionConfiguration,
        config: Arc<Config>,
        auth_manager: Arc<AuthManager>,
        tx_event: Sender<Event>,
        initial_history: InitialHistory,
        session_source: SessionSource,
    ) -> anyhow::Result<Arc<Self>> {
        debug!(
            "Configuring session: model={}; provider={:?}",
            session_configuration.model, session_configuration.provider
        );
        if !session_configuration.cwd.is_absolute() {
            return Err(anyhow::anyhow!(
                "cwd is not absolute: {:?}",
                session_configuration.cwd
            ));
        }

        let (conversation_id, rollout_params) = match &initial_history {
            InitialHistory::New | InitialHistory::Forked(_) => {
                let conversation_id = ConversationId::default();
                (
                    conversation_id,
                    RolloutRecorderParams::new(
                        conversation_id,
                        session_configuration.user_instructions.clone(),
                        session_source,
                    ),
                )
            }
            InitialHistory::Resumed(resumed_history) => (
                resumed_history.conversation_id,
                RolloutRecorderParams::resume(resumed_history.rollout_path.clone()),
            ),
        };

        // Kick off independent async setup tasks in parallel to reduce startup latency.
        //
        // - initialize RolloutRecorder with new or resumed session info
        // - perform default shell discovery
        // - load history metadata
        let rollout_fut = RolloutRecorder::new(&config, rollout_params);

        let default_shell = shell::default_user_shell();
        let history_meta_fut = crate::message_history::history_metadata(&config);
        let auth_statuses_fut = compute_auth_statuses(
            config.mcp_servers.iter(),
            config.mcp_oauth_credentials_store_mode,
        );

        // Join all independent futures.
        let (rollout_recorder, (history_log_id, history_entry_count), auth_statuses) =
            tokio::join!(rollout_fut, history_meta_fut, auth_statuses_fut);

        let rollout_recorder = rollout_recorder.map_err(|e| {
            error!("failed to initialize rollout recorder: {e:#}");
            anyhow::Error::from(e)
        })?;
        let rollout_path = rollout_recorder.rollout_path.clone();

        let mut post_session_configured_events = Vec::<Event>::new();

        for (alias, feature) in session_configuration.features.legacy_feature_usages() {
            let canonical = feature.key();
            let summary = format!("`{alias}` is deprecated. Use `{canonical}` instead.");
            let details = if alias == canonical {
                None
            } else {
                Some(format!(
                    "Enable it with `--enable {canonical}` or `[features].{canonical}` in config.toml. See https://github.com/openai/codex/blob/main/docs/config.md#feature-flags for details."
                ))
            };
            post_session_configured_events.push(Event {
                id: INITIAL_SUBMIT_ID.to_owned(),
                msg: EventMsg::DeprecationNotice(DeprecationNoticeEvent { summary, details }),
            });
        }

        let otel_event_manager = OtelEventManager::new(
            conversation_id,
            config.model.as_str(),
            config.model_family.slug.as_str(),
            auth_manager.auth().and_then(|a| a.get_account_id()),
            auth_manager.auth().and_then(|a| a.get_account_email()),
            auth_manager.auth().map(|a| a.mode),
            config.otel.log_user_prompt,
            terminal::user_agent(),
        );

        otel_event_manager.conversation_starts(
            config.model_provider.name.as_str(),
            config.model_reasoning_effort,
            config.model_reasoning_summary,
            config.model_context_window,
            config.model_auto_compact_token_limit,
            config.approval_policy,
            config.sandbox_policy.clone(),
            config.mcp_servers.keys().map(String::as_str).collect(),
            config.active_profile.clone(),
        );

        // Create the mutable state for the Session.
        let state = SessionState::new(session_configuration.clone());

        let services = SessionServices {
            mcp_connection_manager: Arc::new(RwLock::new(McpConnectionManager::default())),
            mcp_startup_cancellation_token: CancellationToken::new(),
            unified_exec_manager: UnifiedExecSessionManager::default(),
            notifier: UserNotifier::new(config.notify.clone(), true),
            rollout: Mutex::new(Some(rollout_recorder)),
            user_shell: default_shell,
            show_raw_agent_reasoning: config.show_raw_agent_reasoning,
            auth_manager: Arc::clone(&auth_manager),
            otel_event_manager,
            tool_approvals: Mutex::new(ApprovalStore::default()),
        };

        let sess = Arc::new(Session {
            conversation_id,
            tx_event: tx_event.clone(),
            state: Mutex::new(state),
            active_turn: Mutex::new(None),
            services,
            next_internal_sub_id: AtomicU64::new(0),
        });

        // Dispatch the SessionConfiguredEvent first and then report any errors.
        // If resuming, include converted initial messages in the payload so UIs can render them immediately.
        let initial_messages = initial_history.get_event_msgs();

        let events = std::iter::once(Event {
            id: INITIAL_SUBMIT_ID.to_owned(),
            msg: EventMsg::SessionConfigured(SessionConfiguredEvent {
                session_id: conversation_id,
                model: session_configuration.model.clone(),
                model_provider_id: config.model_provider_id.clone(),
                approval_policy: session_configuration.approval_policy,
                sandbox_policy: session_configuration.sandbox_policy.clone(),
                cwd: session_configuration.cwd.clone(),
                reasoning_effort: session_configuration.model_reasoning_effort,
                history_log_id,
                history_entry_count,
                initial_messages,
                rollout_path,
            }),
        })
        .chain(post_session_configured_events.into_iter());
        for event in events {
            sess.send_event_raw(event).await;
        }
        sess.services
            .mcp_connection_manager
            .write()
            .await
            .initialize(
                config.mcp_servers.clone(),
                config.mcp_oauth_credentials_store_mode,
                auth_statuses.clone(),
                tx_event.clone(),
                sess.services.mcp_startup_cancellation_token.clone(),
            )
            .await;

        let sandbox_state = SandboxState {
            sandbox_policy: session_configuration.sandbox_policy.clone(),
            codex_linux_sandbox_exe: config.codex_linux_sandbox_exe.clone(),
            sandbox_cwd: session_configuration.cwd.clone(),
        };
        if let Err(e) = sess
            .services
            .mcp_connection_manager
            .read()
            .await
            .notify_sandbox_state_change(&sandbox_state)
            .await
        {
            tracing::error!("Failed to notify sandbox state change: {e}");
        }

        // record_initial_history can emit events. We record only after the SessionConfiguredEvent is emitted.
        sess.record_initial_history(initial_history).await;

        Ok(sess)
    }

    pub(crate) fn get_tx_event(&self) -> Sender<Event> {
        self.tx_event.clone()
    }

    /// Ensure all rollout writes are durably flushed.
    pub(crate) async fn flush_rollout(&self) {
        let recorder = {
            let guard = self.services.rollout.lock().await;
            guard.clone()
        };
        if let Some(rec) = recorder
            && let Err(e) = rec.flush().await
        {
            warn!("failed to flush rollout recorder: {e}");
        }
    }

    pub(super) fn next_internal_sub_id(&self) -> String {
        let id = self
            .next_internal_sub_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        format!("auto-compact-{id}")
    }

    pub(super) async fn get_total_token_usage(&self) -> i64 {
        let state = self.state.lock().await;
        state.get_total_token_usage()
    }

    pub(super) async fn record_initial_history(&self, conversation_history: InitialHistory) {
        let turn_context = self.new_turn(SessionSettingsUpdate::default()).await;
        match conversation_history {
            InitialHistory::New => {
                // Build and record initial items (user instructions + environment context)
                let items = self.build_initial_context(&turn_context);
                self.record_conversation_items(&turn_context, &items).await;
                // Ensure initial items are visible to immediate readers (e.g., tests, forks).
                self.flush_rollout().await;
            }
            InitialHistory::Resumed(_) | InitialHistory::Forked(_) => {
                let rollout_items = conversation_history.get_rollout_items();
                let persist = matches!(conversation_history, InitialHistory::Forked(_));

                // If resuming, warn when the last recorded model differs from the current one.
                if let InitialHistory::Resumed(_) = conversation_history
                    && let Some(prev) = rollout_items.iter().rev().find_map(|it| {
                        if let RolloutItem::TurnContext(ctx) = it {
                            Some(ctx.model.as_str())
                        } else {
                            None
                        }
                    })
                {
                    let curr = turn_context.client.get_model();
                    if prev != curr {
                        warn!(
                            "resuming session with different model: previous={prev}, current={curr}"
                        );
                        self.send_event(
                                &turn_context,
                                EventMsg::Warning(WarningEvent {
                                    message: format!(
                                        "This session was recorded with model `{prev}` but is resuming with `{curr}`. \
                         Consider switching back to `{prev}` as it may affect Codex performance."
                                    ),
                                }),
                            )
                                .await;
                    }
                }

                // Always add response items to conversation history
                let reconstructed_history =
                    self.reconstruct_history_from_rollout(&turn_context, &rollout_items);
                if !reconstructed_history.is_empty() {
                    self.record_into_history(&reconstructed_history, &turn_context)
                        .await;
                }

                // If persisting, persist all rollout items as-is (recorder filters)
                if persist && !rollout_items.is_empty() {
                    self.persist_rollout_items(&rollout_items).await;
                }
                // Flush after seeding history and any persisted rollout copy.
                self.flush_rollout().await;
            }
        }
    }

    pub(crate) async fn update_settings(&self, updates: SessionSettingsUpdate) {
        let mut state = self.state.lock().await;

        state.session_configuration = state.session_configuration.apply(&updates);
    }

    pub(crate) async fn new_turn(&self, updates: SessionSettingsUpdate) -> Arc<TurnContext> {
        let sub_id = self.next_internal_sub_id();
        self.new_turn_with_sub_id(sub_id, updates).await
    }

    pub(crate) async fn new_turn_with_sub_id(
        &self,
        sub_id: String,
        updates: SessionSettingsUpdate,
    ) -> Arc<TurnContext> {
        let session_configuration = {
            let mut state = self.state.lock().await;
            let session_configuration = state.session_configuration.clone().apply(&updates);
            state.session_configuration = session_configuration.clone();
            session_configuration
        };

        let mut turn_context: TurnContext = Self::make_turn_context(
            Some(Arc::clone(&self.services.auth_manager)),
            &self.services.otel_event_manager,
            session_configuration.provider.clone(),
            &session_configuration,
            self.conversation_id,
            sub_id,
        );
        if let Some(final_schema) = updates.final_output_json_schema {
            turn_context.final_output_json_schema = final_schema;
        }
        Arc::new(turn_context)
    }

    pub(super) fn build_environment_update_item(
        &self,
        previous: Option<&Arc<TurnContext>>,
        next: &TurnContext,
    ) -> Option<ResponseItem> {
        let prev = previous?;

        let prev_context = EnvironmentContext::from(prev.as_ref());
        let next_context = EnvironmentContext::from(next);
        if prev_context.equals_except_shell(&next_context) {
            return None;
        }
        Some(ResponseItem::from(EnvironmentContext::diff(
            prev.as_ref(),
            next,
        )))
    }
}
