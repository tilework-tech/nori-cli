use super::*;

use crate::normalized::ClientEvent;
use crate::normalized::ClientEventNormalizer;
use crate::normalized::session_runtime::QueuedPrompt;
use crate::normalized::session_runtime::QueuedPromptKind;
use crate::normalized::session_runtime::SessionPhase;
use crate::normalized::session_runtime::SessionRuntime;

use super::session_reducer::InboundEvent;
use super::session_reducer::SideEffect;
use super::session_reducer::reduce;

pub(crate) struct SessionDriver {
    runtime: SessionRuntime,
    normalizer: ClientEventNormalizer,
}

pub(crate) struct CompletedTurn {
    pub prompt: QueuedPrompt,
    pub stop_reason: nori_protocol::acp::v1::StopReason,
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
        ClientEvent::SessionCapabilitiesChanged(_) => "session_capabilities_changed",
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
            InboundEvent::PromptResponse { .. } | InboundEvent::PromptFailed { .. }
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
            .map(|active| active.request_id.to_string())
    }

    pub(crate) fn phase_label(&self) -> &'static str {
        session_reducer::session_phase_label(&self.runtime.phase)
    }

    pub(crate) fn queue_len(&self) -> usize {
        self.runtime.queue.len()
    }

    pub(crate) fn public_phase(&self) -> nori_protocol::SessionPhase {
        match &self.runtime.phase {
            SessionPhase::Idle => nori_protocol::SessionPhase::Idle,
            SessionPhase::Loading { request_id } => nori_protocol::SessionPhase::Loading {
                request_id: request_id.clone(),
            },
            SessionPhase::Prompt {
                request_id,
                cancelling: false,
            } => nori_protocol::SessionPhase::Prompting {
                request_id: request_id.clone(),
            },
            SessionPhase::Prompt {
                request_id,
                cancelling: true,
            } => nori_protocol::SessionPhase::Cancelling {
                request_id: request_id.clone(),
            },
        }
    }
}

impl Default for SessionDriver {
    fn default() -> Self {
        Self::new()
    }
}

const CANCEL_WARNING_SECS: u64 = 3;
const CANCEL_FORCE_SECS: u64 = 10;

