//! A session's status follows its turns: working while one runs, blocked while
//! a permission request waits, then done, error or idle by how it ended.
use super::*;
use crate::program_status::ProgramState;
use nori_protocol::AcpEvent;
use nori_protocol::ExternalTurn;
use nori_protocol::ExternalTurnState;
use nori_protocol::NoriEvent;
use nori_protocol::SessionPhase;
use nori_protocol::acp::v1 as acp;
use pretty_assertions::assert_eq;

fn phase(phase: SessionPhase) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::SessionPhaseChanged(phase))
}

fn prompting(id: i64) -> SessionEvent {
    phase(SessionPhase::Prompting {
        request_id: acp::RequestId::Number(id),
    })
}

fn cancelling(id: i64) -> SessionEvent {
    phase(SessionPhase::Cancelling {
        request_id: acp::RequestId::Number(id),
    })
}

fn idle() -> SessionEvent {
    phase(SessionPhase::Idle)
}

fn stopped(id: i64, stop_reason: acp::StopReason) -> SessionEvent {
    SessionEvent::Acp(AcpEvent::Response {
        request_id: acp::RequestId::Number(id),
        response: Ok(acp::AgentResponse::PromptResponse(
            acp::PromptResponse::new(stop_reason),
        )),
    })
}

fn permission(id: i64, title: Option<&str>) -> SessionEvent {
    SessionEvent::Acp(AcpEvent::Request {
        request_id: acp::RequestId::Number(id),
        request: acp::AgentRequest::RequestPermissionRequest(acp::RequestPermissionRequest::new(
            acp::SessionId::new("synthetic-session"),
            acp::ToolCallUpdate::new(
                acp::ToolCallId::new(format!("call-{id}")),
                acp::ToolCallUpdateFields::new().title(title.map(str::to_string)),
            ),
            Vec::new(),
        )),
    })
}

fn resolved(id: i64) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::RequestResolved(nori_protocol::RequestResolved {
        request_id: acp::RequestId::Number(id),
        answered_by: "client".to_string(),
    }))
}

fn failed(id: i64) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::RequestFailed(nori_protocol::RequestFailure {
        request_id: Some(acp::RequestId::Number(id)),
        message: "synthetic failure".to_string(),
        kind: nori_protocol::RequestFailureKind::Fatal,
    }))
}

fn external(state: ExternalTurnState) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::ExternalTurnChanged(ExternalTurn {
        id: "external-1".to_string(),
        state,
    }))
}

fn delegation(status: nori_protocol::DelegationStatus) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::DelegationChanged(nori_protocol::Delegation {
        request_id: "delegation-1".to_string(),
        parent_id: "parent-session".to_string(),
        child_id: "child-session".to_string(),
        parent_agent: "mock-model".to_string(),
        agent: "mock-model-alt".to_string(),
        task: "Summarize the tide tables\nthen chart them".to_string(),
        status,
        depth: 1,
        result: None,
        origin: nori_protocol::DelegationOrigin::User,
    }))
}

fn ended(reason: nori_protocol::SessionEndReason) -> SessionEvent {
    SessionEvent::Nori(NoriEvent::SessionEnded(nori_protocol::SessionEnded {
        reason,
        message: None,
    }))
}

/// The states seen after each event, starting from a fresh session.
fn states_after(events: Vec<SessionEvent>) -> Vec<ProgramState> {
    let mut tracker = ProgramStatusTracker::default();
    let mut seen = vec![tracker.status().state];
    for event in &events {
        tracker.observe(event);
        seen.push(tracker.status().state);
    }
    seen
}

#[test]
fn a_successful_turn_works_then_stays_done() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            stopped(1, acp::StopReason::EndTurn),
            idle()
        ]),
        vec![Idle, Working, Done, Done]
    );
}

#[test]
fn a_pending_permission_blocks_the_turn_until_it_is_answered() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            permission(7, Some("Run cargo test")),
            resolved(7),
            stopped(1, acp::StopReason::EndTurn),
        ]),
        vec![Idle, Working, Blocked, Working, Done]
    );
}

#[test]
fn every_open_permission_must_be_answered_before_the_turn_resumes() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            permission(7, Some("first")),
            permission(8, Some("second")),
            resolved(7),
            resolved(8),
        ]),
        vec![Idle, Working, Blocked, Blocked, Blocked, Working]
    );
}

#[test]
fn a_blocked_status_carries_the_oldest_open_approval_title() {
    let mut tracker = ProgramStatusTracker::default();
    for event in [
        prompting(1),
        permission(7, Some("Run cargo test")),
        permission(8, Some("Edit synthetic.rs")),
    ] {
        tracker.observe(&event);
    }
    assert_eq!(
        tracker.status(),
        ProgramStatus {
            state: ProgramState::Blocked,
            approval_title: Some("Run cargo test".to_string()),
        }
    );

    tracker.observe(&resolved(7));
    assert_eq!(
        tracker.status().approval_title,
        Some("Edit synthetic.rs".to_string())
    );
}

