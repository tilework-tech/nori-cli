use agent_client_protocol_schema as acp;
use nori_protocol::ClientEvent;
use nori_protocol::ClientEventNormalizer;
use nori_protocol::session_runtime::ActiveRequestKind;
use nori_protocol::session_runtime::QueuedPrompt;
use nori_protocol::session_runtime::QueuedPromptKind;
use nori_protocol::session_runtime::SessionPhase;
use nori_protocol::session_runtime::SessionPhaseView;
use nori_protocol::session_runtime::SessionRuntime;
use pretty_assertions::assert_eq;

use super::InboundEvent;
use super::SideEffect;
use super::reduce;

fn new_runtime() -> SessionRuntime {
    SessionRuntime::new()
}

fn new_normalizer() -> ClientEventNormalizer {
    ClientEventNormalizer::default()
}

fn simple_prompt() -> QueuedPrompt {
    QueuedPrompt {
        event_id: "evt-1".to_string(),
        kind: QueuedPromptKind::User,
        text: "hello".to_string(),
        display_text: Some("hello".to_string()),
        images: Vec::new(),
    }
}

fn notification(update: acp::SessionUpdate) -> InboundEvent {
    InboundEvent::Notification(Box::new(update))
}

fn has_event(events: &[ClientEvent], pred: impl Fn(&ClientEvent) -> bool) -> bool {
    events.iter().any(pred)
}

fn has_side_effect(effects: &[SideEffect], pred: impl Fn(&SideEffect) -> bool) -> bool {
    effects.iter().any(pred)
}

// =========================================================================
// 1. Phase transitions: Idle → Prompt → Idle
// =========================================================================

#[test]
fn prompt_submit_from_idle_transitions_to_prompt() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    let out = reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );

    // Phase should be Prompt
    assert_eq!(rt.phase_view(), SessionPhaseView::Prompt);
    assert!(matches!(
        rt.phase,
        SessionPhase::Prompt {
            cancelling: false,
            ..
        }
    ));

    assert!(has_event(&out.events, |e| matches!(
        e,
        ClientEvent::SessionPhaseChanged(SessionPhaseView::Prompt)
    )));

    // Should produce a SendPrompt side effect
    assert!(has_side_effect(&out.side_effects, |e| matches!(
        e,
        SideEffect::SendPrompt { .. }
    )));
}

#[test]
fn prompt_response_transitions_to_idle() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    // First, submit a prompt to get into Prompt phase
    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );
    assert_eq!(rt.phase_view(), SessionPhaseView::Prompt);

    // Now the response arrives
    let out = reduce(
        &mut rt,
        InboundEvent::PromptResponse {
            stop_reason: acp::StopReason::EndTurn,
        },
        &mut norm,
    );

    // Phase should be Idle
    assert_eq!(rt.phase_view(), SessionPhaseView::Idle);
    assert_eq!(rt.phase, SessionPhase::Idle);
    assert!(rt.active.is_none());

    assert!(has_event(&out.events, |e| matches!(
        e,
        ClientEvent::PromptCompleted(_)
    )));
}

// =========================================================================
// 2. Cancel semantics
// =========================================================================

#[test]
fn cancel_sets_cancelling_but_does_not_end_turn() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );
    let out = reduce(&mut rt, InboundEvent::CancelSubmit, &mut norm);

    // Phase should be Prompt { cancelling: true }
    assert_eq!(rt.phase_view(), SessionPhaseView::Cancelling);
    assert!(matches!(
        rt.phase,
        SessionPhase::Prompt {
            cancelling: true,
            ..
        }
    ));

    assert!(has_event(&out.events, |e| matches!(
        e,
        ClientEvent::SessionPhaseChanged(SessionPhaseView::Cancelling)
    )));

    // Should produce a SendCancel side effect
    assert!(has_side_effect(&out.side_effects, |e| matches!(
        e,
        SideEffect::SendCancel
    )));

    // Active request should still exist
    assert!(rt.active.is_some());
}

#[test]
fn double_cancel_is_noop() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );
    reduce(&mut rt, InboundEvent::CancelSubmit, &mut norm);
    let out = reduce(&mut rt, InboundEvent::CancelSubmit, &mut norm);

    // No additional events or side effects
    assert!(out.events.is_empty());
    assert!(out.side_effects.is_empty());
}

#[test]
fn cancelled_prompt_response_completes_turn() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );
    reduce(&mut rt, InboundEvent::CancelSubmit, &mut norm);
    let out = reduce(
        &mut rt,
        InboundEvent::PromptResponse {
            stop_reason: acp::StopReason::Cancelled,
        },
        &mut norm,
    );

    assert_eq!(rt.phase_view(), SessionPhaseView::Idle);
    assert!(rt.active.is_none());
    assert!(has_event(&out.events, |e| matches!(
        e,
        ClientEvent::PromptCompleted(_)
    )));
}

// =========================================================================
// 3. Serialized notification processing
// =========================================================================

