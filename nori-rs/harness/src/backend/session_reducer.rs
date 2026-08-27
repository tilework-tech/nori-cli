//! Serialized reducer for ACP session turn state.
//!
//! All inbound ACP traffic for a session flows through [`reduce()`], which
//! mutates [`SessionRuntime`] and produces [`ClientEvent`]s. The caller
//! executes any [`SideEffect`]s after reduction.

use crate::normalized::ClientEvent;
use crate::normalized::ClientEventNormalizer;
use crate::normalized::PromptCompleted;
use crate::normalized::QueueChanged;
use crate::normalized::WarningInfo;
use crate::normalized::session_runtime::ActiveRequestState;
use crate::normalized::session_runtime::OpenMessage;
use crate::normalized::session_runtime::QueuedPrompt;
use crate::normalized::session_runtime::SessionPhase;
use crate::normalized::session_runtime::SessionRuntime;
use crate::normalized::session_runtime::TranscriptMessage;
use crate::normalized::session_runtime::TranscriptRole;
use nori_protocol::acp::v1 as acp;
use tracing::debug;

/// Everything that can affect [`SessionRuntime`] state.
#[derive(Debug)]
pub enum InboundEvent {
    /// A `session/update` notification from the agent.
    Notification(Box<acp::SessionUpdate>),
    /// The response to an active `session/prompt` request.
    PromptResponse { stop_reason: acp::StopReason },
    /// A transport/protocol failure for the active `session/prompt` request.
    /// `failure` describes the disposition carried onto the completion (`None`
    /// for a clean forced-cancel/timeout).
    PromptFailed {
        failure: Option<crate::normalized::TurnFailure>,
    },
    /// The response to an active `session/load` request.
    LoadResponse,
    /// The transport has assigned the active prompt its ACP wire request ID.
    PromptStarted { request_id: acp::RequestId },
    /// A `session/request_permission` from the agent.
    PermissionRequest { request_id: String, call_id: String },
    /// The user submitted a prompt (may be queued if a request is in flight).
    PromptSubmit(QueuedPrompt),
    /// A remote controller submitted a prompt that must not queue.
    PromptSubmitIfIdle(QueuedPrompt),
    /// The user requested cancellation of the active prompt.
    CancelSubmit,
    /// A controller requested cancellation only if it owns this prompt.
    CancelSubmitFor { request_id: acp::RequestId },
    /// A `session/load` was initiated.
    LoadSubmit { request_id: acp::RequestId },
}

pub(super) fn inbound_event_kind(event: &InboundEvent) -> &'static str {
    match event {
        InboundEvent::Notification(update) => crate::connection::session_update_kind(update),
        InboundEvent::PromptResponse { .. } => "prompt_response",
        InboundEvent::PromptFailed { .. } => "prompt_failed",
        InboundEvent::LoadResponse => "load_response",
        InboundEvent::PromptStarted { .. } => "prompt_started",
        InboundEvent::PermissionRequest { .. } => "permission_request",
        InboundEvent::PromptSubmit(_) | InboundEvent::PromptSubmitIfIdle(_) => "prompt_submit",
        InboundEvent::CancelSubmit | InboundEvent::CancelSubmitFor { .. } => "cancel_submit",
        InboundEvent::LoadSubmit { .. } => "load_submit",
    }
}

pub(super) fn session_phase_label(phase: &SessionPhase) -> &'static str {
    match phase {
        SessionPhase::Idle => "idle",
        SessionPhase::Loading { .. } => "loading",
        SessionPhase::Prompt {
            cancelling: true, ..
        } => "cancelling",
        SessionPhase::Prompt {
            cancelling: false, ..
        } => "prompt",
    }
}

/// Side effects the caller must execute after reduction.
#[derive(Debug, PartialEq)]
pub enum SideEffect {
    /// Send a `session/prompt` to the agent.
    SendPrompt {
        request_id: acp::RequestId,
        prompt: Vec<acp::ContentBlock>,
    },
    /// Send a `session/cancel` notification to the agent.
    SendCancel,
    /// Resolve a pending permission request as cancelled.
    ResolvePermissionCancelled { request_id: String },
    /// Reject a prompt instead of adding it to the queue.
    RejectPromptBusy { event_id: String },
}

/// The output of a single reduction step.
pub struct ReduceOutput {
    pub events: Vec<ClientEvent>,
    pub side_effects: Vec<SideEffect>,
}

