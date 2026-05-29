use super::*;

use nori_protocol::ApprovalSubject;
use nori_protocol::ClientEvent;
use nori_protocol::ClientEventNormalizer;
use nori_protocol::session_runtime::QueuedPrompt;
use nori_protocol::session_runtime::QueuedPromptKind;
use nori_protocol::session_runtime::SessionPhase;
use nori_protocol::session_runtime::SessionRuntime;

use super::session_reducer::InboundEvent;
use super::session_reducer::SideEffect;
use super::session_reducer::reduce;
use crate::transcript::ContentBlock;

pub(crate) struct SessionDriver {
    runtime: SessionRuntime,
    normalizer: ClientEventNormalizer,
}

pub(crate) struct CompletedTurn {
    pub prompt: QueuedPrompt,
    pub stop_reason: agent_client_protocol_schema::StopReason,
    pub last_agent_message: Option<String>,
}

pub(crate) enum SessionRuntimeInput {
    Reducer(InboundEvent),
    PermissionRequest {
        pending_request: Box<PendingApprovalRequest>,
        current_policy: AskForApproval,
    },
}

pub(crate) struct ReducerActions {
    pub events: Vec<ClientEvent>,
    pub side_effects: Vec<SideEffect>,
    pub completed_turn: Option<CompletedTurn>,
}

fn client_event_kind(event: &ClientEvent) -> &'static str {
    match event {
        ClientEvent::SessionUpdateInfo(_) => "session_update_info",
        ClientEvent::SessionConfigUpdate(_) => "session_config_update",
        ClientEvent::SessionModeChanged(_) => "session_mode_changed",
        ClientEvent::SessionPhaseChanged(_) => "session_phase_changed",
        ClientEvent::QueueChanged(_) => "queue_changed",
        ClientEvent::MessageDelta(_) => "message_delta",
        ClientEvent::PromptCompleted(_) => "prompt_completed",
        ClientEvent::ToolSnapshot(_) => "tool_snapshot",
        ClientEvent::ApprovalRequest(_) => "approval_request",
        ClientEvent::AgentCommandsUpdate(_) => "agent_commands_update",
        ClientEvent::PlanSnapshot(_) => "plan_snapshot",
        ClientEvent::LoadCompleted => "load_completed",
        ClientEvent::ContextCompacted(_) => "context_compacted",
        ClientEvent::Warning(_) => "warning",
        ClientEvent::ReplayEntry(_) => "replay_entry",
        ClientEvent::ThreadGoalUpdated(_) => "thread_goal_updated",
        ClientEvent::ThreadGoalCleared => "thread_goal_cleared",
    }
}

impl SessionDriver {
    pub(crate) fn new() -> Self {
        Self {
            runtime: SessionRuntime::new(),
            normalizer: ClientEventNormalizer::default(),
        }
    }

    pub(crate) fn apply(&mut self, event: InboundEvent) -> ReducerActions {
        let completed_prompt = matches!(
            event,
            InboundEvent::PromptResponse { .. } | InboundEvent::PromptFailed
        )
        .then(|| {
            self.runtime
                .active
                .as_ref()
                .and_then(|active| active.prompt.clone())
        })
        .flatten();

        let out = reduce(&mut self.runtime, event, &mut self.normalizer);
        let completed_turn = completed_prompt.and_then(|prompt| {
            out.events.iter().find_map(|event| match event {
                ClientEvent::PromptCompleted(completed) => Some(CompletedTurn {
                    prompt: prompt.clone(),
                    stop_reason: completed.stop_reason,
                    last_agent_message: completed.last_agent_message.clone(),
                }),
                _ => None,
            })
        });

        ReducerActions {
            events: out.events,
            side_effects: out.side_effects,
            completed_turn,
        }
    }

    pub(crate) fn active_request_id(&self) -> Option<String> {
        self.runtime
            .active
            .as_ref()
            .map(|active| active.request_id.clone())
    }

    pub(crate) fn phase_label(&self) -> &'static str {
        session_reducer::session_phase_label(&self.runtime.phase)
    }

    pub(crate) fn queue_len(&self) -> usize {
        self.runtime.queue.len()
    }

    pub(crate) fn push_permission_request(
        &mut self,
        request: &crate::connection::ApprovalRequest,
    ) -> Vec<ClientEvent> {
        self.normalizer
            .push_permission_request(&request.acp_request)
    }
}