#[test]
fn open_messages_finalized_into_transcript_on_completion() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );

    // Stream some agent text
    let chunk = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("response text")),
    ));
    reduce(&mut rt, notification(chunk), &mut norm);

    // Complete the turn
    reduce(
        &mut rt,
        InboundEvent::PromptResponse {
            stop_reason: acp::StopReason::EndTurn,
        },
        &mut norm,
    );

    // Transcript should contain the finalized message
    assert!(!rt.persisted.transcript.is_empty());
    assert_eq!(
        rt.persisted.transcript.last().unwrap().content,
        "response text"
    );
}

// =========================================================================
// 4. Out-of-phase content
// =========================================================================

#[test]
fn notification_while_idle_emits_warning() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    let chunk = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
        acp::ContentBlock::Text(acp::TextContent::new("stray content")),
    ));
    let out = reduce(&mut rt, notification(chunk), &mut norm);

    // Should emit a warning
    assert!(has_event(&out.events, |e| matches!(
        e,
        ClientEvent::Warning(_)
    )));

    // Should NOT create an active request
    assert!(rt.active.is_none());
    assert_eq!(rt.phase, SessionPhase::Idle);
}

// =========================================================================
// 5. Queue drain
// =========================================================================

#[test]
fn prompt_submit_while_active_queues() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );
    let out = reduce(
        &mut rt,
        InboundEvent::PromptSubmit(QueuedPrompt {
            event_id: "evt-2".to_string(),
            kind: QueuedPromptKind::User,
            text: "second".to_string(),
            display_text: Some("second".to_string()),
            images: Vec::new(),
        }),
        &mut norm,
    );

    // Second prompt should be queued, not sent
    assert_eq!(rt.queue.len(), 1);
    assert_eq!(rt.queue[0].text, "second");

    // No SendPrompt side effect for the second one
    assert!(!has_side_effect(&out.side_effects, |e| matches!(
        e,
        SideEffect::SendPrompt { .. }
    )));
}

#[test]
fn end_turn_drains_queue() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );
    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(QueuedPrompt {
            event_id: "evt-2".to_string(),
            kind: QueuedPromptKind::User,
            text: "second".to_string(),
            display_text: Some("second".to_string()),
            images: Vec::new(),
        }),
        &mut norm,
    );

    // First prompt completes with EndTurn
    let out = reduce(
        &mut rt,
        InboundEvent::PromptResponse {
            stop_reason: acp::StopReason::EndTurn,
        },
        &mut norm,
    );

    // Queue should be drained
    assert!(rt.queue.is_empty());

    // Should have transitioned directly to a new Prompt phase
    assert_eq!(rt.phase_view(), SessionPhaseView::Prompt);

    assert!(has_event(&out.events, |e| matches!(
        e,
        ClientEvent::SessionPhaseChanged(SessionPhaseView::Prompt)
    )));

    // Should have a SendPrompt side effect for the queued prompt
    assert!(has_side_effect(&out.side_effects, |e| matches!(
        e,
        SideEffect::SendPrompt { .. }
    )));
}

#[test]
fn cancelled_does_not_drain_queue() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );
    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(QueuedPrompt {
            event_id: "evt-2".to_string(),
            kind: QueuedPromptKind::User,
            text: "second".to_string(),
            display_text: Some("second".to_string()),
            images: Vec::new(),
        }),
        &mut norm,
    );

    reduce(&mut rt, InboundEvent::CancelSubmit, &mut norm);
    reduce(
        &mut rt,
        InboundEvent::PromptResponse {
            stop_reason: acp::StopReason::Cancelled,
        },
        &mut norm,
    );

    // Queue should NOT be drained
    assert_eq!(rt.queue.len(), 1);
    assert_eq!(rt.phase_view(), SessionPhaseView::Idle);
}

// =========================================================================
// 6. Tool snapshot ownership
// =========================================================================

#[test]
fn tool_call_gets_owner_request_id() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );
    let request_id = match &rt.phase {
        SessionPhase::Prompt { request_id, .. } => request_id.clone(),
        _ => panic!("expected Prompt phase"),
    };

    let mut tool_call = acp::ToolCall::new(
        acp::ToolCallId::from("tc-1".to_string()),
        "Read /tmp/test.rs".to_string(),
    );
    tool_call.kind = acp::ToolKind::Read;
    let update = acp::SessionUpdate::ToolCall(tool_call);
    reduce(&mut rt, notification(update), &mut norm);

    let snapshot = rt
        .persisted
        .tool_calls
        .get("tc-1")
        .expect("tool should exist");
    assert_eq!(
        snapshot.owner_request_id.as_deref(),
        Some(request_id.as_str())
    );
}

#[test]
fn cancel_marks_active_tools_cancelled() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );

    // Create a non-generic tool call (title with `/` so normalizer emits it)
    let mut tool_call = acp::ToolCall::new(
        acp::ToolCallId::from("tc-1".to_string()),
        "Read /tmp/test.rs".to_string(),
    );
    tool_call.kind = acp::ToolKind::Read;
    reduce(
        &mut rt,
        notification(acp::SessionUpdate::ToolCall(tool_call)),
        &mut norm,
    );

    // Cancel
    reduce(&mut rt, InboundEvent::CancelSubmit, &mut norm);

    let snapshot = rt
        .persisted
        .tool_calls
        .get("tc-1")
        .expect("tool should exist");
    assert_eq!(snapshot.phase, nori_protocol::ToolPhase::Failed);
}

