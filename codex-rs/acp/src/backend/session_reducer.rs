//! Serialized reducer for ACP session turn state.
//!
//! All inbound ACP traffic for a session flows through [`reduce()`], which
//! mutates [`SessionRuntime`] and produces [`ClientEvent`]s. The caller
//! executes any [`SideEffect`]s after reduction.

use agent_client_protocol_schema as acp;
use nori_protocol::ClientEvent;
use nori_protocol::ClientEventNormalizer;
use nori_protocol::PromptCompleted;
use nori_protocol::QueueChanged;
use nori_protocol::WarningInfo;
use nori_protocol::session_runtime::ActiveRequestKind;
use nori_protocol::session_runtime::ActiveRequestState;
use nori_protocol::session_runtime::OpenMessage;
use nori_protocol::session_runtime::QueuedPrompt;
use nori_protocol::session_runtime::SessionPhase;
use nori_protocol::session_runtime::SessionRuntime;
use nori_protocol::session_runtime::TranscriptMessage;
use nori_protocol::session_runtime::TranscriptRole;

/// Everything that can affect [`SessionRuntime`] state.
#[derive(Debug)]
pub enum InboundEvent {
    /// A `session/update` notification from the agent.
    Notification(Box<acp::SessionUpdate>),
    /// The response to an active `session/prompt` request.
    PromptResponse { stop_reason: acp::StopReason },
    /// A transport/protocol failure for the active `session/prompt` request.
    PromptFailed,
    /// The response to an active `session/load` request.
    LoadResponse,
    /// A `session/request_permission` from the agent.
    PermissionRequest { request_id: String, call_id: String },
    /// The user submitted a prompt (may be queued if a request is in flight).
    PromptSubmit(QueuedPrompt),
    /// The user requested cancellation of the active prompt.
    CancelSubmit,
    /// A `session/load` was initiated.
    LoadSubmit { request_id: String },
}

