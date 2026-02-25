use super::*;

/// Get a human-readable name for an Op variant
pub(crate) fn get_op_name(op: &Op) -> &'static str {
    match op {
        Op::Interrupt => "Interrupt",
        Op::UserInput { .. } => "UserInput",
        Op::UserTurn { .. } => "UserTurn",
        Op::OverrideTurnContext { .. } => "OverrideTurnContext",
        Op::ExecApproval { .. } => "ExecApproval",
        Op::PatchApproval { .. } => "PatchApproval",
        Op::ResolveElicitation { .. } => "ResolveElicitation",
        Op::AddToHistory { .. } => "AddToHistory",
        Op::GetHistoryEntryRequest { .. } => "GetHistoryEntryRequest",
        Op::SearchHistoryRequest { .. } => "SearchHistoryRequest",
        Op::ListMcpTools => "ListMcpTools",
        Op::ListCustomPrompts => "ListCustomPrompts",
        Op::Compact => "Compact",
        Op::Undo => "Undo",
        Op::UndoList => "UndoList",
        Op::UndoTo { .. } => "UndoTo",
        Op::Shutdown => "Shutdown",
        Op::RunUserShellCommand { .. } => "RunUserShellCommand",
        _ => "Unknown",
    }
}

/// Get a human-readable name for an EventMsg variant
pub(crate) fn get_event_msg_type(msg: &EventMsg) -> &'static str {
    match msg {
        EventMsg::SessionConfigured(_) => "SessionConfigured",
        EventMsg::TaskStarted(_) => "TaskStarted",
        EventMsg::TaskComplete(_) => "TaskComplete",
        EventMsg::AgentMessageDelta(_) => "AgentMessageDelta",
        EventMsg::AgentReasoningDelta(_) => "AgentReasoningDelta",
        EventMsg::ExecCommandBegin(_) => "ExecCommandBegin",
        EventMsg::ExecCommandEnd(_) => "ExecCommandEnd",
        EventMsg::ExecApprovalRequest(_) => "ExecApprovalRequest",
        EventMsg::TurnAborted(_) => "TurnAborted",
        EventMsg::Error(_) => "Error",
        EventMsg::ShutdownComplete => "ShutdownComplete",
        _ => "Other",
    }
}

/// Accumulated state for a tool call that was skipped on the initial ToolCall event
/// (because it lacked useful display info) but may receive details in subsequent
/// ToolCallUpdate events before completion.
#[derive(Default)]
pub(crate) struct AccumulatedToolCall {
    pub title: Option<String>,
    pub kind: Option<acp::ToolKind>,
    pub raw_input: Option<serde_json::Value>,
    pub meta_tool_name: Option<String>,
}

/// Extract the tool name from `_meta.claudeCode.toolName` if present.
pub(crate) fn extract_meta_tool_name(meta: Option<&acp::Meta>) -> Option<String> {
    meta?
        .get("claudeCode")?
        .get("toolName")?
        .as_str()
        .map(String::from)
}

