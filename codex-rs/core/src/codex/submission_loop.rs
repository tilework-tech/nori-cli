use super::*;

pub(super) async fn submission_loop(
    sess: Arc<Session>,
    config: Arc<Config>,
    rx_sub: Receiver<Submission>,
) {
    // Seed with context in case there is an OverrideTurnContext first.
    let mut previous_context: Option<Arc<TurnContext>> =
        Some(sess.new_turn(SessionSettingsUpdate::default()).await);

    // To break out of this loop, send Op::Shutdown.
    while let Ok(sub) = rx_sub.recv().await {
        debug!(?sub, "Submission");
        match sub.op.clone() {
            Op::Interrupt => {
                handlers::interrupt(&sess).await;
            }
            Op::OverrideTurnContext {
                cwd,
                approval_policy,
                sandbox_policy,
                model,
                effort,
                summary,
            } => {
                handlers::override_turn_context(
                    &sess,
                    SessionSettingsUpdate {
                        cwd,
                        approval_policy,
                        sandbox_policy,
                        model,
                        reasoning_effort: effort,
                        reasoning_summary: summary,
                        ..Default::default()
                    },
                )
                .await;
            }
            Op::UserInput { .. } | Op::UserTurn { .. } => {
                handlers::user_input_or_turn(&sess, sub.id.clone(), sub.op, &mut previous_context)
                    .await;
            }
            Op::ExecApproval { id, decision } => {
                handlers::exec_approval(&sess, id, decision).await;
            }
            Op::PatchApproval { id, decision } => {
                handlers::patch_approval(&sess, id, decision).await;
            }
            Op::AddToHistory { text } => {
                handlers::add_to_history(&sess, &config, text).await;
            }
            Op::GetHistoryEntryRequest { offset, log_id } => {
                handlers::get_history_entry_request(&sess, &config, sub.id.clone(), offset, log_id)
                    .await;
            }
            Op::ListCustomPrompts => {
                handlers::list_custom_prompts(&sess, sub.id.clone()).await;
            }
            Op::Undo => {
                handlers::undo(&sess, sub.id.clone()).await;
            }
            Op::Compact => {
                handlers::compact(&sess, sub.id.clone()).await;
            }
            Op::RunUserShellCommand { command } => {
                handlers::run_user_shell_command(
                    &sess,
                    sub.id.clone(),
                    command,
                    &mut previous_context,
                )
                .await;
            }
            Op::ResolveElicitation {
                server_name,
                request_id,
                decision,
            } => {
                handlers::resolve_elicitation(&sess, server_name, request_id, decision).await;
            }
            Op::Shutdown => {
                if handlers::shutdown(&sess, sub.id.clone()).await {
                    break;
                }
            }
            _ => {} // Ignore unknown ops; enum is non_exhaustive to allow extensions.
        }
    }
    debug!("Agent loop exited");
}

/// Operation handlers
mod handlers {
    use crate::codex::Session;
    use crate::codex::SessionSettingsUpdate;
    use crate::codex::TurnContext;

    use crate::config::Config;
    use crate::tasks::CompactTask;
    use crate::tasks::RegularTask;
    use crate::tasks::UndoTask;
    use crate::tasks::UserShellCommandTask;
    use codex_protocol::custom_prompts::CustomPrompt;
    use codex_protocol::protocol::CodexErrorInfo;
    use codex_protocol::protocol::ErrorEvent;
    use codex_protocol::protocol::Event;
    use codex_protocol::protocol::EventMsg;
    use codex_protocol::protocol::ListCustomPromptsResponseEvent;
    use codex_protocol::protocol::Op;
    use codex_protocol::protocol::ReviewDecision;
    use codex_protocol::protocol::TurnAbortReason;

    use codex_protocol::user_input::UserInput;
    use codex_rmcp_client::ElicitationAction;
    use codex_rmcp_client::ElicitationResponse;
    use mcp_types::RequestId;
    use std::sync::Arc;
    use tracing::info;
    use tracing::warn;

    pub async fn interrupt(sess: &Arc<Session>) {
        sess.interrupt_task().await;
    }

    pub async fn override_turn_context(sess: &Session, updates: SessionSettingsUpdate) {
        sess.update_settings(updates).await;
    }

    pub async fn user_input_or_turn(
        sess: &Arc<Session>,
        sub_id: String,
        op: Op,
        previous_context: &mut Option<Arc<TurnContext>>,
    ) {
        let (items, updates) = match op {
            Op::UserTurn {
                cwd,
                approval_policy,
                sandbox_policy,
                model,
                effort,
                summary,
                final_output_json_schema,
                items,
            } => (
                items,
                SessionSettingsUpdate {
                    cwd: Some(cwd),
                    approval_policy: Some(approval_policy),
                    sandbox_policy: Some(sandbox_policy),
                    model: Some(model),
                    reasoning_effort: Some(effort),
                    reasoning_summary: Some(summary),
                    final_output_json_schema: Some(final_output_json_schema),
                },
            ),
            Op::UserInput { items } => (items, SessionSettingsUpdate::default()),
            _ => unreachable!(),
        };

        let current_context = sess.new_turn_with_sub_id(sub_id, updates).await;
        current_context
            .client
            .get_otel_event_manager()
            .user_prompt(&items);

        // Attempt to inject input into current task
        if let Err(items) = sess.inject_input(items).await {
            if let Some(env_item) =
                sess.build_environment_update_item(previous_context.as_ref(), &current_context)
            {
                sess.record_conversation_items(&current_context, std::slice::from_ref(&env_item))
                    .await;
            }

            sess.spawn_task(Arc::clone(&current_context), items, RegularTask)
                .await;
            *previous_context = Some(current_context);
        }
    }