/// Side effects the caller must execute after reduction.
#[derive(Debug, PartialEq)]
pub enum SideEffect {
    /// Send a `session/prompt` to the agent.
    SendPrompt {
        request_id: String,
        prompt: Vec<acp::ContentBlock>,
    },
    /// Send a `session/cancel` notification to the agent.
    SendCancel,
    /// Resolve a pending permission request as cancelled.
    ResolvePermissionCancelled { request_id: String },
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
        InboundEvent::CancelSubmit => {
            reduce_cancel_submit(runtime, &mut out);
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
        InboundEvent::PromptFailed => {
            reduce_prompt_failed(runtime, &mut out);
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
        out.events.push(ClientEvent::QueueChanged(QueueChanged {
            prompts: queued_prompt_texts(runtime),
        }));
        return;
    }

    start_prompt(runtime, prompt, out);
}

fn start_prompt(runtime: &mut SessionRuntime, prompt: QueuedPrompt, out: &mut ReduceOutput) {
    let request_id = new_request_id();

    // Build ACP content blocks from the queued prompt.
    let mut content_blocks = Vec::new();
    if !prompt.text.is_empty() {
        content_blocks.push(acp::ContentBlock::Text(acp::TextContent::new(&prompt.text)));
    }
    content_blocks.extend(prompt.images.clone());

    runtime.phase = SessionPhase::Prompt {
        request_id: request_id.clone(),
        cancelling: false,
    };
    runtime.active = Some(ActiveRequestState::new_prompt(
        request_id.clone(),
        prompt.clone(),
    ));

    // Add user message to transcript.
    if let Some(display_text) = &prompt.display_text
        && !display_text.is_empty()
    {
        runtime.persisted.transcript.push(TranscriptMessage {
            role: TranscriptRole::User,
            content: display_text.clone(),
        });
    }

    out.events
        .push(ClientEvent::SessionPhaseChanged(runtime.phase_view()));
    out.side_effects.push(SideEffect::SendPrompt {
        request_id,
        prompt: content_blocks,
    });
}

// ---------------------------------------------------------------------------
// Cancel submit
// ---------------------------------------------------------------------------

fn reduce_cancel_submit(runtime: &mut SessionRuntime, out: &mut ReduceOutput) {
    if let SessionPhase::Prompt {
        cancelling,
        request_id,
        ..
    } = &mut runtime.phase
    {
        if *cancelling {
            return; // double cancel is a no-op
        }
        *cancelling = true;
        let owner_id = request_id.clone();

        // Mark non-finished tool snapshots for this request as failed.
        for snapshot in runtime.persisted.tool_calls.values_mut() {
            if snapshot.owner_request_id.as_deref() == Some(&owner_id)
                && !is_terminal_phase(&snapshot.phase)
            {
                snapshot.phase = nori_protocol::ToolPhase::Failed;
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
    if !matches!(runtime.phase, SessionPhase::Prompt { .. }) {
        out.events.push(ClientEvent::Warning(WarningInfo {
            message: "Received prompt response while not in Prompt phase".to_string(),
        }));
        return;
    }

    let should_drain_queue = stop_reason == acp::StopReason::EndTurn;
    let last_agent_message = finalize_active(runtime);

    runtime.phase = SessionPhase::Idle;

    out.events
        .push(ClientEvent::SessionPhaseChanged(runtime.phase_view()));
    out.events
        .push(ClientEvent::PromptCompleted(PromptCompleted {
            stop_reason,
            last_agent_message,
        }));

    if should_drain_queue && let Some(next_prompt) = runtime.queue.pop_front() {
        out.events.push(ClientEvent::QueueChanged(QueueChanged {
            prompts: queued_prompt_texts(runtime),
        }));
        start_prompt(runtime, next_prompt, out);
    }
}

fn reduce_prompt_failed(runtime: &mut SessionRuntime, out: &mut ReduceOutput) {
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
        }));
}

// ---------------------------------------------------------------------------
// Load submit / response
// ---------------------------------------------------------------------------

fn reduce_load_submit(runtime: &mut SessionRuntime, request_id: String, out: &mut ReduceOutput) {
    if runtime.phase != SessionPhase::Idle {
        out.events.push(ClientEvent::Warning(WarningInfo {
            message: "Received load request while not idle".to_string(),
        }));
        return;
    }
    runtime.phase = SessionPhase::Loading {
        request_id: request_id.clone(),
    };
    runtime.active = Some(ActiveRequestState::new(
        request_id,
        ActiveRequestKind::Loading,
    ));
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
    // Session metadata updates are accepted in any phase.
    if is_session_metadata_update(&update) {
        reduce_metadata_update(runtime, &update, normalizer, out);
        return;
    }

    // Request-owned content requires an active request.
    if runtime.active.is_none() {
        out.events.push(ClientEvent::Warning(WarningInfo {
            message: "Received request-owned content update while no request is active".to_string(),
        }));
        let client_events = normalizer.push_session_update(&update);
        out.events.extend(client_events);
        return;
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
    let request_id = runtime.active.as_ref().map(|a| a.request_id.clone());
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
    let Some(active) = &mut runtime.active else {
        return;
    };

    let text = match &chunk.content {
        acp::ContentBlock::Text(t) => &t.text,
        _ => return,
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

// ---------------------------------------------------------------------------
// Active request finalization
// ---------------------------------------------------------------------------

/// Finalize open messages from the active request into the persisted
/// transcript, clear active, and return the last agent message text.
fn finalize_active(runtime: &mut SessionRuntime) -> Option<String> {
    let active = runtime.active.take()?;
    let mut last_agent_message = None;

    // Finalize open messages in order: user, thought, agent.
    if let Some(open) = active.open_user_message {
        let text = open.text();
        if !text.is_empty() {
            runtime.persisted.transcript.push(TranscriptMessage {
                role: TranscriptRole::User,
                content: text,
            });
        }
    }
    if let Some(open) = active.open_thought_message {
        let text = open.text();
        if !text.is_empty() {
            runtime.persisted.transcript.push(TranscriptMessage {
                role: TranscriptRole::Thought,
                content: text,
            });
        }
    }
    if let Some(open) = active.open_agent_message {
        let text = open.text();
        if !text.is_empty() {
            last_agent_message = Some(text.clone());
            runtime.persisted.transcript.push(TranscriptMessage {
                role: TranscriptRole::Agent,
                content: text,
            });
        }
    }

    last_agent_message
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
    )
}

fn is_terminal_phase(phase: &nori_protocol::ToolPhase) -> bool {
    matches!(
        phase,
        nori_protocol::ToolPhase::Completed | nori_protocol::ToolPhase::Failed
    )
}

fn queued_prompt_texts(runtime: &SessionRuntime) -> Vec<String> {
    runtime
        .queue
        .iter()
        .filter(|prompt| {
            matches!(
                prompt.kind,
                nori_protocol::session_runtime::QueuedPromptKind::User
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

fn new_request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests;
