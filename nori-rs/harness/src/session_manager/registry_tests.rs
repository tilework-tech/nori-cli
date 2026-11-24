use std::path::PathBuf;

use nori_config::AskForApproval;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::RequestResolved;
use nori_protocol::SessionEndReason;
use nori_protocol::SessionEnded;
use nori_protocol::SessionEvent;
use nori_protocol::SessionForked;
use nori_protocol::SessionPhase;
use nori_protocol::SessionStarted;
use nori_protocol::SessionStatus;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;

use super::AgentProcessId;
use super::Registry;
use super::SessionSettings;
use crate::ConversationId;
use crate::session_manager::SessionRecord;
use crate::session_manager::SessionSource;
use crate::session_manager::StartError;

fn settings(agent: &str, parent: Option<ConversationId>) -> SessionSettings {
    SessionSettings {
        conversation_id: ConversationId::new(),
        agent: agent.to_string(),
        parent,
        source: if parent.is_some() {
            SessionSource::Session
        } else {
            SessionSource::User
        },
        resumed_from: None,
        cwd: PathBuf::from("/work"),
        model: Some("default-model".to_string()),
        approval_policy: AskForApproval::OnRequest,
    }
}

fn start(
    registry: &mut Registry,
    agent: &str,
    parent: Option<ConversationId>,
) -> (ConversationId, AgentProcessId) {
    let settings = settings(agent, parent);
    let id = settings.conversation_id;
    let reservation = registry.reserve().expect("slot available");
    let process = registry
        .register(settings, reservation)
        .expect("session registers");
    (id, process)
}

fn started(acp_session_id: &str) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::SessionStarted(SessionStarted {
        transcript_id: None,
        acp_session_id: acp::SessionId::new(acp_session_id),
        cwd: PathBuf::from("/work"),
        transcript_path: Some(PathBuf::from(format!(
            "/transcripts/{acp_session_id}.jsonl"
        ))),
        history_log_id: 0,
        history_entry_count: 0,
        config_options: Vec::new(),
    }))
}

fn prompting(request_id: i64) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Prompting {
        request_id: acp::RequestId::Number(request_id),
    }))
}

fn idle() -> SessionEvent {
    SessionEvent::Nori(NoriEvent::SessionPhaseChanged(SessionPhase::Idle))
}

fn permission(request_id: i64) -> SessionEvent {
    SessionEvent::Acp(AcpEvent::Request {
        request_id: acp::RequestId::Number(request_id),
        request: acp::AgentRequest::RequestPermissionRequest(acp::RequestPermissionRequest::new(
            acp::SessionId::new("agent-session"),
            acp::ToolCallUpdate::new(
                acp::ToolCallId::new("tool-1"),
                acp::ToolCallUpdateFields::new().title("rm -rf target"),
            ),
            vec![acp::PermissionOption::new(
                "allow",
                "Allow",
                acp::PermissionOptionKind::AllowOnce,
            )],
        )),
    })
}

fn agent_text(text: &str) -> SessionEvent {
    SessionEvent::Acp(AcpEvent::Notification(
        acp::AgentNotification::SessionNotification(acp::SessionNotification::new(
            "agent-session",
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text),
            ))),
        )),
    ))
}

fn ended(reason: SessionEndReason) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::SessionEnded(SessionEnded {
        reason,
        message: None,
    }))
}

fn status(registry: &Registry, id: &ConversationId) -> SessionStatus {
    registry.record(id).expect("session is registered").status
}

fn resolved(request_id: i64, answered_by: &str) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::RequestResolved(RequestResolved {
        request_id: acp::RequestId::Number(request_id),
        answered_by: answered_by.to_string(),
    }))
}

fn pending(registry: &Registry, id: &ConversationId) -> Vec<acp::RequestId> {
    registry
        .record(id)
        .map(|record| record.pending_requests)
        .unwrap_or_default()
}