    pub async fn run_user_shell_command(
        sess: &Arc<Session>,
        sub_id: String,
        command: String,
        previous_context: &mut Option<Arc<TurnContext>>,
    ) {
        let turn_context = sess
            .new_turn_with_sub_id(sub_id, SessionSettingsUpdate::default())
            .await;
        sess.spawn_task(
            Arc::clone(&turn_context),
            Vec::new(),
            UserShellCommandTask::new(command),
        )
        .await;
        *previous_context = Some(turn_context);
    }

    pub async fn resolve_elicitation(
        sess: &Arc<Session>,
        server_name: String,
        request_id: RequestId,
        decision: codex_protocol::approvals::ElicitationAction,
    ) {
        let action = match decision {
            codex_protocol::approvals::ElicitationAction::Accept => ElicitationAction::Accept,
            codex_protocol::approvals::ElicitationAction::Decline => ElicitationAction::Decline,
            codex_protocol::approvals::ElicitationAction::Cancel => ElicitationAction::Cancel,
        };
        let response = ElicitationResponse {
            action,
            content: None,
        };
        if let Err(err) = sess
            .resolve_elicitation(server_name, request_id, response)
            .await
        {
            warn!(
                error = %err,
                "failed to resolve elicitation request in session"
            );
        }
    }

    pub async fn exec_approval(sess: &Arc<Session>, id: String, decision: ReviewDecision) {
        match decision {
            ReviewDecision::Abort => {
                sess.interrupt_task().await;
            }
            other => sess.notify_approval(&id, other).await,
        }
    }

    pub async fn patch_approval(sess: &Arc<Session>, id: String, decision: ReviewDecision) {
        match decision {
            ReviewDecision::Abort => {
                sess.interrupt_task().await;
            }
            other => sess.notify_approval(&id, other).await,
        }
    }

    pub async fn add_to_history(sess: &Arc<Session>, config: &Arc<Config>, text: String) {
        let id = sess.conversation_id;
        let config = Arc::clone(config);
        tokio::spawn(async move {
            if let Err(e) = crate::message_history::append_entry(&text, &id, &config).await {
                warn!("failed to append to message history: {e}");
            }
        });
    }

    pub async fn get_history_entry_request(
        sess: &Arc<Session>,
        config: &Arc<Config>,
        sub_id: String,
        offset: usize,
        log_id: u64,
    ) {
        let config = Arc::clone(config);
        let sess_clone = Arc::clone(sess);

        tokio::spawn(async move {
            // Run lookup in blocking thread because it does file IO + locking.
            let entry_opt = tokio::task::spawn_blocking(move || {
                crate::message_history::lookup(log_id, offset, &config)
            })
            .await
            .unwrap_or(None);

            let event = Event {
                id: sub_id,
                msg: EventMsg::GetHistoryEntryResponse(
                    crate::protocol::GetHistoryEntryResponseEvent {
                        offset,
                        log_id,
                        entry: entry_opt.map(|e| codex_protocol::message_history::HistoryEntry {
                            conversation_id: e.session_id,
                            ts: e.ts,
                            text: e.text,
                        }),
                    },
                ),
            };

            sess_clone.send_event_raw(event).await;
        });
    }

    pub async fn list_custom_prompts(sess: &Session, sub_id: String) {
        let custom_prompts: Vec<CustomPrompt> =
            if let Some(dir) = crate::custom_prompts::default_prompts_dir() {
                crate::custom_prompts::discover_prompts_in(&dir).await
            } else {
                Vec::new()
            };

        let event = Event {
            id: sub_id,
            msg: EventMsg::ListCustomPromptsResponse(ListCustomPromptsResponseEvent {
                custom_prompts,
            }),
        };
        sess.send_event_raw(event).await;
    }

    pub async fn undo(sess: &Arc<Session>, sub_id: String) {
        let turn_context = sess
            .new_turn_with_sub_id(sub_id, SessionSettingsUpdate::default())
            .await;
        sess.spawn_task(turn_context, Vec::new(), UndoTask::new())
            .await;
    }

    pub async fn compact(sess: &Arc<Session>, sub_id: String) {
        let turn_context = sess
            .new_turn_with_sub_id(sub_id, SessionSettingsUpdate::default())
            .await;

        sess.spawn_task(
            Arc::clone(&turn_context),
            vec![UserInput::Text {
                text: turn_context.compact_prompt().to_string(),
            }],
            CompactTask,
        )
        .await;
    }

    pub async fn shutdown(sess: &Arc<Session>, sub_id: String) -> bool {
        sess.abort_all_tasks(TurnAbortReason::Interrupted).await;
        sess.services
            .unified_exec_manager
            .terminate_all_sessions()
            .await;
        info!("Shutting down Codex instance");

        // Gracefully flush and shutdown rollout recorder on session end so tests
        // that inspect the rollout file do not race with the background writer.
        let recorder_opt = {
            let mut guard = sess.services.rollout.lock().await;
            guard.take()
        };
        if let Some(rec) = recorder_opt
            && let Err(e) = rec.shutdown().await
        {
            warn!("failed to shutdown rollout recorder: {e}");
            let event = Event {
                id: sub_id.clone(),
                msg: EventMsg::Error(ErrorEvent {
                    message: "Failed to shutdown rollout recorder".to_string(),
                    codex_error_info: Some(CodexErrorInfo::Other),
                }),
            };
            sess.send_event_raw(event).await;
        }

        let event = Event {
            id: sub_id,
            msg: EventMsg::ShutdownComplete,
        };
        sess.send_event_raw(event).await;
        true
    }
}