/// Translate an ACP SessionUpdate to codex_protocol::EventMsg variants.
///
/// The `pending_patch_changes` map stores FileChange data from ToolCall events
/// so that it can be retrieved when ToolCallUpdate arrives (after approval).
///
/// The `pending_tool_calls` map accumulates title/kind/raw_input from skipped
/// ToolCall events and intermediate (non-completed) ToolCallUpdate events,
/// so the best available display name can be resolved on completion.
pub(crate) fn translate_session_update_to_events(
    update: &acp::SessionUpdate,
    pending_patch_changes: &mut std::collections::HashMap<
        String,
        std::collections::HashMap<PathBuf, codex_protocol::protocol::FileChange>,
    >,
    pending_tool_calls: &mut std::collections::HashMap<String, AccumulatedToolCall>,
) -> Vec<EventMsg> {
    match update {
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                debug!(
                    target: "acp_event_flow",
                    event_type = "AgentMessageChunk",
                    delta_len = text.text.len(),
                    delta_preview = %truncate_for_log(&text.text, 50),
                    "ACP -> TUI: streaming text delta"
                );
                vec![EventMsg::AgentMessageDelta(
                    codex_protocol::protocol::AgentMessageDeltaEvent {
                        delta: text.text.clone(),
                    },
                )]
            } else {
                vec![]
            }
        }
        acp::SessionUpdate::AgentThoughtChunk(chunk) => {
            if let acp::ContentBlock::Text(text) = &chunk.content {
                debug!(
                    target: "acp_event_flow",
                    event_type = "AgentThoughtChunk",
                    delta_len = text.text.len(),
                    "ACP -> TUI: reasoning delta"
                );
                vec![EventMsg::AgentReasoningDelta(
                    codex_protocol::protocol::AgentReasoningDeltaEvent {
                        delta: text.text.clone(),
                    },
                )]
            } else {
                vec![]
            }
        }
        acp::SessionUpdate::ToolCall(tool_call) => {
            // Skip Begin events that don't have useful display information.
            // The ACP protocol emits multiple ToolCall events for the same call_id:
            // 1. First event: generic (title="Read File", raw_input={} or partial)
            // 2. Second event: detailed (title="Read /path/to/file.rs", raw_input={path: "..."})
            // We only want to emit the detailed one to avoid duplicate Begin events in the TUI.
            //
            // Check for useful info in EITHER:
            // - raw_input (has path, command, pattern, etc.)
            // - title itself (contains an absolute path like "Read /home/...")
            let display_args = tool_call
                .raw_input
                .as_ref()
                .and_then(|input| extract_display_args(&tool_call.title, input));
            let title_has_path = title_contains_useful_info(&tool_call.title);
            if display_args.is_none() && !title_has_path {
                debug!(
                    target: "acp_event_flow",
                    event_type = "ToolCall",
                    call_id = %tool_call.tool_call_id,
                    title = %tool_call.title,
                    has_raw_input = tool_call.raw_input.is_some(),
                    title_has_path = title_has_path,
                    "ACP: skipping generic ToolCall (no display args), waiting for detailed event"
                );
                // Store whatever data we have so it can be used on completion
                pending_tool_calls.insert(
                    tool_call.tool_call_id.to_string(),
                    AccumulatedToolCall {
                        title: Some(tool_call.title.clone()),
                        kind: Some(tool_call.kind),
                        raw_input: tool_call.raw_input.clone(),
                        meta_tool_name: extract_meta_tool_name(tool_call.meta.as_ref()),
                    },
                );
                return vec![];
            }

            // For patch operations (Edit/Write/Delete), don't emit anything on ToolCall.
            // Store the FileChange data so we can emit PatchApplyBegin on ToolCallUpdate.
            // The approval request will be shown first via ApplyPatchApprovalRequest.
            if is_patch_operation(
                Some(&tool_call.kind),
                &tool_call.title,
                tool_call.raw_input.as_ref(),
            ) && let Some((path, change)) =
                tool_call_to_file_change(Some(&tool_call.kind), tool_call.raw_input.as_ref())
            {
                let mut changes = std::collections::HashMap::new();
                changes.insert(path, change);

                // Store for retrieval on ToolCallUpdate
                pending_patch_changes.insert(tool_call.tool_call_id.to_string(), changes);

                debug!(
                    target: "acp_event_flow",
                    event_type = "ToolCall",
                    call_id = %tool_call.tool_call_id,
                    title = %tool_call.title,
                    kind = ?tool_call.kind,
                    "ACP: stored patch changes for later (will show after approval)"
                );
                return vec![];
            }

            // Format command with tool name and input arguments for better display
            let command = format_tool_call_command(&tool_call.title, tool_call.raw_input.as_ref());
            // Classify the tool call to enable proper TUI rendering (Exploring vs Command mode)
            let parsed_cmd = classify_tool_to_parsed_command(
                &tool_call.title,
                Some(&tool_call.kind),
                tool_call.raw_input.as_ref(),
            );
            debug!(
                target: "acp_event_flow",
                event_type = "ToolCall",
                call_id = %tool_call.tool_call_id,
                title = %tool_call.title,
                kind = ?tool_call.kind,
                command = %command,
                parsed_cmd_count = parsed_cmd.len(),
                has_raw_input = tool_call.raw_input.is_some(),
                "ACP -> TUI: ExecCommandBegin (tool call started)"
            );
            vec![EventMsg::ExecCommandBegin(
                codex_protocol::protocol::ExecCommandBeginEvent {
                    call_id: tool_call.tool_call_id.to_string(),
                    process_id: None,
                    turn_id: String::new(),
                    command: vec![command],
                    cwd: PathBuf::new(),
                    parsed_cmd,
                    source: codex_protocol::protocol::ExecCommandSource::Agent,
                    interaction_input: None,
                },
            )]
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            // Tool call updates can be mapped based on status
            let status = update.fields.status;
            let title = update.fields.title.clone().unwrap_or_default();
            debug!(
                target: "acp_event_flow",
                event_type = "ToolCallUpdate",
                call_id = %update.tool_call_id,
                status = ?status,
                title = %title,
                "ACP: tool call update received"
            );
            if status == Some(acp::ToolCallStatus::Completed) {
                // Check if we have stored patch changes from the original ToolCall event.
                // This data was stored when we first saw the ToolCall, before approval.
                let call_id = update.tool_call_id.to_string();
                if let Some(changes) = pending_patch_changes.remove(&call_id) {
                    pending_tool_calls.remove(&call_id);
                    debug!(
                        target: "acp_event_flow",
                        event_type = "ToolCallUpdate",
                        call_id = %call_id,
                        title = %title,
                        num_files = changes.len(),
                        "ACP -> TUI: PatchApplyBegin (showing completed file operation)"
                    );

                    // Use PatchApplyBegin to create the history cell with the diff
                    return vec![EventMsg::PatchApplyBegin(PatchApplyBeginEvent {
                        call_id,
                        turn_id: String::new(),
                        auto_approved: true, // Already approved by this point
                        changes,
                    })];
                }

                // Resolve the best available title by merging accumulated data
                // with whatever this completion update provides.
                let accumulated = pending_tool_calls.remove(&call_id);
                let meta_tool_name = extract_meta_tool_name(update.meta.as_ref())
                    .or_else(|| accumulated.as_ref().and_then(|a| a.meta_tool_name.clone()));

                // Title resolution: update fields > accumulated > meta toolName > kind display name
                let resolved_title = if !title.is_empty() && !title_is_raw_id(&title) {
                    title
                } else if let Some(ref acc) = accumulated
                    && let Some(ref acc_title) = acc.title
                    && !acc_title.is_empty()
                    && !title_is_raw_id(acc_title)
                {
                    acc_title.clone()
                } else if let Some(ref meta_name) = meta_tool_name {
                    meta_name.clone()
                } else {
                    // Last resort: use kind-based display name
                    let kind = update
                        .fields
                        .kind
                        .or_else(|| accumulated.as_ref().and_then(|a| a.kind));
                    kind.map(kind_to_display_name).unwrap_or("Tool").to_string()
                };

                let resolved_kind = update
                    .fields
                    .kind
                    .as_ref()
                    .or_else(|| accumulated.as_ref().and_then(|a| a.kind.as_ref()));
                let resolved_raw_input = update
                    .fields
                    .raw_input
                    .as_ref()
                    .or_else(|| accumulated.as_ref().and_then(|a| a.raw_input.as_ref()));

                // Extract output from tool call content and raw_output
                let aggregated_output = extract_tool_output(&update.fields);
                let command = format_tool_call_command(&resolved_title, resolved_raw_input);
                // Classify the tool call to enable proper TUI rendering (Exploring vs Command mode)
                let parsed_cmd = classify_tool_to_parsed_command(
                    &resolved_title,
                    resolved_kind,
                    resolved_raw_input,
                );

                debug!(
                    target: "acp_event_flow",
                    event_type = "ToolCallUpdate",
                    call_id = %update.tool_call_id,
                    title = %resolved_title,
                    command = %command,
                    output_len = aggregated_output.len(),
                    "ACP -> TUI: ExecCommandEnd (tool call completed)"
                );
                vec![EventMsg::ExecCommandEnd(
                    codex_protocol::protocol::ExecCommandEndEvent {
                        call_id: update.tool_call_id.to_string(),
                        process_id: None,
                        turn_id: String::new(),
                        command: vec![command],
                        cwd: PathBuf::new(),
                        parsed_cmd,
                        source: codex_protocol::protocol::ExecCommandSource::Agent,
                        interaction_input: None,
                        stdout: String::new(),
                        stderr: String::new(),
                        aggregated_output,
                        exit_code: 0,
                        duration: std::time::Duration::ZERO,
                        formatted_output: String::new(),
                    },
                )]
            } else {
                // Non-completed update: accumulate title/kind/raw_input for later use
                let call_id = update.tool_call_id.to_string();
                let meta_tool_name = extract_meta_tool_name(update.meta.as_ref());
                let acc = pending_tool_calls.entry(call_id).or_default();
                if let Some(ref t) = update.fields.title {
                    acc.title = Some(t.clone());
                }
                if let Some(k) = update.fields.kind {
                    acc.kind = Some(k);
                }
                if let Some(ref ri) = update.fields.raw_input {
                    acc.raw_input = Some(ri.clone());
                }
                if let Some(mn) = meta_tool_name {
                    acc.meta_tool_name = Some(mn);
                }
                vec![]
            }
        }
        // Other update types don't have direct event mappings
        other => {
            debug!(
                target: "acp_event_flow",
                event_type = ?std::mem::discriminant(other),
                "ACP: unhandled update type (no event emitted)"
            );
            vec![]
        }
    }
}