#[test]
fn envelopes_number_each_conversation_from_one_without_gaps() {
    let mut registry = Registry::new(4);
    let (a, process_a) = start(&mut registry, "claude", None);
    let (b, process_b) = start(&mut registry, "codex", None);

    let mut seen = Vec::new();
    for (process, event) in [
        (process_a, started("a")),
        (process_b, started("b")),
        (process_a, prompting(1)),
        (process_a, agent_text("hello from a")),
        (process_b, prompting(1)),
        (process_a, idle()),
    ] {
        let envelope = registry.observe(process, event).expect("known process");
        seen.push((envelope.conversation_id, envelope.agent, envelope.seq));
    }

    assert_eq!(
        seen,
        vec![
            (a.to_string(), "claude".to_string(), 1),
            (b.to_string(), "codex".to_string(), 1),
            (a.to_string(), "claude".to_string(), 2),
            (a.to_string(), "claude".to_string(), 3),
            (b.to_string(), "codex".to_string(), 2),
            (a.to_string(), "claude".to_string(), 4),
        ]
    );
}

#[test]
fn status_follows_start_turns_and_permission_requests() {
    let mut registry = Registry::new(4);
    let (id, process) = start(&mut registry, "claude", None);
    let mut statuses = vec![status(&registry, &id)];

    registry.observe(process, started("a"));
    statuses.push(status(&registry, &id));
    registry.observe(process, prompting(1));
    statuses.push(status(&registry, &id));
    registry.observe(process, permission(7));
    statuses.push(status(&registry, &id));
    registry.observe(process, resolved(7, "client"));
    statuses.push(status(&registry, &id));
    registry.observe(process, idle());
    statuses.push(status(&registry, &id));

    assert_eq!(
        statuses,
        vec![
            SessionStatus::Starting,
            SessionStatus::Ready,
            SessionStatus::Working,
            SessionStatus::WaitingOnYou,
            SessionStatus::Working,
            SessionStatus::Ready,
        ]
    );
}

#[test]
fn a_finished_turn_drops_requests_nobody_answered() {
    let mut registry = Registry::new(4);
    let (id, process) = start(&mut registry, "claude", None);
    registry.observe(process, started("a"));
    registry.observe(process, prompting(1));
    registry.observe(process, permission(7));

    registry.observe(process, idle());

    assert_eq!(status(&registry, &id), SessionStatus::Ready);
}

#[test]
fn the_end_reason_decides_between_failed_and_closed() {
    let cases = [
        (SessionEndReason::ConnectionLost, SessionStatus::Failed),
        (SessionEndReason::SpawnFailed, SessionStatus::Failed),
        (SessionEndReason::TimedOut, SessionStatus::Failed),
        (SessionEndReason::Shutdown, SessionStatus::Closed),
        (SessionEndReason::Closed, SessionStatus::Closed),
    ];
    let mut results = Vec::new();
    for (reason, _) in cases {
        let mut registry = Registry::new(4);
        let (id, process) = start(&mut registry, "claude", None);
        registry.observe(process, started("a"));
        registry.observe(process, ended(reason));
        registry.process_exited(process);
        results.push((reason, status(&registry, &id)));
    }

    assert_eq!(results, cases.to_vec());
}

#[test]
fn a_stream_that_closes_without_an_end_event_is_a_failure() {
    let mut registry = Registry::new(4);
    let (id, process) = start(&mut registry, "claude", None);
    registry.observe(process, started("a"));
    registry.observe(process, prompting(1));

    registry.process_exited(process);

    assert_eq!(status(&registry, &id), SessionStatus::Failed);
    assert_eq!(registry.live_process(&id), None);
}

#[test]
fn one_dead_agent_process_fails_only_its_own_session() {
    let mut registry = Registry::new(2);
    let (a, process_a) = start(&mut registry, "claude", None);
    let (b, process_b) = start(&mut registry, "codex", None);
    for process in [process_a, process_b] {
        registry.observe(process, started("s"));
        registry.observe(process, prompting(1));
    }
    let last_b = registry
        .observe(process_b, agent_text("before"))
        .expect("b is live")
        .seq;

    registry.observe(process_a, ended(SessionEndReason::ConnectionLost));
    registry.process_exited(process_a);
    let next_b = registry
        .observe(process_b, agent_text("still going"))
        .expect("b keeps its process");

    assert_eq!(
        (
            status(&registry, &a),
            status(&registry, &b),
            next_b.seq,
            registry.live_process(&a),
            registry.live_process(&b),
        ),
        (
            SessionStatus::Failed,
            SessionStatus::Working,
            last_b + 1,
            None,
            Some(process_b),
        )
    );
    assert!(
        registry.reserve().is_ok(),
        "the failed session's slot is free again"
    );
}