// =========================================================================
// 7. Permission request lifecycle
// =========================================================================

#[test]
fn permission_during_prompt_is_resolved_on_cancel() {
    // The observable behavior of recording a permission request is that
    // it gets resolved as cancelled when a cancel arrives.
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );
    reduce(
        &mut rt,
        InboundEvent::PermissionRequest {
            request_id: "perm-1".to_string(),
            call_id: "tc-1".to_string(),
        },
        &mut norm,
    );
    reduce(
        &mut rt,
        InboundEvent::PermissionRequest {
            request_id: "perm-2".to_string(),
            call_id: "tc-2".to_string(),
        },
        &mut norm,
    );

    let out = reduce(&mut rt, InboundEvent::CancelSubmit, &mut norm);

    // Both permissions should be resolved as cancelled
    let cancelled_ids: Vec<&str> = out
        .side_effects
        .iter()
        .filter_map(|e| match e {
            SideEffect::ResolvePermissionCancelled { request_id } => Some(request_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(cancelled_ids.len(), 2);
    assert!(cancelled_ids.contains(&"perm-1"));
    assert!(cancelled_ids.contains(&"perm-2"));
}

#[test]
fn permission_request_while_idle_emits_warning() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    let out = reduce(
        &mut rt,
        InboundEvent::PermissionRequest {
            request_id: "perm-1".to_string(),
            call_id: "tc-1".to_string(),
        },
        &mut norm,
    );

    assert!(has_event(&out.events, |e| matches!(
        e,
        ClientEvent::Warning(_)
    )));
}

// =========================================================================
// 8. Message assembly
// =========================================================================

#[test]
fn multiple_chunks_assembled_into_one_transcript_entry() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );

    for text in ["hello ", "world", "!"] {
        let chunk = acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(
            acp::ContentBlock::Text(acp::TextContent::new(text)),
        ));
        reduce(&mut rt, notification(chunk), &mut norm);
    }

    reduce(
        &mut rt,
        InboundEvent::PromptResponse {
            stop_reason: acp::StopReason::EndTurn,
        },
        &mut norm,
    );

    // Should have exactly one agent message in transcript
    let agent_messages: Vec<_> = rt
        .persisted
        .transcript
        .iter()
        .filter(|m| m.role == nori_protocol::session_runtime::TranscriptRole::Agent)
        .collect();
    assert_eq!(agent_messages.len(), 1);
    assert_eq!(agent_messages[0].content, "hello world!");
}

// =========================================================================
// 9. Load lifecycle
// =========================================================================

#[test]
fn load_transitions_idle_to_loading_and_back() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    reduce(
        &mut rt,
        InboundEvent::LoadSubmit {
            request_id: "load-1".to_string(),
        },
        &mut norm,
    );

    assert_eq!(rt.phase_view(), SessionPhaseView::Loading);
    assert!(rt.active.is_some());
    let active = rt.active.as_ref().unwrap();
    assert_eq!(active.kind, ActiveRequestKind::Loading);

    // Load response
    reduce(&mut rt, InboundEvent::LoadResponse, &mut norm);

    assert_eq!(rt.phase_view(), SessionPhaseView::Idle);
    assert!(rt.active.is_none());
}

#[test]
fn load_does_not_drain_queue() {
    let mut rt = new_runtime();
    let mut norm = new_normalizer();

    // Queue a prompt, then start a load
    // (Unusual but possible if load happens before queue is drained)
    rt.queue.push_back(simple_prompt());

    reduce(
        &mut rt,
        InboundEvent::LoadSubmit {
            request_id: "load-1".to_string(),
        },
        &mut norm,
    );
    reduce(&mut rt, InboundEvent::LoadResponse, &mut norm);

    // Queue should NOT be drained by load completion
    assert_eq!(rt.queue.len(), 1);
}

// =========================================================================
// 10. Session metadata in any phase
// =========================================================================

#[test]
fn available_commands_update_accepted_in_any_phase() {
    let mut norm = new_normalizer();

    let cmd = acp::AvailableCommand::new("/test", "A test command");
    let update =
        acp::SessionUpdate::AvailableCommandsUpdate(acp::AvailableCommandsUpdate::new(vec![cmd]));

    // Test in Idle
    let mut rt = new_runtime();
    let out = reduce(&mut rt, notification(update.clone()), &mut norm);
    assert_eq!(rt.persisted.available_commands.len(), 1);
    assert!(has_event(&out.events, |e| matches!(
        e,
        ClientEvent::AgentCommandsUpdate(_)
    )));

    // Test during Prompt
    let mut rt = new_runtime();
    reduce(
        &mut rt,
        InboundEvent::PromptSubmit(simple_prompt()),
        &mut norm,
    );
    let out = reduce(&mut rt, notification(update), &mut norm);
    assert_eq!(rt.persisted.available_commands.len(), 1);
    assert!(has_event(&out.events, |e| matches!(
        e,
        ClientEvent::AgentCommandsUpdate(_)
    )));
}