impl AcpBackend {
    pub(super) async fn apply_session_event(&self, event: InboundEvent) {
        let is_prompt_terminal = matches!(
            event,
            InboundEvent::PromptResponse { .. } | InboundEvent::PromptFailed { .. }
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
        let acp::AgentRequest::RequestPermissionRequest(permission) =
            &pending_request.request.request
        else {
            let _ = pending_request
                .request
                .response_tx
                .send(Err(acp::Error::method_not_found()));
            return;
        };
        let request_id = pending_request.request.request_id.to_string();
        let call_id = permission.tool_call.tool_call_id.to_string();
        let (actions, permission_is_valid) = {
            let mut driver = self.session_driver.lock().await;
            let actions = driver.apply(InboundEvent::PermissionRequest {
                request_id,
                call_id: call_id.clone(),
            });
            let permission_is_valid = matches!(
                driver.runtime.phase,
                crate::normalized::session_runtime::SessionPhase::Prompt { .. }
            );
            (actions, permission_is_valid)
        };
        self.dispatch_reducer_actions(actions).await;

        if !permission_is_valid {
            let _ = pending_request.request.response_tx.send(Ok(
                acp::ClientResponse::RequestPermissionResponse(
                    acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled),
                ),
            ));
            return;
        }

        if current_policy == AskForApproval::Never {
            debug!(
                target: "acp_event_flow",
                call_id = %call_id,
                "Auto-approving request (approval_policy=Never)"
            );
            let outcome = permission
                .options
                .iter()
                .find(|option| {
                    matches!(
                        option.kind,
                        acp::PermissionOptionKind::AllowOnce
                            | acp::PermissionOptionKind::AllowAlways
                    )
                })
                .map(|option| {
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        option.option_id.clone(),
                    ))
                })
                .unwrap_or(acp::RequestPermissionOutcome::Cancelled);
            let _ = pending_request.request.response_tx.send(Ok(
                acp::ClientResponse::RequestPermissionResponse(
                    acp::RequestPermissionResponse::new(outcome),
                ),
            ));
            return;
        }

        let _ = self
            .backend_event_tx
            .send(BackendEvent::Public(nori_protocol::SessionEvent::Acp(
                nori_protocol::AcpEvent::Request {
                    request_id: pending_request.request.request_id.clone(),
                    request: pending_request.request.request.clone(),
                },
            )))
            .await;

        let command_for_notification = permission
            .tool_call
            .fields
            .title
            .clone()
            .unwrap_or_else(|| "tool request".to_string());
        self.pending_approvals.lock().await.push(*pending_request);
        self.user_notifier
            .notify(&crate::UserNotification::AwaitingApproval {
                call_id,
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
                self.forward_client_events(&non_completion_events).await;
                if let Some(completed_turn) = actions.completed_turn {
                    self.handle_completed_turn(&completed_turn).await;
                }
                if let Some(event) = completion_event {
                    self.forward_client_event(event).await;
                }
            }
            _ => {
                self.forward_client_events(&actions.events).await;
                if let Some(completed_turn) = actions.completed_turn {
                    self.handle_completed_turn(&completed_turn).await;
                }
            }
        }

        for side_effect in actions.side_effects {
            self.execute_side_effect(side_effect).await;
        }
    }

    async fn forward_client_events(&self, client_events: &[ClientEvent]) {
        for client_event in client_events {
            self.forward_client_event(client_event.clone()).await;
        }
    }

    async fn forward_client_event(&self, client_event: ClientEvent) {
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
        let public_event = match &client_event {
            ClientEvent::SessionPhaseChanged(_) => {
                Some(nori_protocol::NoriEvent::SessionPhaseChanged(
                    self.session_driver.lock().await.public_phase(),
                ))
            }
            ClientEvent::QueueChanged(queue) => Some(nori_protocol::NoriEvent::QueueChanged(
                nori_protocol::QueueSnapshot {
                    prompts: queue.prompts.clone(),
                },
            )),
            ClientEvent::ContextCompacted(compacted) => Some(
                nori_protocol::NoriEvent::ContextCompacted(nori_protocol::ContextCompactedEvent {
                    summary: compacted.summary.clone(),
                }),
            ),
            ClientEvent::SessionCapabilitiesChanged(capabilities) => {
                Some(nori_protocol::NoriEvent::CapabilitiesChanged(
                    public_nori_capabilities(capabilities.clone()),
                ))
            }
            ClientEvent::ThreadGoalUpdated(update) => Some(nori_protocol::NoriEvent::GoalChanged(
                Some(update.goal.clone()),
            )),
            ClientEvent::ThreadGoalCleared => Some(nori_protocol::NoriEvent::GoalChanged(None)),
            ClientEvent::Warning(warning) => {
                Some(nori_protocol::NoriEvent::Notice(nori_protocol::Notice {
                    message: warning.message.clone(),
                }))
            }
            _ => None,
        };
        if let Some(event) = public_event {
            let _ = self
                .backend_event_tx
                .send(BackendEvent::Public(SessionEvent::Nori(event)))
                .await;
        }
        if let Some(goal_event) = goal_event
            && let ClientEvent::ThreadGoalUpdated(update) = goal_event
        {
            let _ = self
                .backend_event_tx
                .send(BackendEvent::Public(SessionEvent::Nori(
                    nori_protocol::NoriEvent::GoalChanged(Some(update.goal)),
                )))
                .await;
        }
    }

    async fn handle_completed_turn(&self, completed_turn: &CompletedTurn) {
        match completed_turn.prompt.kind {
            QueuedPromptKind::User | QueuedPromptKind::GoalContinuation => {
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
                    route_hook_results(&results, &self.backend_event_tx, None).await;
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
                            &self.backend_event_tx,
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
                let mut mcp_servers = crate::connection::mcp::to_acp_mcp_servers(
                    &self.mcp_servers,
                    self.mcp_oauth_credentials_store_mode,
                );
                let previous_goal_mcp_connected = {
                    let goal_mcp_http_server = self.goal_mcp_http_server.lock().await;
                    if let Some(server) = goal_mcp_http_server.as_ref() {
                        mcp_servers.push(server.as_mcp_server());
                        Some(
                            self.goal_mcp_connected
                                .swap(false, std::sync::atomic::Ordering::Relaxed),
                        )
                    } else {
                        None
                    }
                };
                let pending_nori_client_server = if previous_goal_mcp_connected.is_some() {
                    None
                } else {
                    match nori_client_mcp::register_for_session(
                        &self.connection,
                        &mut mcp_servers,
                        Arc::clone(&self.thread_goal_state),
                        self.backend_event_tx.clone(),
                        Arc::clone(&self.goal_mcp_connected),
                    )
                    .await
                    {
                        Ok(server) => server,
                        Err(err) => {
                            warn!("Failed to register goal MCP server after compact: {err}");
                            None
                        }
                    }
                };
                let nori_client_advertised = mcp_servers.iter().any(|server| {
                    matches!(
                        server,
                        acp::McpServer::Http(http) if http.name == "nori-client"
                    )
                });
                match self.connection.create_session(&cwd, mcp_servers).await {
                    Ok(new_session_id) => {
                        if let Some(server) = pending_nori_client_server {
                            server.commit(&self.goal_mcp_http_server).await;
                        }
                        debug!("Created new session after compact: {:?}", new_session_id);
                        *self.session_id.write().await = new_session_id;
                        self.forward_client_event(ClientEvent::SessionCapabilitiesChanged(
                            nori_client_mcp::capabilities_update_for_nori_client(
                                &self.connection,
                                nori_client_advertised,
                                self.goal_mcp_connected
                                    .load(std::sync::atomic::Ordering::Relaxed),
                            ),
                        ))
                        .await;
                    }
                    Err(err) => {
                        if let Some(previous) = previous_goal_mcp_connected {
                            self.goal_mcp_connected
                                .store(previous, std::sync::atomic::Ordering::Relaxed);
                        }
                        warn!("Failed to create new session after compact: {err}");
                    }
                }

                self.forward_client_event(ClientEvent::ContextCompacted(
                    crate::normalized::ContextCompacted {
                        summary: Some(summary),
                    },
                ))
                .await;

                let _ = self
                    .backend_event_tx
                    .send(BackendEvent::Public(SessionEvent::Nori(
                        nori_protocol::NoriEvent::Notice(nori_protocol::Notice {
                            message: "Heads up: Long conversations and multiple compactions can cause the model to be less accurate. Start a new conversation when possible to keep conversations small and targeted.".to_string(),
                        }),
                    )))
                    .await;
            }
        }
    }

    async fn maybe_submit_goal_continuation(&self, completed_turn: &CompletedTurn) {
        if completed_turn.stop_reason != nori_protocol::acp::v1::StopReason::EndTurn {
            return;
        }

        // Safety invariant: a hidden continuation may only chain after another
        // hidden continuation once we have observed the agent actually connect to
        // the `nori-client` MCP endpoint (`goal.connected`). Until then the agent
        // has no way to mark the goal complete/blocked, so unbounded
        // continuation-to-continuation chaining would loop forever. A user turn may
        // always start one continuation; only continuation-to-continuation chaining
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
        if self.goal_mcp_http_server.lock().await.is_none() {
            return;
        }

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
                    crate::normalized::session_runtime::QueuedPrompt {
                        event_id: format!("goal-continuation-{}", uuid::Uuid::new_v4()),
                        kind:
                            crate::normalized::session_runtime::QueuedPromptKind::GoalContinuation,
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

                let (prompt_kind, prompt_event_id) = {
                    let driver = self.session_driver.lock().await;
                    driver
                        .runtime
                        .active
                        .as_ref()
                        .and_then(|active| active.prompt.as_ref())
                        .map(|prompt| (prompt.kind, prompt.event_id.clone()))
                        .unwrap_or((QueuedPromptKind::User, request_id.to_string()))
                };
                let client_request_started = self
                    .pending_prompt_submissions
                    .lock()
                    .await
                    .remove(&prompt_event_id);
                let (transport_started_tx, transport_started_rx) = oneshot::channel();
                let backend = (*self).clone();
                let prompt_result_tx = self.prompt_result_tx.clone();
                let request_id_for_task = request_id.clone();
                let prompt_task = tokio::spawn(async move {
                    let session_id = backend.session_id.read().await.clone();
                    debug!(
                        target: "acp_event_flow",
                        request_id = %request_id_for_task,
                        session_id = %session_id,
                        ?prompt_kind,
                        content_blocks = prompt.len(),
                        "Sending ACP session/prompt request"
                    );
                    let result = backend
                        .connection
                        .prompt_with_request_id(session_id, prompt, Some(transport_started_tx))
                        .await;
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
                            let failure = backend.send_prompt_error(prompt_kind, &err).await;
                            let _ = prompt_result_tx
                                .send(InboundEvent::PromptFailed {
                                    failure: Some(failure),
                                })
                                .await;
                        }
                    }
                });
                *self.prompt_task_abort.lock().await = Some(prompt_task.abort_handle());

                match transport_started_rx.await {
                    Ok(Ok(wire_request_id)) => {
                        let actions =
                            self.session_driver
                                .lock()
                                .await
                                .apply(InboundEvent::PromptStarted {
                                    request_id: wire_request_id.clone(),
                                });
                        debug_assert!(actions.side_effects.is_empty());
                        debug_assert!(actions.completed_turn.is_none());
                        self.forward_client_events(&actions.events).await;
                        if let Some(request_started) = client_request_started {
                            let _ = request_started.send(Ok(wire_request_id));
                        }
                    }
                    Ok(Err(error)) => {
                        if let Some(request_started) = client_request_started {
                            let _ = request_started.send(Err(error));
                        }
                    }
                    Err(_) => {
                        if let Some(request_started) = client_request_started {
                            let _ = request_started.send(Err(anyhow::anyhow!(
                                "ACP transport did not assign a prompt request ID"
                            )));
                        }
                    }
                }
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

                let event_tx = self.backend_event_tx.clone();
                let prompt_task_abort = Arc::clone(&self.prompt_task_abort);
                let prompt_result_tx = self.prompt_result_tx.clone();
                let watchdog = tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(CANCEL_WARNING_SECS)).await;
                    warn!("Agent has not responded to cancel after {CANCEL_WARNING_SECS}s");
                    let _ = event_tx
                        .send(BackendEvent::Public(SessionEvent::Nori(
                            nori_protocol::NoriEvent::Notice(nori_protocol::Notice {
                                message: format!(
                                    "Agent is slow to cancel. Will force-cancel in {}s…",
                                    CANCEL_FORCE_SECS - CANCEL_WARNING_SECS
                                ),
                            }),
                        )))
                        .await;

                    tokio::time::sleep(std::time::Duration::from_secs(
                        CANCEL_FORCE_SECS - CANCEL_WARNING_SECS,
                    ))
                    .await;
                    if let Some(abort) = prompt_task_abort.lock().await.take() {
                        warn!("Force-cancelling prompt task after {CANCEL_FORCE_SECS}s timeout");
                        abort.abort();
                        let _ = prompt_result_tx
                            .send(InboundEvent::PromptFailed { failure: None })
                            .await;
                    }
                });
                *self.cancel_timeout_abort.lock().await = Some(watchdog.abort_handle());
            }
            SideEffect::ResolvePermissionCancelled { request_id } => {
                self.resolve_cancelled_permission(&request_id).await;
            }
        }
    }

    /// Surface a failed prompt as an `EventMsg::Error` (for display) and report
    /// its disposition so the caller can attach it to the prompt completion.
    async fn send_prompt_error(
        &self,
        prompt_kind: QueuedPromptKind,
        err: &anyhow::Error,
    ) -> crate::normalized::TurnFailure {
        let (message, retryable) = match prompt_kind {
            QueuedPromptKind::Compact => (format!("Compact failed: {err}"), false),
            QueuedPromptKind::GoalContinuation => {
                (format!("Goal continuation failed: {err}"), false)
            }
            QueuedPromptKind::User => {
                let error_string = format!("{err:?}");
                let AcpErrorDetails { category, detail } = categorize_acp_error_chain(err);
                // Top-level Display only: the alternate form (`{err:#}`) walks
                // the anyhow chain into `acp::Error`'s Display, which dumps the
                // pretty-printed `data` JSON blob. The clean agent-supplied
                // `data.detail` is appended parenthetically below instead,
                // mirroring `enhance_agent_error`.
                let display_error = format!("{err}");
                let retryable = category.is_retryable();
                warn!(
                    ?category,
                    retryable, "ACP user prompt failed: {error_string}"
                );
                let message = match category {
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
                    AcpErrorCategory::SessionNotFound => format!(
                        "The session no longer exists on the agent — it may have expired or been closed elsewhere: {display_error}"
                    ),
                    AcpErrorCategory::SessionNotResumable => format!(
                        "The session can't be reattached — start a new session instead: {display_error}"
                    ),
                    AcpErrorCategory::SessionAlreadyActive => format!(
                        "A session is already active on the agent connection — close it before switching: {display_error}"
                    ),
                    AcpErrorCategory::NoActiveSession => format!(
                        "No session is active on the agent connection — start a new chat with /new: {display_error}"
                    ),
                    AcpErrorCategory::AgentUnreachable => format!(
                        "The agent's backing service is unreachable — check your network and try again: {display_error}"
                    ),
                    AcpErrorCategory::Unknown => format!("ACP prompt failed: {display_error}"),
                };
                // The agent-supplied `error.data.detail` is clean, user-facing
                // context (e.g. "connection reset by broker") — keep it verbatim.
                let message = match detail {
                    Some(detail) => format!("{message} ({detail})"),
                    None => message,
                };
                (message, retryable)
            }
        };

        debug!("{message}");
        if retryable {
            crate::normalized::TurnFailure::Retryable
        } else {
            crate::normalized::TurnFailure::Fatal
        }
    }

    async fn resolve_cancelled_permission(&self, request_id: &str) {
        let mut pending = self.pending_approvals.lock().await;
        if let Some(position) = pending.iter().position(|pending_request| {
            pending_request.request.request_id.to_string() == request_id
        }) {
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
            user_notifier.notify(&crate::UserNotification::Idle {
                session_id,
                idle_duration_secs: idle_secs,
            });
        });
        *self.idle_timer_abort.lock().await = Some(idle_task.abort_handle());
    }
}
