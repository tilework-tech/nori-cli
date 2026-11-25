// The status an attached terminal reports is the selected session's, built
// from the envelopes the host publishes for it.

fn envelope(session: &str, seq: i64, event: nori_protocol::SessionEvent) -> SessionEnvelope {
    SessionEnvelope {
        conversation_id: session.into(),
        agent: "test-agent".into(),
        seq,
        event,
    }
}

fn prompting_envelope(session: &str, seq: i64) -> SessionEnvelope {
    envelope(
        session,
        seq,
        nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::SessionPhaseChanged(
            nori_protocol::SessionPhase::Prompting {
                request_id: nori_protocol::acp::v1::RequestId::Number(1),
            },
        )),
    )
}

fn finished_envelope(session: &str, seq: i64) -> SessionEnvelope {
    use nori_protocol::acp::v1 as acp;
    envelope(
        session,
        seq,
        nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Response {
            request_id: acp::RequestId::Number(1),
            response: Ok(acp::AgentResponse::PromptResponse(acp::PromptResponse::new(
                acp::StopReason::EndTurn,
            ))),
        }),
    )
}

fn permission_envelope(session: &str, seq: i64, title: &str) -> SessionEnvelope {
    use nori_protocol::acp::v1 as acp;
    envelope(
        session,
        seq,
        nori_protocol::SessionEvent::Acp(nori_protocol::AcpEvent::Request {
            request_id: acp::RequestId::Number(9),
            request: acp::AgentRequest::RequestPermissionRequest(
                acp::RequestPermissionRequest::new(
                    acp::SessionId::new("agent-session"),
                    acp::ToolCallUpdate::new(
                        acp::ToolCallId::new("call-9"),
                        acp::ToolCallUpdateFields::new().title(title.to_string()),
                    ),
                    Vec::new(),
                ),
            ),
        }),
    )
}

fn shown_state(state: &AttachState) -> Option<crate::program_status::ProgramState> {
    state.foreground_program_status().map(|status| status.state)
}

#[test]
fn the_reported_status_follows_the_selected_session() {
    use crate::program_status::ProgramState;
    let mut state = AttachState::default();
    state.apply(prompting_envelope("alpha", 1)).unwrap();
    state.apply(permission_envelope("alpha", 2, "Run cargo test")).unwrap();
    state.apply(prompting_envelope("beta", 1)).unwrap();
    state.apply(finished_envelope("beta", 2)).unwrap();

    state.selected = Some("alpha".into());
    let alpha = state.foreground_program_status();
    state.selected = Some("beta".into());
    let beta = shown_state(&state);

    assert_eq!(
        (alpha, beta),
        (
            Some(crate::program_status::ProgramStatus {
                state: ProgramState::Blocked,
                approval_title: Some("Run cargo test".to_string()),
            }),
            Some(ProgramState::Done),
        )
    );
}

#[test]
fn a_selected_session_reports_nothing_until_its_events_arrive() {
    let mut state = AttachState {
        selected: Some("alpha".into()),
        ..Default::default()
    };
    assert_eq!(shown_state(&state), None);

    state.apply(prompting_envelope("alpha", 1)).unwrap();
    assert_eq!(
        shown_state(&state),
        Some(crate::program_status::ProgramState::Working)
    );
}

#[test]
fn with_no_session_open_the_terminal_reports_idle() {
    let state = AttachState::default();
    assert_eq!(
        shown_state(&state),
        Some(crate::program_status::ProgramState::Idle)
    );
}