impl Default for SessionDriver {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn patch_approval_request_owner(
    client_events: Vec<ClientEvent>,
    owner_request_id: Option<String>,
) -> Vec<ClientEvent> {
    client_events
        .into_iter()
        .map(|event| match event {
            ClientEvent::ApprovalRequest(mut approval) => {
                let ApprovalSubject::ToolSnapshot(snapshot) = &mut approval.subject;
                if snapshot.owner_request_id.is_none() {
                    snapshot.owner_request_id = owner_request_id.clone();
                }
                ClientEvent::ApprovalRequest(approval)
            }
            other => other,
        })
        .collect()
}

const CANCEL_WARNING_SECS: u64 = 3;
const CANCEL_FORCE_SECS: u64 = 10;

impl AcpBackend {
    pub(super) async fn apply_session_event(&self, event: InboundEvent) {
        let is_prompt_terminal = matches!(
            event,
            InboundEvent::PromptResponse { .. } | InboundEvent::PromptFailed
        );
        if is_prompt_terminal {
            if let Some(abort) = self.cancel_timeout_abort.lock().await.take() {
                abort.abort();
            }
            *self.prompt_task_abort.lock().await = None;
        }
        let event_kind = session_reducer::inbound_event_kind(&event);
        let actions = {
            let mut driver = self.session_driver.lock().await;
            let phase_before = driver.phase_label();
            let active_before = driver.active_request_id();
            let queue_len_before = driver.queue_len();
            let actions = driver.apply(event);
            debug!(
                target: "acp_event_flow",
                event_kind,
                phase_before,
                active_request_id_before = active_before.as_deref().unwrap_or("<none>"),
                queue_len_before,
                phase_after = driver.phase_label(),
                active_request_id_after = driver.active_request_id().as_deref().unwrap_or("<none>"),
                queue_len_after = driver.queue_len(),
                client_events = actions.events.len(),
                side_effects = actions.side_effects.len(),
                "Applied reducer event in serialized session runtime"
            );
            actions
        };
        self.dispatch_reducer_actions(actions).await;
        if is_prompt_terminal {
            self.maybe_start_idle_timer().await;
        }
    }