#[test]
fn the_cap_refuses_extra_sessions_until_one_ends() {
    let mut registry = Registry::new(2);
    let (_, first) = start(&mut registry, "claude", None);
    start(&mut registry, "codex", None);

    let refused = registry.reserve().err();
    registry.observe(first, ended(SessionEndReason::Shutdown));
    let after_end = registry.reserve().is_ok();

    assert_eq!(
        (refused, after_end),
        (Some(StartError::CapacityReached { limit: 2 }), true)
    );
}

#[test]
fn an_unused_reservation_gives_its_slot_back() {
    let registry = Registry::new(1);
    let reservation = registry.reserve().expect("slot available");
    assert_eq!(
        registry.reserve().err(),
        Some(StartError::CapacityReached { limit: 1 })
    );

    drop(reservation);

    assert!(registry.reserve().is_ok());
}

#[test]
fn closing_admission_refuses_new_sessions() {
    let registry = Registry::new(4);

    registry.close_admission();

    assert_eq!(registry.reserve().err(), Some(StartError::ShuttingDown));
}

#[test]
fn a_child_needs_a_parent_that_is_registered() {
    let mut registry = Registry::new(1);
    let missing = ConversationId::new();
    let reservation = registry.reserve().expect("slot available");

    let result = registry.register(settings("pi", Some(missing)), reservation);

    assert_eq!(result.err(), Some(StartError::UnknownParent(missing)));
    assert!(
        registry.reserve().is_ok(),
        "a refused session gives its slot back"
    );
}

#[test]
fn a_family_is_waiting_on_you_when_any_member_does() {
    let mut registry = Registry::new(4);
    let (parent, parent_process) = start(&mut registry, "codex", None);
    let (child, child_process) = start(&mut registry, "pi", Some(parent));
    let (other_child, other_process) = start(&mut registry, "pi", Some(parent));
    for process in [parent_process, child_process, other_process] {
        registry.observe(process, started("s"));
    }
    registry.observe(other_process, prompting(1));
    registry.observe(child_process, prompting(1));
    registry.observe(child_process, permission(3));

    assert_eq!(
        (
            registry.family_status(&parent),
            registry.family_status(&child),
            registry.family_status(&other_child),
            status(&registry, &parent),
        ),
        (
            Some(SessionStatus::WaitingOnYou),
            Some(SessionStatus::WaitingOnYou),
            Some(SessionStatus::WaitingOnYou),
            SessionStatus::Ready,
        )
    );
}

#[test]
fn family_rollup_prefers_attention_over_progress() {
    let mut registry = Registry::new(4);
    let (parent, parent_process) = start(&mut registry, "codex", None);
    let (_, failing_child) = start(&mut registry, "pi", Some(parent));
    let (_, working_child) = start(&mut registry, "pi", Some(parent));
    for process in [parent_process, failing_child, working_child] {
        registry.observe(process, started("s"));
    }
    registry.observe(working_child, prompting(1));
    let working = registry.family_status(&parent);

    registry.observe(failing_child, ended(SessionEndReason::ConnectionLost));
    let failed = registry.family_status(&parent);

    assert_eq!(
        (working, failed),
        (Some(SessionStatus::Working), Some(SessionStatus::Failed))
    );
}

#[test]
fn a_waiting_request_outranks_a_failed_member() {
    let mut registry = Registry::new(4);
    let (parent, parent_process) = start(&mut registry, "codex", None);
    let (_, failed_child) = start(&mut registry, "pi", Some(parent));
    for process in [parent_process, failed_child] {
        registry.observe(process, started("s"));
    }
    registry.observe(failed_child, ended(SessionEndReason::ConnectionLost));
    registry.observe(parent_process, prompting(1));
    registry.observe(parent_process, permission(4));

    assert_eq!(
        registry.family_status(&parent),
        Some(SessionStatus::WaitingOnYou)
    );
}

