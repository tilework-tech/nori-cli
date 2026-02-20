use super::*;

/// Takes a user message as input and runs a loop where, at each turn, the model
/// replies with either:
///
/// - requested function calls
/// - an assistant message
///
/// While it is possible for the model to return multiple of these items in a
/// single turn, in practice, we generally one item per turn:
///
/// - If the model requests a function call, we execute it and send the output
///   back to the model in the next turn.
/// - If the model sends only an assistant message, we record it in the
///   conversation history and consider the task complete.
///
pub(crate) async fn run_task(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    input: Vec<UserInput>,
    cancellation_token: CancellationToken,
) -> Option<String> {
    if input.is_empty() {
        return None;
    }
    let event = EventMsg::TaskStarted(TaskStartedEvent {
        model_context_window: turn_context.client.get_model_context_window(),
    });
    sess.send_event(&turn_context, event).await;

    let initial_input_for_turn: ResponseInputItem = ResponseInputItem::from(input);
    let response_item: ResponseItem = initial_input_for_turn.clone().into();
    sess.record_response_item_and_emit_turn_item(turn_context.as_ref(), response_item)
        .await;

    sess.maybe_start_ghost_snapshot(Arc::clone(&turn_context), cancellation_token.child_token())
        .await;
    let mut last_agent_message: Option<String> = None;
    // Although from the perspective of codex.rs, TurnDiffTracker has the lifecycle of a Task which contains
    // many turns, from the perspective of the user, it is a single turn.
    let turn_diff_tracker = Arc::new(tokio::sync::Mutex::new(TurnDiffTracker::new()));

    loop {
        // Note that pending_input would be something like a message the user
        // submitted through the UI while the model was running. Though the UI
        // may support this, the model might not.
        let pending_input = sess
            .get_pending_input()
            .await
            .into_iter()
            .map(ResponseItem::from)
            .collect::<Vec<ResponseItem>>();

        // Construct the input that we will send to the model.
        let turn_input: Vec<ResponseItem> = {
            sess.record_conversation_items(&turn_context, &pending_input)
                .await;
            sess.clone_history().await.get_history_for_prompt()
        };

        let turn_input_messages = turn_input
            .iter()
            .filter_map(|item| match parse_turn_item(item) {
                Some(TurnItem::UserMessage(user_message)) => Some(user_message),
                _ => None,
            })
            .map(|user_message| user_message.message())
            .collect::<Vec<String>>();
        match run_turn(
            Arc::clone(&sess),
            Arc::clone(&turn_context),
            Arc::clone(&turn_diff_tracker),
            turn_input,
            cancellation_token.child_token(),
        )
        .await
        {
            Ok(turn_output) => {
                let processed_items = turn_output;
                let limit = turn_context
                    .client
                    .get_auto_compact_token_limit()
                    .unwrap_or(i64::MAX);
                let total_usage_tokens = sess.get_total_token_usage().await;
                let token_limit_reached = total_usage_tokens >= limit;
                let (responses, items_to_record_in_conversation_history) =
                    process_items(processed_items, &sess, &turn_context).await;

                // as long as compaction works well in getting us way below the token limit, we shouldn't worry about being in an infinite loop.
                if token_limit_reached {
                    if should_use_remote_compact_task(&sess).await {
                        run_inline_remote_auto_compact_task(sess.clone(), turn_context.clone())
                            .await;
                    } else {
                        run_inline_auto_compact_task(sess.clone(), turn_context.clone()).await;
                    }
                    continue;
                }

                if responses.is_empty() {
                    last_agent_message = get_last_assistant_message_from_turn(
                        &items_to_record_in_conversation_history,
                    );
                    sess.notifier()
                        .notify(&UserNotification::AgentTurnComplete {
                            thread_id: sess.conversation_id.to_string(),
                            turn_id: turn_context.sub_id.clone(),
                            cwd: turn_context.cwd.display().to_string(),
                            input_messages: turn_input_messages,
                            last_assistant_message: last_agent_message.clone(),
                        });
                    break;
                }
                continue;
            }
            Err(CodexErr::TurnAborted {
                dangling_artifacts: processed_items,
            }) => {
                let _ = process_items(processed_items, &sess, &turn_context).await;
                // Aborted turn is reported via a different event.
                break;
            }
            Err(e) => {
                info!("Turn error: {e:#}");
                let event = EventMsg::Error(e.to_error_event(None));
                sess.send_event(&turn_context, event).await;
                // let the user continue the conversation
                break;
            }
        }
    }

    last_agent_message
}