#[test]
fn a_cancelled_turn_settles_idle_and_drops_its_open_permissions() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            permission(7, None),
            cancelling(1),
            stopped(1, acp::StopReason::Cancelled),
            idle(),
        ]),
        vec![Idle, Working, Blocked, Working, Idle, Idle]
    );
}

#[test]
fn a_failed_prompt_reports_an_error() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![prompting(1), failed(1), idle()]),
        vec![Idle, Working, Error, Error]
    );
}

#[test]
fn an_error_response_to_the_prompt_reports_an_error() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            SessionEvent::Acp(AcpEvent::Response {
                request_id: acp::RequestId::Number(1),
                response: Err(acp::Error::new(-32603, "synthetic".to_string())),
            }),
            idle(),
        ]),
        vec![Idle, Working, Error, Error]
    );
}

#[test]
fn a_failure_of_another_request_leaves_the_turn_running() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![prompting(1), failed(99)]),
        vec![Idle, Working, Working]
    );
}

#[test]
fn a_turn_that_goes_idle_before_its_response_arrives_is_done() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            idle(),
            stopped(1, acp::StopReason::Cancelled)
        ]),
        vec![Idle, Working, Done, Idle]
    );
}

#[test]
fn a_cancelled_turn_that_goes_idle_before_its_response_never_reports_done() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            cancelling(1),
            idle(),
            stopped(1, acp::StopReason::Cancelled),
        ]),
        vec![Idle, Working, Working, Idle, Idle]
    );
}

#[test]
fn a_failure_reported_after_the_turn_went_idle_is_an_error() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![prompting(1), idle(), failed(1)]),
        vec![Idle, Working, Done, Error]
    );
}

#[test]
fn a_refusal_counts_as_a_finished_turn() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![prompting(1), stopped(1, acp::StopReason::Refusal)]),
        vec![Idle, Working, Done]
    );
}

#[test]
fn an_agent_started_turn_works_then_finishes() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            external(ExternalTurnState::Working),
            external(ExternalTurnState::Idle),
            external(ExternalTurnState::Working),
            external(ExternalTurnState::Cancelled),
        ]),
        vec![Idle, Working, Done, Working, Idle]
    );
}

#[test]
fn an_agent_that_stops_unexpectedly_reports_an_error() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            permission(7, None),
            ended(nori_protocol::SessionEndReason::ConnectionLost),
        ]),
        vec![Idle, Working, Blocked, Error]
    );
}

#[test]
fn a_session_shut_down_after_its_turn_keeps_the_outcome() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            stopped(1, acp::StopReason::EndTurn),
            ended(nori_protocol::SessionEndReason::Shutdown),
        ]),
        vec![Idle, Working, Done, Done]
    );
}

#[test]
fn a_session_closed_mid_turn_goes_idle() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            permission(7, None),
            ended(nori_protocol::SessionEndReason::Closed),
        ]),
        vec![Idle, Working, Blocked, Idle]
    );
}

#[test]
fn a_restored_transcript_starts_the_status_over() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            stopped(1, acp::StopReason::EndTurn),
            SessionEvent::Nori(NoriEvent::ReplayStarted(nori_protocol::ReplayStarted {
                source: nori_protocol::ReplaySource::Transcript,
            })),
        ]),
        vec![Idle, Working, Done, Idle]
    );
}

#[test]
fn a_reply_to_another_request_with_the_same_id_keeps_the_approval_open() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            prompting(1),
            permission(7, None),
            SessionEvent::Acp(AcpEvent::Response {
                request_id: acp::RequestId::Number(7),
                response: Ok(acp::AgentResponse::SetSessionModeResponse(
                    acp::SetSessionModeResponse::new(),
                )),
            }),
        ]),
        vec![Idle, Working, Blocked, Blocked]
    );
}

#[test]
fn a_delegation_waiting_for_approval_blocks_until_it_is_answered() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            delegation(nori_protocol::DelegationStatus::AwaitingApproval),
            idle(),
            delegation(nori_protocol::DelegationStatus::Running),
        ]),
        vec![Idle, Blocked, Blocked, Idle]
    );
}

#[test]
fn a_blocked_delegation_names_the_agent_and_the_first_line_of_its_task() {
    let mut tracker = ProgramStatusTracker::default();
    tracker.observe(&delegation(
        nori_protocol::DelegationStatus::AwaitingApproval,
    ));
    assert_eq!(
        tracker.status(),
        ProgramStatus {
            state: ProgramState::Blocked,
            approval_title: Some("Start mock-model-alt: Summarize the tide tables".to_string()),
        }
    );
}

#[test]
fn a_superseded_agent_turn_stops_working_without_settling() {
    use ProgramState::*;
    assert_eq!(
        states_after(vec![
            external(ExternalTurnState::Working),
            external(ExternalTurnState::Idle),
            external(ExternalTurnState::Working),
            external(ExternalTurnState::Superseded),
        ]),
        vec![Idle, Working, Done, Working, Done]
    );
}
