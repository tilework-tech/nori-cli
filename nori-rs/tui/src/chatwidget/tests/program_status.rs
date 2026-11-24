//! The OSC 7501 status follows the foreground session: working during a
//! turn, blocked on an approval, and done, error or idle by how the turn
//! ended.
use super::*;
use crate::program_status::ProgramStatus;
use pretty_assertions::assert_eq;

fn chat() -> ChatWidget {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    chat.config.animations = false;
    chat.set_foreground(true);
    chat.pre_draw_tick();
    chat
}

fn complete_turn(
    chat: &mut ChatWidget,
    stop_reason: nori_protocol::acp::v1::StopReason,
    failure: Option<crate::presentation::TurnFailure>,
) {
    chat.handle_client_event(crate::presentation::ClientEvent::PromptCompleted(
        crate::presentation::PromptCompleted {
            stop_reason,
            last_agent_message: Some("All set".to_string()),
            failure,
        },
    ));
    chat.pre_draw_tick();
}

fn approval(id: &str) -> crate::bottom_pane::ApprovalRequest {
    crate::bottom_pane::ApprovalRequest {
        request_id: id.to_string().into(),
        title: "Run cargo test".to_string(),
        kind: crate::presentation::ToolKind::Execute,
        cwd: PathBuf::from("/synthetic/harbor-ledger"),
        snapshot: Box::new(crate::presentation::ToolSnapshot {
            call_id: format!("call-{id}"),
            title: "Run cargo test".to_string(),
            kind: crate::presentation::ToolKind::Execute,
            phase: crate::presentation::ToolPhase::PendingApproval,
            locations: Vec::new(),
            invocation: Some(crate::presentation::Invocation::Command {
                command: "cargo test".to_string(),
            }),
            artifacts: Vec::new(),
            raw_input: None,
            raw_output: None,
            owner_request_id: None,
            details: Default::default(),
        }),
        options: vec![nori_protocol::acp::v1::PermissionOption::new(
            nori_protocol::acp::v1::PermissionOptionId::new("allow-once"),
            "Allow",
            nori_protocol::acp::v1::PermissionOptionKind::AllowOnce,
        )],
    }
}

#[test]
fn a_fresh_session_reports_idle() {
    let chat = chat();
    assert_eq!(chat.last_program_status, Some(ProgramStatus::Idle));
}

#[test]
fn a_turn_works_then_reports_done() {
    let mut chat = chat();

    chat.on_task_started();
    chat.pre_draw_tick();
    assert_eq!(chat.last_program_status, Some(ProgramStatus::Working));

    complete_turn(&mut chat, nori_protocol::acp::v1::StopReason::EndTurn, None);
    assert_eq!(chat.last_program_status, Some(ProgramStatus::Done));
}

#[test]
fn a_cancelled_turn_reports_idle() {
    let mut chat = chat();
    chat.on_task_started();

    complete_turn(
        &mut chat,
        nori_protocol::acp::v1::StopReason::Cancelled,
        None,
    );
    assert_eq!(chat.last_program_status, Some(ProgramStatus::Idle));
}

#[test]
fn a_failed_turn_reports_error() {
    let mut chat = chat();
    chat.on_task_started();

    complete_turn(
        &mut chat,
        nori_protocol::acp::v1::StopReason::EndTurn,
        Some(crate::presentation::TurnFailure::Fatal),
    );
    assert_eq!(chat.last_program_status, Some(ProgramStatus::Error));
}

#[test]
fn a_pending_approval_blocks_until_it_is_answered_elsewhere() {
    let mut chat = chat();
    chat.on_task_started();

    chat.bottom_pane.push_approval_request(approval("1"));
    chat.pre_draw_tick();
    assert_eq!(
        chat.last_program_status,
        Some(ProgramStatus::Blocked {
            message: "Run cargo test".to_string(),
        })
    );

    chat.bottom_pane.dismiss_request(&"1".to_string().into());
    chat.pre_draw_tick();
    assert_eq!(chat.last_program_status, Some(ProgramStatus::Working));
}

#[test]
fn a_parked_session_does_not_report() {
    let mut chat = chat();
    chat.set_foreground(false);
    chat.last_program_status = None;

    chat.on_task_started();
    chat.pre_draw_tick();
    assert_eq!(chat.last_program_status, None);
}
