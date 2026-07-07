//! Cloud session identity surfacing in the ChatWidget. The harness names the
//! session in `SessionConfiguredEvent.acp_session_id` ONLY for cloud
//! (live-reattach) agents and sends `None` for local agents, so id presence
//! is the cloud signal. Tests drive events in PRODUCTION order —
//! SessionConfigured first, capabilities after (their delivery is a scheduler
//! race) — proving the immutable welcome card cannot miss the identity.

use super::*;

/// A SessionConfigured event; `acp_session_id` is `Some` only for cloud
/// sessions, mirroring the harness contract.
fn session_configured(acp_session_id: Option<&str>) -> Event {
    let rollout_file = NamedTempFile::new().unwrap();
    let configured = codex_protocol::protocol::SessionConfiguredEvent {
        session_id: ConversationId::new(),
        model: "test-model".to_string(),
        model_provider_id: "test-provider".to_string(),
        approval_policy: AskForApproval::Never,
        sandbox_policy: SandboxPolicy::ReadOnly,
        cwd: PathBuf::from("/home/user/project"),
        reasoning_effort: Some(ReasoningEffortConfig::default()),
        acp_session_id: acp_session_id.map(str::to_string),
        history_log_id: 0,
        history_entry_count: 0,
        initial_messages: None,
        rollout_path: rollout_file.path().to_path_buf(),
    };
    Event {
        id: "configured".into(),
        msg: EventMsg::SessionConfigured(configured),
    }
}

/// On a cloud session, once SessionConfigured names the session, the /status
/// output must surface that id.
#[test]
fn cloud_status_output_surfaces_session_id() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_codex_event(session_configured(Some("nori-fast-kazunoko-aac8")));
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_capabilities(),
    ));
    // Drop the session-info cell emitted by SessionConfigured.
    let _ = drain_insert_history(&mut rx);

    chat.add_status_output();

    let rendered = cells_to_text(&drain_insert_history(&mut rx));
    assert!(
        rendered.contains("nori-fast-kazunoko-aac8"),
        "cloud status output must surface the cloud session id, got:\n{rendered}"
    );
}

/// Guard: a local agent is never named by the harness (`acp_session_id:
/// None` by contract), so /status must not render a session line.
#[test]
fn local_status_output_hides_session_id() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_codex_event(session_configured(None));
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        local_capabilities(false),
    ));
    let _ = drain_insert_history(&mut rx);

    chat.add_status_output();

    let rendered = cells_to_text(&drain_insert_history(&mut rx));
    assert!(
        !rendered.contains("session:"),
        "a local session must not render cloud session identity, got:\n{rendered}"
    );
}

/// Welcome-banner pipeline in PRODUCTION order: the session-info cell is
/// emitted at SessionConfigured time, BEFORE any capabilities arrive. The
/// card is immutable, so the identity must not depend on capability delivery
/// order — id presence alone carries it.
#[test]
fn welcome_card_pipeline_surfaces_cloud_session_id() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_codex_event(session_configured(Some("nori-fast-kazunoko-aac8")));

    let rendered = cells_to_text(&drain_insert_history(&mut rx));
    assert!(
        rendered.contains("session:"),
        "cloud welcome cell must show a 'session:' line, got:\n{rendered}"
    );
    assert!(
        rendered.contains("nori-fast-kazunoko-aac8"),
        "cloud welcome cell must name the cloud session id, got:\n{rendered}"
    );
}

/// Order-independence variant: capabilities arriving BEFORE SessionConfigured
/// (the pre-race test ordering) must produce the same welcome card.
#[test]
fn welcome_card_pipeline_caps_first_still_surfaces_session_id() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_capabilities(),
    ));

    chat.handle_codex_event(session_configured(Some("nori-fast-kazunoko-aac8")));

    let rendered = cells_to_text(&drain_insert_history(&mut rx));
    assert!(
        rendered.contains("nori-fast-kazunoko-aac8"),
        "cloud welcome cell must name the session id regardless of event order, got:\n{rendered}"
    );
}

/// Welcome-banner pipeline negative: a local agent (unnamed by the harness)
/// must keep the plain `directory:` card, no session line.
#[test]
fn welcome_card_pipeline_local_agent_hides_session_id() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.handle_codex_event(session_configured(None));
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        local_capabilities(false),
    ));

    let rendered = cells_to_text(&drain_insert_history(&mut rx));
    assert!(
        !rendered.contains("session:"),
        "a local welcome cell must not render cloud session identity, got:\n{rendered}"
    );
    assert!(
        rendered.contains("directory:"),
        "a local welcome cell must keep the directory line, got:\n{rendered}"
    );
}

/// Footer pipeline: on a cloud session the rendered widget footer must show
/// the id named by SessionConfigured (production order).
#[test]
fn footer_pipeline_cloud_agent_shows_session_id() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_codex_event(session_configured(Some("nori-fast-kazunoko-aac8")));
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_capabilities(),
    ));
    let _ = drain_insert_history(&mut rx);

    let rendered = render_bottom_popup(&chat, 120);
    assert!(
        rendered.contains("nori-fast-kazunoko-aac8"),
        "cloud footer must show the cloud session id, got:\n{rendered}"
    );
}

/// Footer pipeline negative: a local agent (unnamed SessionConfigured) must
/// get no cloud badge, whatever capabilities later report.
#[test]
fn footer_pipeline_local_agent_shows_no_cloud_id() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_codex_event(session_configured(None));
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        local_capabilities(false),
    ));
    let _ = drain_insert_history(&mut rx);

    let rendered = render_bottom_popup(&chat, 120);
    assert!(
        !rendered.contains('☁'),
        "a local session must not render a cloud badge in the footer, got:\n{rendered}"
    );
}