#[test]
fn a_family_is_closed_only_when_every_member_is() {
    let mut registry = Registry::new(4);
    let (parent, parent_process) = start(&mut registry, "codex", None);
    let (_, child_process) = start(&mut registry, "pi", Some(parent));
    registry.observe(parent_process, ended(SessionEndReason::Shutdown));
    let child_open = registry.family_status(&parent);

    registry.observe(child_process, ended(SessionEndReason::Shutdown));

    assert_eq!(
        (child_open, registry.family_status(&parent)),
        (Some(SessionStatus::Starting), Some(SessionStatus::Closed))
    );
}

#[test]
fn requests_are_answered_per_session() {
    let mut registry = Registry::new(4);
    let (a, process_a) = start(&mut registry, "claude", None);
    let (b, process_b) = start(&mut registry, "codex", None);
    for process in [process_a, process_b] {
        registry.observe(process, started("s"));
        registry.observe(process, prompting(1));
        registry.observe(process, permission(0));
    }

    registry.observe(process_a, resolved(0, "client"));

    assert_eq!(
        (status(&registry, &a), status(&registry, &b)),
        (SessionStatus::Working, SessionStatus::WaitingOnYou)
    );
}

#[test]
fn an_answered_request_is_announced_once_in_sequence() {
    let mut registry = Registry::new(4);
    let (id, process) = start(&mut registry, "claude", None);
    registry.observe(process, started("s"));
    registry.observe(process, prompting(1));
    let request = registry
        .observe(process, permission(7))
        .expect("known process");

    let announced = registry
        .observe(process, resolved(7, "remote_control"))
        .expect("a pending request is announced when answered");
    let repeated = registry.observe(process, resolved(7, "client"));
    let next = registry
        .observe(process, agent_text("after"))
        .expect("known process");

    assert_eq!(
        (
            (announced.conversation_id, announced.seq),
            match &announced.event {
                SessionEvent::Nori(NoriEvent::RequestResolved(resolved)) => Some(resolved.clone()),
                _ => None,
            },
            repeated.map(|envelope| envelope.seq),
            next.seq,
            pending(&registry, &id),
            status(&registry, &id),
        ),
        (
            (id.to_string(), request.seq + 1),
            Some(RequestResolved {
                request_id: acp::RequestId::Number(7),
                answered_by: "remote_control".to_string(),
            }),
            None,
            request.seq + 2,
            Vec::new(),
            SessionStatus::Working,
        )
    );
}

#[test]
fn an_answer_that_arrives_before_its_request_is_announced_after_it() {
    let mut registry = Registry::new(4);
    let (id, process) = start(&mut registry, "claude", None);
    registry.observe_all(process, started("s"));
    registry.observe_all(process, prompting(1));

    let early = registry.observe_all(process, resolved(7, "remote_control"));
    let request = registry.observe_all(process, permission(7));

    let order: Vec<(i64, Option<String>)> = request
        .iter()
        .map(|envelope| {
            let answered_by = match &envelope.event {
                SessionEvent::Nori(NoriEvent::RequestResolved(resolved)) => {
                    Some(resolved.answered_by.clone())
                }
                _ => None,
            };
            (envelope.seq, answered_by)
        })
        .collect();
    assert_eq!(
        (
            early.len(),
            order,
            pending(&registry, &id),
            status(&registry, &id)
        ),
        (
            0,
            // The session's mode is announced just before its start.
            vec![(4, None), (5, Some("remote_control".to_string()))],
            Vec::new(),
            SessionStatus::Working,
        )
    );
}