/// Record tool call and result events to the transcript.
///
/// This handles recording both regular tool calls (as ToolCall/ToolResult entries)
/// and patch operations (as PatchApply entries). Patch operations (Edit/Write/Delete)
/// are recorded separately because they represent file modifications rather than
/// generic tool invocations.
pub(crate) async fn record_tool_events_to_transcript(
    update: &acp::SessionUpdate,
    recorder: &TranscriptRecorder,
    recorded_call_ids: &mut std::collections::HashSet<String>,
) {
    match update {
        acp::SessionUpdate::ToolCall(tool_call) => {
            let call_id = tool_call.tool_call_id.to_string();

            // Skip if we've already recorded this call_id (ACP may send multiple
            // ToolCall events for the same call_id as details become available)
            if recorded_call_ids.contains(&call_id) {
                return;
            }

            // Skip patch operations here - they're recorded on ToolCallUpdate completion
            if is_patch_operation(
                Some(&tool_call.kind),
                &tool_call.title,
                tool_call.raw_input.as_ref(),
            ) {
                return;
            }

            // Record non-patch tool calls
            let input = tool_call.raw_input.clone().unwrap_or(serde_json::json!({}));
            if let Err(e) = recorder
                .record_tool_call(&call_id, &tool_call.title, &input)
                .await
            {
                warn!("Failed to record tool call to transcript: {e}");
            } else {
                recorded_call_ids.insert(call_id);
            }
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            // Only record completed tool calls
            if update.fields.status != Some(acp::ToolCallStatus::Completed) {
                return;
            }

            let call_id = update.tool_call_id.to_string();
            let title = update.fields.title.clone().unwrap_or_default();
            let kind = update.fields.kind;

            // Check if this is a patch operation
            if is_patch_operation(kind.as_ref(), &title, update.fields.raw_input.as_ref()) {
                // Record as patch operation
                let operation = match kind {
                    Some(acp::ToolKind::Edit) => crate::transcript::PatchOperationType::Edit,
                    Some(acp::ToolKind::Delete) => crate::transcript::PatchOperationType::Delete,
                    _ => {
                        // Default to Write for other kinds (including None)
                        crate::transcript::PatchOperationType::Write
                    }
                };

                // Extract path from raw_input or locations
                let path = update
                    .fields
                    .raw_input
                    .as_ref()
                    .and_then(|input| {
                        input
                            .get("file_path")
                            .or_else(|| input.get("path"))
                            .and_then(|v| v.as_str())
                            .map(PathBuf::from)
                    })
                    .or_else(|| {
                        update
                            .fields
                            .locations
                            .as_ref()
                            .and_then(|locs| locs.first())
                            .map(|loc| loc.path.clone())
                    })
                    .unwrap_or_else(|| PathBuf::from("unknown"));

                // Completed status means success (Failed status handled separately)
                if let Err(e) = recorder
                    .record_patch_apply(&call_id, operation, &path, true, None)
                    .await
                {
                    warn!("Failed to record patch apply to transcript: {e}");
                }
            } else {
                // Record as tool result for non-patch operations
                let output = extract_tool_output(&update.fields);
                let truncated = output.len() > 10000;
                let output_to_record = if truncated {
                    let safe = codex_utils_string::take_bytes_at_char_boundary(&output, 10000);
                    format!("{safe}... (truncated)")
                } else {
                    output
                };

                // Extract exit_code from raw_output if available
                let exit_code = update
                    .fields
                    .raw_output
                    .as_ref()
                    .and_then(|v| v.get("exit_code"))
                    .and_then(serde_json::Value::as_i64)
                    .map(|v| v as i32);

                if let Err(e) = recorder
                    .record_tool_result(&call_id, &output_to_record, truncated, exit_code)
                    .await
                {
                    warn!("Failed to record tool result to transcript: {e}");
                }
            }
        }
        _ => {}
    }
}