    pub(super) async fn handle_permission_request(
        &self,
        pending_request: Box<PendingApprovalRequest>,
        current_policy: AskForApproval,
    ) {
        let request_id = pending_request.request_id.clone();
        let call_id = pending_request.request.event.call_id().to_string();
        let (actions, permission_is_valid) = {
            let mut driver = self.session_driver.lock().await;
            let actions = driver.apply(InboundEvent::PermissionRequest {
                request_id,
                call_id: call_id.clone(),
            });
            let permission_is_valid = matches!(
                driver.runtime.phase,
                nori_protocol::session_runtime::SessionPhase::Prompt { .. }
            );
            (actions, permission_is_valid)
        };
        self.dispatch_reducer_actions(actions).await;

        if !permission_is_valid {
            let _ = pending_request
                .request
                .response_tx
                .send(ReviewDecision::Denied);
            return;
        }

        if current_policy == AskForApproval::Never {
            debug!(
                target: "acp_event_flow",
                call_id = %call_id,
                "Auto-approving request (approval_policy=Never)"
            );
            let _ = pending_request
                .request
                .response_tx
                .send(ReviewDecision::Approved);
            return;
        }

        let owner_request_id = {
            let driver = self.session_driver.lock().await;
            driver.active_request_id()
        };
        let client_events = {
            let mut driver = self.session_driver.lock().await;
            patch_approval_request_owner(
                driver.push_permission_request(&pending_request.request),
                owner_request_id,
            )
        };
        self.forward_and_record_client_events(&client_events).await;

        let (notification_call_id, command_for_notification) = match &pending_request.request.event
        {
            ApprovalEventType::Exec(exec_event) => {
                (exec_event.call_id.clone(), exec_event.command.join(" "))
            }
            ApprovalEventType::Patch(patch_event) => (
                patch_event.call_id.clone(),
                format!(
                    "patch: {}",
                    patch_event
                        .changes
                        .keys()
                        .map(|path| path.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
        };

        self.pending_approvals.lock().await.push(*pending_request);
        self.user_notifier
            .notify(&codex_core::UserNotification::AwaitingApproval {
                call_id: notification_call_id,
                command: command_for_notification,
                cwd: self.cwd.display().to_string(),
            });
    }

    async fn dispatch_reducer_actions(&self, actions: ReducerActions) {
        match actions.completed_turn.as_ref().map(|turn| turn.prompt.kind) {
            Some(QueuedPromptKind::Compact) => {
                let mut completion_event = None;
                let mut non_completion_events = Vec::new();
                for event in actions.events {
                    match event {
                        ClientEvent::PromptCompleted(_) => {
                            completion_event = Some(event);
                        }
                        other => non_completion_events.push(other),
                    }
                }
                self.forward_and_record_client_events(&non_completion_events)
                    .await;
                if let Some(completed_turn) = actions.completed_turn {
                    self.handle_completed_turn(&completed_turn).await;
                }
                if let Some(event) = completion_event {
                    self.forward_and_record_client_event(event).await;
                }
            }
            _ => {
                self.forward_and_record_client_events(&actions.events).await;
                if let Some(completed_turn) = actions.completed_turn {
                    self.handle_completed_turn(&completed_turn).await;
                }
            }
        }

        for side_effect in actions.side_effects {
            self.execute_side_effect(side_effect).await;
        }
    }

    async fn forward_and_record_client_events(&self, client_events: &[ClientEvent]) {
        for client_event in client_events {
            self.forward_and_record_client_event(client_event.clone())
                .await;
        }
    }

    async fn forward_and_record_client_event(&self, client_event: ClientEvent) {
        match &client_event {
            ClientEvent::SessionPhaseChanged(phase) => {
                debug!(
                    target: "acp_event_flow",
                    client_event = client_event_kind(&client_event),
                    ?phase,
                    "Forwarding client event from ACP backend"
                );
            }
            ClientEvent::PromptCompleted(completed) => {
                debug!(
                    target: "acp_event_flow",
                    client_event = client_event_kind(&client_event),
                    stop_reason = ?completed.stop_reason,
                    has_last_agent_message = completed
                        .last_agent_message
                        .as_ref()
                        .is_some_and(|message| !message.is_empty()),
                    "Forwarding client event from ACP backend"
                );
            }
            _ => {
                debug!(
                    target: "acp_event_flow",
                    client_event = client_event_kind(&client_event),
                    "Forwarding client event from ACP backend"
                );
            }
        }
        let goal_event = self
            .thread_goal_update_from_client_event(&client_event)
            .await;
        emit_client_event(
            &self.backend_event_tx,
            self.transcript_recorder.as_ref(),
            client_event,
        )
        .await;
        if let Some(goal_event) = goal_event {
            emit_client_event(
                &self.backend_event_tx,
                self.transcript_recorder.as_ref(),
                goal_event,
            )
            .await;
        }
    }

    async fn handle_completed_turn(&self, completed_turn: &CompletedTurn) {
        match completed_turn.prompt.kind {
            QueuedPromptKind::User | QueuedPromptKind::GoalContinuation => {
                if let Some(last_agent_message) = &completed_turn.last_agent_message
                    && let Some(ref recorder) = self.transcript_recorder
                {
                    let content = vec![ContentBlock::Text {
                        text: last_agent_message.clone(),
                    }];
                    if let Err(err) = recorder
                        .record_assistant_message(&completed_turn.prompt.event_id, content, None)
                        .await
                    {
                        warn!("Failed to record assistant message to transcript: {err}");
                    }
                }

                if let Some(last_agent_message) = &completed_turn.last_agent_message
                    && !last_agent_message.is_empty()
                    && !self.post_agent_response_hooks.is_empty()
                {
                    let env_vars = HashMap::from([
                        (
                            "NORI_HOOK_EVENT".to_string(),
                            "post_agent_response".to_string(),
                        ),
                        (
                            "NORI_HOOK_RESPONSE_TEXT".to_string(),
                            last_agent_message.clone(),
                        ),
                    ]);
                    let results = crate::hooks::execute_hooks_with_env(
                        &self.post_agent_response_hooks,
                        self.script_timeout,
                        &env_vars,
                    )
                    .await;
                    route_hook_results(
                        &results,
                        &self.event_tx,
                        &completed_turn.prompt.event_id,
                        None,
                    )
                    .await;
                }

                if let Some(last_agent_message) = &completed_turn.last_agent_message
                    && !last_agent_message.is_empty()
                    && !self.async_post_agent_response_hooks.is_empty()
                {
                    let env_vars = HashMap::from([
                        (
                            "NORI_HOOK_EVENT".to_string(),
                            "post_agent_response".to_string(),
                        ),
                        (
                            "NORI_HOOK_RESPONSE_TEXT".to_string(),
                            last_agent_message.clone(),
                        ),
                    ]);
                    let _ = crate::hooks::execute_hooks_fire_and_forget(
                        self.async_post_agent_response_hooks.clone(),
                        self.script_timeout,
                        env_vars,
                    );
                }

                if completed_turn.prompt.kind == QueuedPromptKind::User {
                    if let Some(display_text) = &completed_turn.prompt.display_text
                        && !self.post_user_prompt_hooks.is_empty()
                    {
                        let env_vars = HashMap::from([
                            (
                                "NORI_HOOK_EVENT".to_string(),
                                "post_user_prompt".to_string(),
                            ),
                            ("NORI_HOOK_PROMPT_TEXT".to_string(), display_text.clone()),
                        ]);
                        let results = crate::hooks::execute_hooks_with_env(
                            &self.post_user_prompt_hooks,
                            self.script_timeout,
                            &env_vars,
                        )
                        .await;
                        route_hook_results(
                            &results,
                            &self.event_tx,
                            &completed_turn.prompt.event_id,
                            Some(&self.pending_hook_context),
                        )
                        .await;
                    }

                    if let Some(display_text) = &completed_turn.prompt.display_text
                        && !self.async_post_user_prompt_hooks.is_empty()
                    {
                        let env_vars = HashMap::from([
                            (
                                "NORI_HOOK_EVENT".to_string(),
                                "post_user_prompt".to_string(),
                            ),
                            ("NORI_HOOK_PROMPT_TEXT".to_string(), display_text.clone()),
                        ]);
                        let _ = crate::hooks::execute_hooks_fire_and_forget(
                            self.async_post_user_prompt_hooks.clone(),
                            self.script_timeout,
                            env_vars,
                        );
                    }
                }

                self.maybe_submit_goal_continuation(completed_turn).await;
            }
            QueuedPromptKind::Compact => {
                let Some(summary) = completed_turn.last_agent_message.clone() else {
                    return;
                };
                *self.pending_compact_summary.lock().await = Some(summary.clone());

                let cwd = self.cwd.clone();
                let mut mcp_servers = crate::connection::mcp::to_sacp_mcp_servers(
                    &self.mcp_servers,
                    self.mcp_oauth_credentials_store_mode,
                );
                if let Err(err) = nori_client_mcp::register_for_session(
                    &self.connection,
                    &mut mcp_servers,
                    Arc::clone(&self.thread_goal_state),
                    self.backend_event_tx.clone(),
                    Arc::clone(&self.transcript_recorder_cell),
                    Arc::clone(&self.goal_mcp_connected),
                    Arc::clone(&self.goal_mcp_http_server),
                )
                .await
                {
                    warn!("Failed to register goal MCP server after compact: {err}");
                }
                match self.connection.create_session(&cwd, mcp_servers).await {
                    Ok(new_session_id) => {
                        debug!("Created new session after compact: {:?}", new_session_id);
                        *self.session_id.write().await = new_session_id;
                    }
                    Err(err) => {
                        warn!("Failed to create new session after compact: {err}");
                    }
                }

                self.forward_and_record_client_event(ClientEvent::ContextCompacted(
                    nori_protocol::ContextCompacted {
                        summary: Some(summary),
                    },
                ))
                .await;

                let _ = self
                    .event_tx
                    .send(Event {
                        id: completed_turn.prompt.event_id.clone(),
                        msg: EventMsg::Warning(WarningEvent {
                            message: "Heads up: Long conversations and multiple compactions can cause the model to be less accurate. Start a new conversation when possible to keep conversations small and targeted.".to_string(),
                        }),
                    })
                    .await;
            }
        }
    }

    async fn maybe_submit_goal_continuation(&self, completed_turn: &CompletedTurn) {
        if completed_turn.stop_reason != agent_client_protocol_schema::StopReason::EndTurn {
            return;
        }

        // Safety invariant: a hidden continuation may only chain after another
        // hidden continuation once we have observed the agent actually connect to
        // the `nori-client` MCP endpoint (`goal.connected`). Until then the agent
        // has no way to mark the goal complete/blocked, so unbounded
        // continuation→continuation chaining would loop forever. A user turn may
        // always start one continuation; only continuation→continuation chaining
        // requires the completion tool to be reachable.
        let can_chain_continuation = self
            .goal_mcp_connected
            .load(std::sync::atomic::Ordering::Relaxed);
        match completed_turn.prompt.kind {
            QueuedPromptKind::User => {}
            QueuedPromptKind::GoalContinuation if can_chain_continuation => {}
            QueuedPromptKind::GoalContinuation | QueuedPromptKind::Compact => return,
        }

        self.submit_goal_continuation_if_idle().await;
    }

    pub(super) async fn submit_goal_continuation_if_idle(&self) {
        let prompt_text = {
            self.thread_goal_state
                .lock()
                .await
                .continuation_prompt(thread_goal::now_seconds())
        };
        let Some(prompt_text) = prompt_text else {
            return;
        };

        let can_start_continuation = {
            let driver = self.session_driver.lock().await;
            matches!(driver.runtime.phase, SessionPhase::Idle) && driver.runtime.queue.is_empty()
        };
        if !can_start_continuation {
            debug!(
                target: "acp_event_flow",
                "Skipping goal continuation because the ACP runtime is not idle"
            );
            return;
        }

        let _ = self
            .session_event_tx
            .send(SessionRuntimeInput::Reducer(
                session_reducer::InboundEvent::PromptSubmit(
                    nori_protocol::session_runtime::QueuedPrompt {
                        event_id: format!("goal-continuation-{}", uuid::Uuid::new_v4()),
                        kind: nori_protocol::session_runtime::QueuedPromptKind::GoalContinuation,
                        text: prompt_text,
                        display_text: None,
                        images: Vec::new(),
                    },
                ),
            ))
            .await;
    }

    async fn execute_side_effect(&self, side_effect: SideEffect) {
        match side_effect {
            SideEffect::SendPrompt { request_id, prompt } => {
                if let Some(abort_handle) = self.idle_timer_abort.lock().await.take() {
                    abort_handle.abort();
                }

                let prompt_kind = {
                    let driver = self.session_driver.lock().await;
                    driver.active_request_id().and_then(|_| {
                        driver
                            .runtime
                            .active
                            .as_ref()
                            .and_then(|active| active.prompt.as_ref().map(|prompt| prompt.kind))
                    })
                };
                let backend = (*self).clone();
                let prompt_result_tx = self.prompt_result_tx.clone();
                let request_id_for_task = request_id.clone();
                let prompt_task = tokio::spawn(async move {
                    let session_id = backend.session_id.read().await.clone();
                    let prompt_kind = prompt_kind.unwrap_or(QueuedPromptKind::User);
                    debug!(
                        target: "acp_event_flow",
                        request_id = %request_id_for_task,
                        session_id = %session_id,
                        ?prompt_kind,
                        content_blocks = prompt.len(),
                        "Sending ACP session/prompt request"
                    );
                    let result = backend.connection.prompt(session_id, prompt).await;
                    match result {
                        Ok(stop_reason) => {
                            debug!(
                                target: "acp_event_flow",
                                request_id = %request_id_for_task,
                                ?stop_reason,
                                "Prompt task received ACP session/prompt response"
                            );
                            let _ = prompt_result_tx
                                .send(InboundEvent::PromptResponse { stop_reason })
                                .await;
                        }
                        Err(err) => {
                            warn!(
                                target: "acp_event_flow",
                                request_id = %request_id_for_task,
                                error = %err,
                                "Prompt task failed before reducer observed a prompt response"
                            );
                            backend.send_prompt_error(prompt_kind, &err).await;
                            let _ = prompt_result_tx.send(InboundEvent::PromptFailed).await;
                        }
                    }
                });
                *self.prompt_task_abort.lock().await = Some(prompt_task.abort_handle());
            }
            SideEffect::SendCancel => {
                let session_id = self.session_id.read().await.clone();
                debug!(
                    target: "acp_event_flow",
                    session_id = %session_id,
                    "Sending ACP session/cancel notification"
                );
                if let Err(err) = self.connection.cancel(&session_id).await {
                    warn!("Failed to cancel ACP session: {err}");
                }

                let event_tx = self.event_tx.clone();
                let prompt_task_abort = Arc::clone(&self.prompt_task_abort);
                let prompt_result_tx = self.prompt_result_tx.clone();
                let watchdog = tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(CANCEL_WARNING_SECS)).await;
                    warn!("Agent has not responded to cancel after {CANCEL_WARNING_SECS}s");
                    let _ = event_tx
                        .send(Event {
                            id: String::new(),
                            msg: EventMsg::Warning(WarningEvent {
                                message: format!(
                                    "Agent is slow to cancel. Will force-cancel in {}s…",
                                    CANCEL_FORCE_SECS - CANCEL_WARNING_SECS
                                ),
                            }),
                        })
                        .await;

                    tokio::time::sleep(std::time::Duration::from_secs(
                        CANCEL_FORCE_SECS - CANCEL_WARNING_SECS,
                    ))
                    .await;
                    if let Some(abort) = prompt_task_abort.lock().await.take() {
                        warn!("Force-cancelling prompt task after {CANCEL_FORCE_SECS}s timeout");
                        abort.abort();
                        let _ = prompt_result_tx.send(InboundEvent::PromptFailed).await;
                    }
                });
                *self.cancel_timeout_abort.lock().await = Some(watchdog.abort_handle());
            }
            SideEffect::ResolvePermissionCancelled { request_id } => {
                self.resolve_cancelled_permission(&request_id).await;
            }
        }
    }