#[test]
fn the_record_lists_open_requests_in_the_order_they_arrived() {
    let mut registry = Registry::new(4);
    let (id, process) = start(&mut registry, "claude", None);
    registry.observe(process, started("s"));
    registry.observe(process, prompting(1));
    registry.observe(process, permission(9));
    registry.observe(process, permission(2));
    let while_waiting = pending(&registry, &id);
    registry.observe(process, resolved(9, "client"));
    let after_one_answer = pending(&registry, &id);
    registry.observe(process, idle());

    assert_eq!(
        (while_waiting, after_one_answer, pending(&registry, &id)),
        (
            vec![acp::RequestId::Number(9), acp::RequestId::Number(2)],
            vec![acp::RequestId::Number(2)],
            Vec::new(),
        )
    );
}

#[test]
fn session_started_fills_in_the_record() {
    let mut registry = Registry::new(4);
    let (id, process) = start(&mut registry, "claude", None);
    let mut started_event = started("acp-a");
    if let SessionEvent::Nori(NoriEvent::SessionStarted(started)) = &mut started_event {
        started.config_options = vec![
            acp::SessionConfigOption::select(
                "model",
                "Model",
                "opus",
                vec![acp::SessionConfigSelectOption::new("opus", "Opus")],
            )
            .category(acp::SessionConfigOptionCategory::Model),
        ];
    }

    registry.observe(process, started_event);

    assert_eq!(
        registry.records(),
        vec![SessionRecord {
            conversation_id: id,
            agent: "claude".to_string(),
            parent: None,
            source: SessionSource::User,
            cwd: PathBuf::from("/work"),
            model: Some("opus".to_string()),
            model_name: Some("Opus".to_string()),
            approval_policy: AskForApproval::OnRequest,
            transcript_path: Some(PathBuf::from("/transcripts/acp-a.jsonl")),
            acp_session_id: Some("acp-a".to_string()),
            pending_requests: Vec::new(),
            status: SessionStatus::Ready,
        }]
    );
}

#[test]
fn a_fork_moves_the_agent_process_to_a_new_child_conversation() {
    let mut registry = Registry::new(1);
    let (parent, process) = start(&mut registry, "claude", None);
    registry.observe(process, started("acp-parent"));
    let child = ConversationId::new();

    let fork_envelope = registry
        .observe(
            process,
            SessionEvent::Nori(NoriEvent::SessionForked(SessionForked {
                previous_conversation_id: parent.to_string(),
                new_conversation_id: child.to_string(),
                new_acp_session_id: acp::SessionId::new("acp-child"),
            })),
        )
        .expect("known process");
    let next = registry
        .observe(process, prompting(2))
        .expect("process still live");

    let child_record = registry.record(&child).expect("fork is registered");
    assert_eq!(
        (
            (fork_envelope.conversation_id, fork_envelope.seq),
            (next.conversation_id, next.seq),
            status(&registry, &parent),
            (
                child_record.parent,
                child_record.source,
                child_record.status
            ),
            child_record.acp_session_id,
            registry.live_process(&parent),
            registry.live_process(&child),
            registry.conversation_of(process),
            registry.reserve().err(),
        ),
        (
            (child.to_string(), 1),
            (child.to_string(), 2),
            SessionStatus::Closed,
            (Some(parent), SessionSource::Fork, SessionStatus::Working),
            Some("acp-child".to_string()),
            None,
            Some(process),
            Some(child),
            Some(StartError::CapacityReached { limit: 1 }),
        )
    );
}

#[test]
fn a_resumed_session_keeps_its_identity_order_children_and_sequence() {
    let mut registry = Registry::new(4);
    let (failed, failed_process) = start(&mut registry, "claude", None);
    let (child, _) = start(&mut registry, "pi", Some(failed));
    registry.observe(failed_process, started("old-acp"));
    let last = registry
        .observe(failed_process, ended(SessionEndReason::ConnectionLost))
        .expect("terminal envelope");
    let mut replacement_settings = settings("claude", None);
    replacement_settings.conversation_id = failed;
    replacement_settings.resumed_from = Some(failed);
    let reservation = registry.reserve().expect("slot available");
    let replacement_process = registry
        .register(replacement_settings, reservation)
        .expect("failed session restarts");
    let restarted = registry
        .observe(replacement_process, started("new-acp"))
        .expect("restart envelope");
    assert_eq!(
        (
            registry
                .records()
                .into_iter()
                .map(|record| (record.conversation_id, record.parent))
                .collect::<Vec<_>>(),
            restarted.conversation_id,
            restarted.seq
        ),
        (
            vec![(failed, None), (child, Some(failed))],
            failed.to_string(),
            last.seq + 1
        ),
    );
}