async fn run_turn(
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    turn_diff_tracker: SharedTurnDiffTracker,
    input: Vec<ResponseItem>,
    cancellation_token: CancellationToken,
) -> CodexResult<Vec<ProcessedResponseItem>> {
    let mcp_tools = sess
        .services
        .mcp_connection_manager
        .read()
        .await
        .list_all_tools()
        .or_cancel(&cancellation_token)
        .await?;
    let router = Arc::new(ToolRouter::from_config(
        &turn_context.tools_config,
        Some(
            mcp_tools
                .into_iter()
                .map(|(name, tool)| (name, tool.tool))
                .collect(),
        ),
    ));

    let model_supports_parallel = turn_context
        .client
        .get_model_family()
        .supports_parallel_tool_calls;

    // TODO(jif) revert once testing phase is done.
    let parallel_tool_calls = model_supports_parallel
        && sess
            .state
            .lock()
            .await
            .session_configuration
            .features
            .enabled(Feature::ParallelToolCalls);
    let mut base_instructions = turn_context.base_instructions.clone();
    if parallel_tool_calls {
        static INSTRUCTIONS: &str = include_str!("../../templates/parallel/instructions.md");
        if let Some(family) =
            find_family_for_model(&sess.state.lock().await.session_configuration.model)
        {
            let mut new_instructions = base_instructions.unwrap_or(family.base_instructions);
            new_instructions.push_str(INSTRUCTIONS);
            base_instructions = Some(new_instructions);
        }
    }
    let prompt = Prompt {
        input,
        tools: router.specs(),
        parallel_tool_calls,
        base_instructions_override: base_instructions,
        output_schema: turn_context.final_output_json_schema.clone(),
    };

    let mut retries = 0;
    loop {
        match try_run_turn(
            Arc::clone(&router),
            Arc::clone(&sess),
            Arc::clone(&turn_context),
            Arc::clone(&turn_diff_tracker),
            &prompt,
            cancellation_token.child_token(),
        )
        .await
        {
            Ok(output) => return Ok(output),
            Err(CodexErr::TurnAborted {
                dangling_artifacts: processed_items,
            }) => {
                return Err(CodexErr::TurnAborted {
                    dangling_artifacts: processed_items,
                });
            }
            Err(CodexErr::Interrupted) => return Err(CodexErr::Interrupted),
            Err(CodexErr::EnvVar(var)) => return Err(CodexErr::EnvVar(var)),
            Err(e @ CodexErr::Fatal(_)) => return Err(e),
            Err(e @ CodexErr::ContextWindowExceeded) => {
                sess.set_total_tokens_full(&turn_context).await;
                return Err(e);
            }
            Err(CodexErr::UsageLimitReached(e)) => {
                let rate_limits = e.rate_limits.clone();
                if let Some(rate_limits) = rate_limits {
                    sess.update_rate_limits(&turn_context, rate_limits).await;
                }
                return Err(CodexErr::UsageLimitReached(e));
            }
            Err(CodexErr::UsageNotIncluded) => return Err(CodexErr::UsageNotIncluded),
            Err(e @ CodexErr::QuotaExceeded) => return Err(e),
            Err(e @ CodexErr::RefreshTokenFailed(_)) => return Err(e),
            Err(e) => {
                // Use the configured provider-specific stream retry budget.
                let max_retries = turn_context.client.get_provider().stream_max_retries();
                if retries < max_retries {
                    retries += 1;
                    let delay = match e {
                        CodexErr::Stream(_, Some(delay)) => delay,
                        _ => backoff(retries),
                    };
                    warn!(
                        "stream disconnected - retrying turn ({retries}/{max_retries} in {delay:?})...",
                    );

                    // Surface retry information to any UI/front-end so the
                    // user understands what is happening instead of staring
                    // at a seemingly frozen screen.
                    sess.notify_stream_error(
                        &turn_context,
                        format!("Reconnecting... {retries}/{max_retries}"),
                        e,
                    )
                    .await;

                    tokio::time::sleep(delay).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn try_run_turn(
    router: Arc<ToolRouter>,
    sess: Arc<Session>,
    turn_context: Arc<TurnContext>,
    turn_diff_tracker: SharedTurnDiffTracker,
    prompt: &Prompt,
    cancellation_token: CancellationToken,
) -> CodexResult<Vec<ProcessedResponseItem>> {
    let rollout_item = RolloutItem::TurnContext(TurnContextItem {
        cwd: turn_context.cwd.clone(),
        approval_policy: turn_context.approval_policy,
        sandbox_policy: turn_context.sandbox_policy.clone(),
        model: turn_context.client.get_model(),
        effort: turn_context.client.get_reasoning_effort(),
        summary: turn_context.client.get_reasoning_summary(),
    });

    sess.persist_rollout_items(&[rollout_item]).await;
    let mut stream = turn_context
        .client
        .clone()
        .stream(prompt)
        .or_cancel(&cancellation_token)
        .await??;

    let tool_runtime = ToolCallRuntime::new(
        Arc::clone(&router),
        Arc::clone(&sess),
        Arc::clone(&turn_context),
        Arc::clone(&turn_diff_tracker),
    );
    let mut output: FuturesOrdered<BoxFuture<CodexResult<ProcessedResponseItem>>> =
        FuturesOrdered::new();

    let mut active_item: Option<TurnItem> = None;

    loop {
        // Poll the next item from the model stream. We must inspect *both* Ok and Err
        // cases so that transient stream failures (e.g., dropped SSE connection before
        // `response.completed`) bubble up and trigger the caller's retry logic.
        let event = match stream.next().or_cancel(&cancellation_token).await {
            Ok(event) => event,
            Err(codex_async_utils::CancelErr::Cancelled) => {
                let processed_items = output.try_collect().await?;
                return Err(CodexErr::TurnAborted {
                    dangling_artifacts: processed_items,
                });
            }
        };

        let event = match event {
            Some(res) => res?,
            None => {
                return Err(CodexErr::Stream(
                    "stream closed before response.completed".into(),
                    None,
                ));
            }
        };

        let add_completed = &mut |response_item: ProcessedResponseItem| {
            output.push_back(future::ready(Ok(response_item)).boxed());
        };

        match event {
            ResponseEvent::Created => {}
            ResponseEvent::OutputItemDone(item) => {
                let previously_active_item = active_item.take();
                match ToolRouter::build_tool_call(sess.as_ref(), item.clone()).await {
                    Ok(Some(call)) => {
                        let payload_preview = call.payload.log_payload().into_owned();
                        tracing::info!("ToolCall: {} {}", call.tool_name, payload_preview);

                        let response =
                            tool_runtime.handle_tool_call(call, cancellation_token.child_token());

                        output.push_back(
                            async move {
                                Ok(ProcessedResponseItem {
                                    item,
                                    response: Some(response.await?),
                                })
                            }
                            .boxed(),
                        );
                    }
                    Ok(None) => {
                        if let Some(turn_item) = handle_non_tool_response_item(&item).await {
                            if previously_active_item.is_none() {
                                sess.emit_turn_item_started(&turn_context, &turn_item).await;
                            }

                            sess.emit_turn_item_completed(&turn_context, turn_item)
                                .await;
                        }

                        add_completed(ProcessedResponseItem {
                            item,
                            response: None,
                        });
                    }
                    Err(FunctionCallError::MissingLocalShellCallId) => {
                        let msg = "LocalShellCall without call_id or id";
                        turn_context
                            .client
                            .get_otel_event_manager()
                            .log_tool_failed("local_shell", msg);
                        error!(msg);

                        let response = ResponseInputItem::FunctionCallOutput {
                            call_id: String::new(),
                            output: FunctionCallOutputPayload {
                                content: msg.to_string(),
                                ..Default::default()
                            },
                        };
                        add_completed(ProcessedResponseItem {
                            item,
                            response: Some(response),
                        });
                    }
                    Err(FunctionCallError::RespondToModel(message))
                    | Err(FunctionCallError::Denied(message)) => {
                        let response = ResponseInputItem::FunctionCallOutput {
                            call_id: String::new(),
                            output: FunctionCallOutputPayload {
                                content: message,
                                ..Default::default()
                            },
                        };
                        add_completed(ProcessedResponseItem {
                            item,
                            response: Some(response),
                        });
                    }
                    Err(FunctionCallError::Fatal(message)) => {
                        return Err(CodexErr::Fatal(message));
                    }
                }
            }
            ResponseEvent::OutputItemAdded(item) => {
                if let Some(turn_item) = handle_non_tool_response_item(&item).await {
                    let tracked_item = turn_item.clone();
                    sess.emit_turn_item_started(&turn_context, &turn_item).await;

                    active_item = Some(tracked_item);
                }
            }
            ResponseEvent::RateLimits(snapshot) => {
                // Update internal state with latest rate limits, but defer sending until
                // token usage is available to avoid duplicate TokenCount events.
                sess.update_rate_limits(&turn_context, snapshot).await;
            }
            ResponseEvent::Completed {
                response_id: _,
                token_usage,
            } => {
                sess.update_token_usage_info(&turn_context, token_usage.as_ref())
                    .await;
                let processed_items = output.try_collect().await?;
                let unified_diff = {
                    let mut tracker = turn_diff_tracker.lock().await;
                    tracker.get_unified_diff()
                };
                if let Ok(Some(unified_diff)) = unified_diff {
                    let msg = EventMsg::TurnDiff(TurnDiffEvent { unified_diff });
                    sess.send_event(&turn_context, msg).await;
                }

                return Ok(processed_items);
            }
            ResponseEvent::OutputTextDelta(delta) => {
                if let Some(active) = active_item.as_ref() {
                    let event = AgentMessageContentDeltaEvent {
                        thread_id: sess.conversation_id.to_string(),
                        turn_id: turn_context.sub_id.clone(),
                        item_id: active.id(),
                        delta: delta.clone(),
                    };
                    sess.send_event(&turn_context, EventMsg::AgentMessageContentDelta(event))
                        .await;
                } else {
                    error_or_panic("OutputTextDelta without active item".to_string());
                }
            }
            ResponseEvent::ReasoningSummaryDelta {
                delta,
                summary_index,
            } => {
                if let Some(active) = active_item.as_ref() {
                    let event = ReasoningContentDeltaEvent {
                        thread_id: sess.conversation_id.to_string(),
                        turn_id: turn_context.sub_id.clone(),
                        item_id: active.id(),
                        delta,
                        summary_index,
                    };
                    sess.send_event(&turn_context, EventMsg::ReasoningContentDelta(event))
                        .await;
                } else {
                    error_or_panic("ReasoningSummaryDelta without active item".to_string());
                }
            }
            ResponseEvent::ReasoningSummaryPartAdded { summary_index } => {
                if let Some(active) = active_item.as_ref() {
                    let event =
                        EventMsg::AgentReasoningSectionBreak(AgentReasoningSectionBreakEvent {
                            item_id: active.id(),
                            summary_index,
                        });
                    sess.send_event(&turn_context, event).await;
                } else {
                    error_or_panic("ReasoningSummaryPartAdded without active item".to_string());
                }
            }
            ResponseEvent::ReasoningContentDelta {
                delta,
                content_index,
            } => {
                if let Some(active) = active_item.as_ref() {
                    let event = ReasoningRawContentDeltaEvent {
                        thread_id: sess.conversation_id.to_string(),
                        turn_id: turn_context.sub_id.clone(),
                        item_id: active.id(),
                        delta,
                        content_index,
                    };
                    sess.send_event(&turn_context, EventMsg::ReasoningRawContentDelta(event))
                        .await;
                } else {
                    error_or_panic("ReasoningRawContentDelta without active item".to_string());
                }
            }
        }
    }
}

async fn handle_non_tool_response_item(item: &ResponseItem) -> Option<TurnItem> {
    debug!(?item, "Output item");

    match item {
        ResponseItem::Message { .. }
        | ResponseItem::Reasoning { .. }
        | ResponseItem::WebSearchCall { .. } => parse_turn_item(item),
        ResponseItem::FunctionCallOutput { .. } | ResponseItem::CustomToolCallOutput { .. } => {
            debug!("unexpected tool output from stream");
            None
        }
        _ => None,
    }
}

pub(crate) fn get_last_assistant_message_from_turn(responses: &[ResponseItem]) -> Option<String> {
    responses.iter().rev().find_map(|item| {
        if let ResponseItem::Message { role, content, .. } = item {
            if role == "assistant" {
                content.iter().rev().find_map(|ci| {
                    if let ContentItem::OutputText { text } = ci {
                        Some(text.clone())
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        } else {
            None
        }
    })
}
