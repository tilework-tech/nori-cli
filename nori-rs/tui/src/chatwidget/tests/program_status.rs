//! A widget's program status follows the host events of its own session.
use super::*;
use crate::program_status::ProgramState;
use crate::program_status::ProgramStatus;
use pretty_assertions::assert_eq;

fn prompting(id: i64) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
        nori_protocol::SessionPhase::Prompting {
            request_id: nori_protocol::acp::v1::RequestId::Number(id),
        },
    ))
}

fn permission(id: i64, title: &str) -> nori_protocol::SessionEvent {
    use nori_protocol::acp::v1 as acp;
    nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Request {
        request_id: acp::RequestId::Number(id),
        request: acp::AgentRequest::RequestPermissionRequest(acp::RequestPermissionRequest::new(
            acp::SessionId::new("synthetic-session"),
            acp::ToolCallUpdate::new(
                acp::ToolCallId::new(format!("call-{id}")),
                acp::ToolCallUpdateFields::new().title(title.to_string()),
            ),
            vec![acp::PermissionOption::new(
                "allow",
                "Allow",
                acp::PermissionOptionKind::AllowOnce,
            )],
        )),
    })
}

#[test]
fn the_status_follows_the_widgets_session_events() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    let generation = chat.session_generation;

    chat.handle_session_event(generation, prompting(1));
    let working = chat.program_status.status();
    chat.handle_session_event(generation, permission(5, "Run cargo test"));

    assert_eq!(
        (working.state, chat.program_status.status()),
        (
            ProgramState::Working,
            ProgramStatus {
                state: ProgramState::Blocked,
                approval_title: Some("Run cargo test".to_string()),
            }
        )
    );
}

#[test]
fn events_from_a_replaced_session_do_not_change_the_status() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();
    let stale = chat.session_generation.wrapping_sub(1);
    chat.handle_session_event(stale, prompting(1));
    assert_eq!(chat.program_status.status(), ProgramStatus::IDLE);
}