    async fn send_prompt_error(&self, prompt_kind: QueuedPromptKind, err: &anyhow::Error) {
        let message = match prompt_kind {
            QueuedPromptKind::Compact => format!("Compact failed: {err}"),
            QueuedPromptKind::GoalContinuation => format!("Goal continuation failed: {err}"),
            QueuedPromptKind::User => {
                let error_string = format!("{err:?}");
                let category = categorize_acp_error(&error_string);
                let display_error = format!("{err:#}");
                match category {
                    AcpErrorCategory::Authentication => {
                        format!(
                            "Authentication error: {display_error}. Please check your credentials or re-authenticate."
                        )
                    }
                    AcpErrorCategory::QuotaExceeded => {
                        format!("Rate limit or quota exceeded: {display_error}")
                    }
                    AcpErrorCategory::ExecutableNotFound => {
                        format!("Agent executable not found: {display_error}")
                    }
                    AcpErrorCategory::Initialization => {
                        format!("Agent initialization failed: {display_error}")
                    }
                    AcpErrorCategory::PromptTooLong => {
                        "Prompt is too long. Try using /compact to reduce context size, or start a new session."
                            .to_string()
                    }
                    AcpErrorCategory::ApiServerError => {
                        "The API returned a server error. This is usually temporary — please try again."
                            .to_string()
                    }
                    AcpErrorCategory::Unknown => format!("ACP prompt failed: {display_error}"),
                }
            }
        };

        let _ = self
            .event_tx
            .send(Event {
                id: String::new(),
                msg: EventMsg::Error(ErrorEvent {
                    message,
                    codex_error_info: None,
                }),
            })
            .await;
    }

    async fn resolve_cancelled_permission(&self, request_id: &str) {
        let mut pending = self.pending_approvals.lock().await;
        if let Some(position) = pending
            .iter()
            .position(|pending_request| pending_request.request_id == request_id)
        {
            let pending_request = pending.remove(position);
            drop(pending_request);
        }
    }

    async fn maybe_start_idle_timer(&self) {
        let is_idle = {
            let driver = self.session_driver.lock().await;
            driver.active_request_id().is_none()
        };
        if !is_idle {
            return;
        }

        let Some(duration) = self.notify_after_idle.as_duration() else {
            return;
        };

        let idle_secs = duration.as_secs();
        let user_notifier = Arc::clone(&self.user_notifier);
        let session_id = self.session_id.read().await.to_string();
        let idle_task = tokio::spawn(async move {
            tokio::time::sleep(duration).await;
            user_notifier.notify(&codex_core::UserNotification::Idle {
                session_id,
                idle_duration_secs: idle_secs,
            });
        });
        *self.idle_timer_abort.lock().await = Some(idle_task.abort_handle());
    }
}