/// Process one inbound event, mutating the session runtime and producing
/// client events and side effects.
pub fn reduce(
    runtime: &mut SessionRuntime,
    event: InboundEvent,
    normalizer: &mut ClientEventNormalizer,
) -> ReduceOutput {
    let mut out = ReduceOutput {
        events: Vec::new(),
        side_effects: Vec::new(),
    };

    match event {
        InboundEvent::PromptSubmit(prompt) => {
            reduce_prompt_submit(runtime, prompt, &mut out);
        }
        InboundEvent::PromptSubmitIfIdle(prompt) => {
            if runtime.phase == SessionPhase::Idle && runtime.queue.is_empty() {
                start_prompt(runtime, prompt, &mut out);
            } else {
                out.side_effects.push(SideEffect::RejectPromptBusy {
                    event_id: prompt.event_id,
                });
            }
        }
        InboundEvent::PromptStarted { request_id } => {
            reduce_prompt_started(runtime, request_id, &mut out);
        }
        InboundEvent::CancelSubmit => {
            reduce_cancel_submit(runtime, None, &mut out);
        }
        InboundEvent::CancelSubmitFor { request_id } => {
            reduce_cancel_submit(runtime, Some(&request_id), &mut out);
        }
        InboundEvent::LoadSubmit { request_id } => {
            reduce_load_submit(runtime, request_id, &mut out);
        }
        InboundEvent::Notification(update) => {
            reduce_notification(runtime, *update, normalizer, &mut out);
        }
        InboundEvent::PromptResponse { stop_reason } => {
            reduce_prompt_response(runtime, stop_reason, &mut out);
        }
        InboundEvent::PromptFailed { failure } => {
            reduce_prompt_failed(runtime, failure, &mut out);
        }
        InboundEvent::LoadResponse => {
            reduce_load_response(runtime, &mut out);
        }
        InboundEvent::PermissionRequest {
            request_id,
            call_id,
        } => {
            reduce_permission_request(runtime, request_id, call_id, &mut out);
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Prompt submit
// ---------------------------------------------------------------------------

fn reduce_prompt_submit(
    runtime: &mut SessionRuntime,
    prompt: QueuedPrompt,
    out: &mut ReduceOutput,
) {
    if runtime.phase != SessionPhase::Idle {
        runtime.queue.push_back(prompt);
        debug!(
            target: "acp_event_flow",
            phase = session_phase_label(&runtime.phase),
            queue_len = runtime.queue.len(),
            "Queued prompt while another session request is active"
        );
        out.events.push(ClientEvent::QueueChanged(QueueChanged {
            prompts: queued_prompt_texts(runtime),
        }));
        return;
    }

    start_prompt(runtime, prompt, out);
}

fn start_prompt(runtime: &mut SessionRuntime, prompt: QueuedPrompt, out: &mut ReduceOutput) {
    let phase_before = session_phase_label(&runtime.phase);
    let request_id = new_request_id();

    runtime.phase = SessionPhase::Prompt {
        request_id: request_id.clone(),
        cancelling: false,
    };
    runtime.active = Some(ActiveRequestState::new_prompt(
        request_id.clone(),
        prompt.clone(),
    ));
    runtime.orphan_update_warning_emitted = false;

    // Add user message to transcript.
    if let Some(display_text) = &prompt.display_text
        && !display_text.is_empty()
    {
        runtime.persisted.transcript.push(TranscriptMessage {
            role: TranscriptRole::User,
            content: display_text.clone(),
        });
    }

    debug!(
        target: "acp_event_flow",
        request_id = %request_id,
        prompt_kind = ?prompt.kind,
        phase_before,
        queue_len = runtime.queue.len(),
        "Reducer started prompt and emitted session/prompt side effect"
    );

    out.side_effects.push(SideEffect::SendPrompt {
        request_id,
        prompt: prompt.content,
    });
}

fn reduce_prompt_started(
    runtime: &mut SessionRuntime,
    request_id: acp::RequestId,
    out: &mut ReduceOutput,
) {
    let SessionPhase::Prompt {
        request_id: phase_request_id,
        ..
    } = &mut runtime.phase
    else {
        out.events.push(ClientEvent::Warning(WarningInfo {
            message: "Transport started a prompt while no prompt was active".to_string(),
        }));
        return;
    };

    *phase_request_id = request_id.clone();
    if let Some(active) = &mut runtime.active {
        active.request_id = request_id;
    }
    out.events
        .push(ClientEvent::SessionPhaseChanged(runtime.phase_view()));
}

// ---------------------------------------------------------------------------
// Cancel submit
// ---------------------------------------------------------------------------

fn reduce_cancel_submit(
    runtime: &mut SessionRuntime,
    expected_request_id: Option<&acp::RequestId>,
    out: &mut ReduceOutput,
) {
    if let SessionPhase::Prompt {
        cancelling,
        request_id,
        ..
    } = &mut runtime.phase
    {
        if expected_request_id.is_some_and(|expected| expected != request_id) {
            return;
        }
        if *cancelling {
            return; // double cancel is a no-op
        }
        *cancelling = true;
        let owner_id = request_id.to_string();

        // Mark non-finished tool snapshots for this request as failed.
        for snapshot in runtime.persisted.tool_calls.values_mut() {
            if snapshot.owner_request_id.as_deref() == Some(owner_id.as_str())
                && !is_terminal_phase(&snapshot.phase)
            {
                snapshot.phase = crate::normalized::ToolPhase::Failed;
            }
        }

        // Resolve pending permission requests as cancelled.
        if let Some(active) = &runtime.active {
            for perm_id in &active.pending_permission_requests {
                out.side_effects
                    .push(SideEffect::ResolvePermissionCancelled {
                        request_id: perm_id.clone(),
                    });
            }
        }

        debug!(
            target: "acp_event_flow",
            request_id = %owner_id,
            pending_permission_requests = runtime
                .active
                .as_ref()
                .map_or(0, |active| active.pending_permission_requests.len()),
            tool_calls = runtime
                .active
                .as_ref()
                .map_or(0, |active| active.tool_call_ids.len()),
            "Reducer marked the active prompt as cancelling"
        );

        out.events
            .push(ClientEvent::SessionPhaseChanged(runtime.phase_view()));
        out.side_effects.push(SideEffect::SendCancel);
    }
}

// ---------------------------------------------------------------------------
// Prompt response
// ---------------------------------------------------------------------------

fn reduce_prompt_response(
    runtime: &mut SessionRuntime,
    stop_reason: acp::StopReason,
    out: &mut ReduceOutput,
) {
    let active_request_id = runtime
        .active
        .as_ref()
        .map(|active| active.request_id.to_string())
        .unwrap_or_else(|| "<none>".to_string());
    let phase_before = session_phase_label(&runtime.phase);
    let queue_len_before = runtime.queue.len();
    debug!(
        target: "acp_event_flow",
        active_request_id,
        phase_before,
        queue_len_before,
        ?stop_reason,
        "Reducer received prompt response"
    );

    if !matches!(runtime.phase, SessionPhase::Prompt { .. }) {
        out.events.push(ClientEvent::Warning(WarningInfo {
            message: "Received prompt response while not in Prompt phase".to_string(),
        }));
        return;
    }

    let should_drain_queue = stop_reason == acp::StopReason::EndTurn;
    let last_agent_message = finalize_active(runtime);

    runtime.phase = SessionPhase::Idle;

    debug!(
        target: "acp_event_flow",
        active_request_id,
        ?stop_reason,
        should_drain_queue,
        queue_len_after_finalize = runtime.queue.len(),
        "Reducer finalized prompt response"
    );

    out.events
        .push(ClientEvent::SessionPhaseChanged(runtime.phase_view()));
    out.events
        .push(ClientEvent::PromptCompleted(PromptCompleted {
            stop_reason,
            last_agent_message,
            failure: None,
        }));

    if should_drain_queue && let Some(next_prompt) = runtime.queue.pop_front() {
        out.events.push(ClientEvent::QueueChanged(QueueChanged {
            prompts: queued_prompt_texts(runtime),
        }));
        start_prompt(runtime, next_prompt, out);
    }
}

fn reduce_prompt_failed(
    runtime: &mut SessionRuntime,
    failure: Option<crate::normalized::TurnFailure>,
    out: &mut ReduceOutput,
) {
    let active_request_id = runtime
        .active
        .as_ref()
        .map(|active| active.request_id.to_string())
        .unwrap_or_else(|| "<none>".to_string());
    debug!(
        target: "acp_event_flow",
        active_request_id,
        phase = session_phase_label(&runtime.phase),
        "Reducer received prompt failure"
    );

    if !matches!(runtime.phase, SessionPhase::Prompt { .. }) {
        out.events.push(ClientEvent::Warning(WarningInfo {
            message: "Received prompt failure while not in Prompt phase".to_string(),
        }));
        return;
    }

    let last_agent_message = finalize_active(runtime);
    runtime.phase = SessionPhase::Idle;
    out.events
        .push(ClientEvent::SessionPhaseChanged(runtime.phase_view()));
    out.events
        .push(ClientEvent::PromptCompleted(PromptCompleted {
            stop_reason: acp::StopReason::Cancelled,
            last_agent_message,
            failure,
        }));
}

// ---------------------------------------------------------------------------
// Load submit / response
// ---------------------------------------------------------------------------

fn reduce_load_submit(
    runtime: &mut SessionRuntime,
    request_id: acp::RequestId,
    out: &mut ReduceOutput,
) {
    if runtime.phase != SessionPhase::Idle {
        out.events.push(ClientEvent::Warning(WarningInfo {
            message: "Received load request while not idle".to_string(),
        }));
        return;
    }
    runtime.phase = SessionPhase::Loading {
        request_id: request_id.clone(),
    };
    runtime.active = Some(ActiveRequestState::new_loading(request_id));
    runtime.orphan_update_warning_emitted = false;
    out.events
        .push(ClientEvent::SessionPhaseChanged(runtime.phase_view()));
}

fn reduce_load_response(runtime: &mut SessionRuntime, out: &mut ReduceOutput) {
    if !matches!(runtime.phase, SessionPhase::Loading { .. }) {
        out.events.push(ClientEvent::Warning(WarningInfo {
            message: "Received load response while not in Loading phase".to_string(),
        }));
        return;
    }

    finalize_active(runtime);
    runtime.phase = SessionPhase::Idle;
    out.events
        .push(ClientEvent::SessionPhaseChanged(runtime.phase_view()));
    out.events.push(ClientEvent::LoadCompleted);
    // Loads never drain the queue.
}

// ---------------------------------------------------------------------------
// Notification (session/update)
// ---------------------------------------------------------------------------

fn reduce_notification(
    runtime: &mut SessionRuntime,
    update: acp::SessionUpdate,
    normalizer: &mut ClientEventNormalizer,
    out: &mut ReduceOutput,
) {
    debug!(
        target: "acp_event_flow",
        update_kind = crate::connection::session_update_kind(&update),
        phase = session_phase_label(&runtime.phase),
        active_request_id = runtime
            .active
            .as_ref()
            .map(|active| active.request_id.to_string())
            .unwrap_or_else(|| "<none>".to_string()),
        "Reducer received session/update"
    );

    // Session metadata updates are accepted in any phase.
    if is_session_metadata_update(&update) {
        reduce_metadata_update(runtime, &update, normalizer, out);
        return;
    }

    // Warn once for content that no local prompt or load owns.
    if runtime.active.is_none()
        && !runtime.observer_turn_active
        && !runtime.orphan_update_warning_emitted
    {
        out.events.push(ClientEvent::Warning(WarningInfo {
            message: "Received update with no active local request".to_string(),
        }));
        runtime.orphan_update_warning_emitted = true;
    }

    // Route to specific handlers.
    match &update {
        acp::SessionUpdate::AgentMessageChunk(chunk) => {
            append_chunk_to_open_message(runtime, chunk, MessageKind::Agent);
        }
        acp::SessionUpdate::AgentThoughtChunk(chunk) => {
            append_chunk_to_open_message(runtime, chunk, MessageKind::Thought);
        }
        acp::SessionUpdate::UserMessageChunk(chunk) => {
            append_chunk_to_open_message(runtime, chunk, MessageKind::User);
        }
        acp::SessionUpdate::Plan(_) => {
            // Plan patches persisted state.
        }
        acp::SessionUpdate::ToolCall(tool_call) => {
            reduce_tool_call(runtime, tool_call);
        }
        acp::SessionUpdate::ToolCallUpdate(tool_update) => {
            reduce_tool_call_update(runtime, tool_update);
        }
        _ => {}
    }

    // Always forward to normalizer for ClientEvent production.
    let client_events = normalizer.push_session_update(&update);

    // Patch owner_request_id on any ToolSnapshot events.
    let request_id = runtime
        .active
        .as_ref()
        .map(|active| active.request_id.to_string());
    let client_events = client_events
        .into_iter()
        .map(|event| match event {
            ClientEvent::ToolSnapshot(mut snapshot) => {
                if snapshot.owner_request_id.is_none() {
                    snapshot.owner_request_id = request_id.clone();
                }
                ClientEvent::ToolSnapshot(snapshot)
            }
            other => other,
        })
        .collect::<Vec<_>>();

    // Update persisted state from produced events.
    for event in &client_events {
        match event {
            ClientEvent::PlanSnapshot(plan) => {
                runtime.persisted.plan = Some(plan.clone());
            }
            ClientEvent::ToolSnapshot(snapshot) => {
                runtime
                    .persisted
                    .tool_calls
                    .insert(snapshot.call_id.clone(), snapshot.clone());
            }
            _ => {}
        }
    }

    out.events.extend(client_events);
}

fn reduce_metadata_update(
    runtime: &mut SessionRuntime,
    update: &acp::SessionUpdate,
    normalizer: &mut ClientEventNormalizer,
    out: &mut ReduceOutput,
) {
    match update {
        acp::SessionUpdate::AvailableCommandsUpdate(_) => {}
        acp::SessionUpdate::CurrentModeUpdate(current_mode) => {
            runtime.persisted.current_mode = Some(current_mode.current_mode_id.to_string());
        }
        acp::SessionUpdate::ConfigOptionUpdate(config_options) => {
            runtime.persisted.config_options = config_options.config_options.clone();
        }
        acp::SessionUpdate::SessionInfoUpdate(session_info) => {
            if let Some(title) = session_info.title.as_opt_ref() {
                runtime.persisted.session_info.title = title.cloned();
            }
            if let Some(updated_at) = session_info.updated_at.as_opt_ref() {
                runtime.persisted.session_info.updated_at = updated_at.cloned();
            }
            if runtime.active.is_none()
                && let Some(status) = session_info
                    .meta
                    .as_ref()
                    .and_then(|meta| meta.get("nori"))
                    .and_then(|nori| nori.get("status"))
                    .and_then(serde_json::Value::as_str)
            {
                match status {
                    "working" => runtime.observer_turn_active = true,
                    "idle" => {
                        runtime.observer_turn_active = false;
                        runtime.orphan_update_warning_emitted = false;
                    }
                    _ => {}
                }
            }
        }
        acp::SessionUpdate::UsageUpdate(usage) => {
            runtime.persisted.session_usage =
                Some(crate::normalized::session_runtime::SessionUsageState {
                    used_tokens: saturating_i64(usage.used),
                    total_tokens: saturating_i64(usage.size),
                    cost_display: usage
                        .cost
                        .as_ref()
                        .map(|cost| format!("{:.2} {}", cost.amount, cost.currency)),
                });
        }
        _ => {}
    }

    let client_events = normalizer.push_session_update(update);

    for event in &client_events {
        if let ClientEvent::AgentCommandsUpdate(commands_update) = event {
            runtime.persisted.available_commands = commands_update.commands.clone();
        }
    }

    out.events.extend(client_events);
}

// ---------------------------------------------------------------------------
// Tool call handling
// ---------------------------------------------------------------------------

fn reduce_tool_call(runtime: &mut SessionRuntime, tool_call: &acp::ToolCall) {
    let call_id = tool_call.tool_call_id.to_string();

    if let Some(active) = &mut runtime.active
        && !active.tool_call_ids.contains(&call_id)
    {
        active.tool_call_ids.push(call_id);
    }

    // The persisted tool snapshot will be set by the normalizer output +
    // owner_request_id patching in reduce_notification.
}

fn reduce_tool_call_update(runtime: &mut SessionRuntime, tool_update: &acp::ToolCallUpdate) {
    let call_id = tool_update.tool_call_id.to_string();

    if let Some(active) = &mut runtime.active
        && !active.tool_call_ids.contains(&call_id)
    {
        active.tool_call_ids.push(call_id);
    }
}

// ---------------------------------------------------------------------------
// Permission request
// ---------------------------------------------------------------------------

fn reduce_permission_request(
    runtime: &mut SessionRuntime,
    request_id: String,
    _call_id: String,
    out: &mut ReduceOutput,
) {
    match &runtime.phase {
        SessionPhase::Prompt { .. } => {
            if let Some(active) = &mut runtime.active {
                active.pending_permission_requests.insert(request_id);
            }
        }
        _ => {
            out.events.push(ClientEvent::Warning(WarningInfo {
                message: "Received permission request while no prompt is active".to_string(),
            }));
        }
    }
}

// ---------------------------------------------------------------------------
// Message assembly
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, PartialEq, Eq)]
enum MessageKind {
    Agent,
    Thought,
    User,
}

fn append_chunk_to_open_message(
    runtime: &mut SessionRuntime,
    chunk: &acp::ContentChunk,
    kind: MessageKind,
) {
    let text = match &chunk.content {
        acp::ContentBlock::Text(t) => &t.text,
        _ => return,
    };

    flush_open_messages_except(runtime, kind);

    let Some(active) = &mut runtime.active else {
        return;
    };

    let open = match kind {
        MessageKind::Agent => active
            .open_agent_message
            .get_or_insert_with(OpenMessage::new),
        MessageKind::Thought => active
            .open_thought_message
            .get_or_insert_with(OpenMessage::new),
        MessageKind::User => active
            .open_user_message
            .get_or_insert_with(OpenMessage::new),
    };

    open.chunks.push(text.clone());
}

fn flush_open_messages_except(runtime: &mut SessionRuntime, keep: MessageKind) {
    let (user, thought, agent) = {
        let Some(active) = &mut runtime.active else {
            return;
        };
        (
            (keep != MessageKind::User)
                .then(|| active.open_user_message.take())
                .flatten(),
            (keep != MessageKind::Thought)
                .then(|| active.open_thought_message.take())
                .flatten(),
            (keep != MessageKind::Agent)
                .then(|| active.open_agent_message.take())
                .flatten(),
        )
    };

    push_open_transcript_message(runtime, TranscriptRole::User, user);
    push_open_transcript_message(runtime, TranscriptRole::Thought, thought);
    if let Some(text) = push_open_transcript_message(runtime, TranscriptRole::Agent, agent)
        && let Some(active) = &mut runtime.active
    {
        active.last_agent_message = Some(text);
    }
}

// ---------------------------------------------------------------------------
// Active request finalization
// ---------------------------------------------------------------------------

/// Finalize open messages from the active request into the persisted
/// transcript, clear active, and return the last agent message text.
fn finalize_active(runtime: &mut SessionRuntime) -> Option<String> {
    let active = runtime.active.take()?;
    let mut last_agent_message = active.last_agent_message;

    // At most one text kind should still be open because kind switches flush
    // the previous buffer as chunks arrive.
    push_open_transcript_message(runtime, TranscriptRole::User, active.open_user_message);
    push_open_transcript_message(
        runtime,
        TranscriptRole::Thought,
        active.open_thought_message,
    );
    if let Some(text) =
        push_open_transcript_message(runtime, TranscriptRole::Agent, active.open_agent_message)
    {
        last_agent_message = Some(text);
    }

    last_agent_message
}

fn push_open_transcript_message(
    runtime: &mut SessionRuntime,
    role: TranscriptRole,
    open: Option<OpenMessage>,
) -> Option<String> {
    let text = open?.text();
    if text.is_empty() {
        return None;
    }

    runtime.persisted.transcript.push(TranscriptMessage {
        role,
        content: text.clone(),
    });

    (role == TranscriptRole::Agent).then_some(text)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn is_session_metadata_update(update: &acp::SessionUpdate) -> bool {
    matches!(
        update,
        acp::SessionUpdate::AvailableCommandsUpdate(_)
            | acp::SessionUpdate::CurrentModeUpdate(_)
            | acp::SessionUpdate::ConfigOptionUpdate(_)
            | acp::SessionUpdate::SessionInfoUpdate(_)
            | acp::SessionUpdate::UsageUpdate(_)
    )
}

fn saturating_i64(value: u64) -> i64 {
    match i64::try_from(value) {
        Ok(value) => value,
        Err(_) => i64::MAX,
    }
}

fn is_terminal_phase(phase: &crate::normalized::ToolPhase) -> bool {
    matches!(
        phase,
        crate::normalized::ToolPhase::Completed | crate::normalized::ToolPhase::Failed
    )
}

fn queued_prompt_texts(runtime: &SessionRuntime) -> Vec<String> {
    runtime
        .queue
        .iter()
        .filter(|prompt| {
            matches!(
                prompt.kind,
                crate::normalized::session_runtime::QueuedPromptKind::User
            )
        })
        .filter_map(|prompt| {
            prompt
                .display_text
                .clone()
                .or_else(|| Some(prompt.text.clone()))
        })
        .collect()
}

fn new_request_id() -> acp::RequestId {
    acp::RequestId::Str(uuid::Uuid::new_v4().to_string())
}

#[cfg(test)]
mod tests;