#[test]
fn stale_process_events_and_eof_cannot_change_a_resumed_session() {
    let mut registry = Registry::new(1);
    let (id, old_process) = start(&mut registry, "claude", None);
    registry.observe(old_process, started("old-acp"));
    registry.observe(old_process, ended(SessionEndReason::ConnectionLost));
    let mut replacement_settings = settings("claude", None);
    replacement_settings.conversation_id = id;
    let reservation = registry.reserve().expect("failed process released slot");
    let new_process = registry
        .register(replacement_settings, reservation)
        .expect("restart");
    assert_ne!(new_process, old_process);
    registry.observe(new_process, started("new-acp"));
    assert!(
        registry
            .observe(old_process, ended(SessionEndReason::ConnectionLost))
            .is_none(),
        "retired process cannot publish into the replacement"
    );
    registry.process_exited(old_process);
    assert_eq!(
        (
            registry
                .record(&id)
                .map(|record| (record.status, record.acp_session_id)),
            registry.live_process(&id),
            registry.free_slots()
        ),
        (
            Some((SessionStatus::Ready, Some("new-acp".to_string()))),
            Some(new_process),
            0
        )
    );
}

#[test]
fn a_live_session_cannot_be_registered_as_a_second_incarnation() {
    let mut registry = Registry::new(2);
    let (id, process) = start(&mut registry, "claude", None);
    registry.observe(process, started("original-acp"));
    let mut duplicate = settings("claude", None);
    duplicate.conversation_id = id;
    let reservation = registry.reserve().expect("second capacity slot exists");
    assert!(
        registry.register(duplicate, reservation).is_err(),
        "one live process owns a session ID"
    );
    assert_eq!(
        (registry.live_process(&id), registry.free_slots()),
        (Some(process), 1)
    );
}

#[test]
fn closing_a_forked_parent_reaches_the_child_that_took_its_process() {
    let mut registry = Registry::new(2);
    let (parent, process) = start(&mut registry, "claude", None);
    registry.observe(process, started("acp-parent"));
    let child = ConversationId::new();
    registry.observe(
        process,
        SessionEvent::Nori(NoriEvent::SessionForked(SessionForked {
            previous_conversation_id: parent.to_string(),
            new_conversation_id: child.to_string(),
            new_acp_session_id: acp::SessionId::new("acp-child"),
        })),
    );

    assert_eq!(
        (
            registry.process_following_forks(&parent),
            registry.process_following_forks(&child),
        ),
        (Some(process), Some(process))
    );
}

#[test]
fn after_a_resume_the_old_ids_lead_to_the_replacement() {
    let mut registry = Registry::new(4);
    let (parent, process) = start(&mut registry, "claude", None);
    registry.observe(process, started("acp-parent"));
    let child = ConversationId::new();
    registry.observe(
        process,
        SessionEvent::Nori(NoriEvent::SessionForked(SessionForked {
            previous_conversation_id: parent.to_string(),
            new_conversation_id: child.to_string(),
            new_acp_session_id: acp::SessionId::new("acp-child"),
        })),
    );
    registry.process_exited(process);
    let mut replacement_settings = settings("claude", Some(parent));
    replacement_settings.source = SessionSource::Fork;
    replacement_settings.resumed_from = Some(child);
    replacement_settings.conversation_id = child;
    let replacement = child;
    let reservation = registry.reserve().expect("slot available");
    let replacement_process = registry
        .register(replacement_settings, reservation)
        .expect("replacement registers");

    assert_eq!(
        (
            registry.process_following_forks(&parent),
            registry.process_following_forks(&child),
            registry.process_following_forks(&replacement),
        ),
        (
            Some(replacement_process),
            Some(replacement_process),
            Some(replacement_process),
        )
    );
}
