use super::*;
use insta::assert_snapshot;
use nori_harness::SessionConfigOption;
use nori_harness::SessionConfigOptionCategory;
use nori_harness::SessionConfigSelectOption;

fn model_config_option() -> SessionConfigOption {
    SessionConfigOption::select(
        "model",
        "Model",
        "claude-opus-4-6",
        vec![
            SessionConfigSelectOption::new("claude-opus-4-6", "Opus 4.6")
                .description("Most capable model"),
            SessionConfigSelectOption::new("claude-sonnet-4-6", "Sonnet 4.6")
                .description("Fast and capable"),
        ],
    )
    .category(SessionConfigOptionCategory::Model)
}

/// When an ACP agent provides a Model-category config option, /model should
/// open the session config value picker for that option (showing selectable
/// model choices) instead of the "not supported" message.
#[tokio::test]
async fn model_popup_routes_to_config_option_when_model_category_present() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Set up a mock ACP handle that responds to GetSessionConfig with a
    // Model-category config option.
    let (command_tx, mut command_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::chatwidget::agent::AcpAgentCommand>();
    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            if let crate::chatwidget::agent::AcpAgentCommand::GetSessionConfig { response_tx } =
                command
            {
                let _ = response_tx.send(vec![model_config_option()]);
            }
        }
    });
    chat.acp_handle = Some(crate::chatwidget::agent::AcpAgentHandle::from_command_tx(
        command_tx,
    ));

    chat.open_model_popup();

    // The async task sends an event — wait for it.
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for event")
        .expect("channel closed");

    // Should route to the session config value picker, not the ACP model picker.
    assert_matches!(
        event,
        AppEvent::OpenAcpSessionConfigValuePicker { option } => {
            assert_eq!(option.category, Some(SessionConfigOptionCategory::Model));
        }
    );
}

/// When an ACP handle is present but config_options have NO Model-category
/// option, /model should show the "not supported" fallback.
#[tokio::test]
async fn model_popup_falls_back_when_no_model_config_option() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Set up a mock ACP handle that responds to GetSessionConfig with a
    // Mode-only config option (no Model category).
    let (command_tx, mut command_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::chatwidget::agent::AcpAgentCommand>();
    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            if let crate::chatwidget::agent::AcpAgentCommand::GetSessionConfig { response_tx } =
                command
            {
                let mode_only = SessionConfigOption::select(
                    "mode",
                    "Mode",
                    "plan",
                    vec![
                        SessionConfigSelectOption::new("plan", "Plan"),
                        SessionConfigSelectOption::new("build", "Build"),
                    ],
                )
                .category(SessionConfigOptionCategory::Mode);
                let _ = response_tx.send(vec![mode_only]);
            }
        }
    });
    chat.acp_handle = Some(crate::chatwidget::agent::AcpAgentHandle::from_command_tx(
        command_tx,
    ));

    chat.open_model_popup();

    // Wait for the event. With no Model-category config option it should fall
    // back to the "not supported" model picker.
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for event")
        .expect("channel closed");

    assert_matches!(event, AppEvent::OpenAcpModelPickerUnsupported);
}

/// Snapshot: the model value picker rendered via a Model-category config option
/// should display selectable model names.
#[test]
fn model_popup_via_config_option_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.open_acp_session_config_value_picker(model_config_option());

    let popup = render_bottom_popup(&chat, 80);
    assert_snapshot!("model_popup_via_config_option", popup);
}

/// When an agent switch is pending (the user picked a new agent but hasn't
/// submitted a prompt yet, so no new session exists), /model must NOT query the
/// still-live OLD agent's handle — that would show stale models. Instead it
/// shows an explanatory message naming the pending agent.
#[tokio::test]
async fn model_popup_shows_pending_message_instead_of_stale_models() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // The OLD agent's handle would happily respond with a Model config option.
    let (command_tx, mut command_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::chatwidget::agent::AcpAgentCommand>();
    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            if let crate::chatwidget::agent::AcpAgentCommand::GetSessionConfig { response_tx } =
                command
            {
                let _ = response_tx.send(vec![model_config_option()]);
            }
        }
    });
    chat.acp_handle = Some(crate::chatwidget::agent::AcpAgentHandle::from_command_tx(
        command_tx,
    ));

    // But an agent switch is pending: no new session has started.
    chat.set_pending_agent("newagent".to_string(), "New Agent".to_string());

    chat.open_model_popup();

    // Give any (incorrectly) spawned task time to query the OLD handle and
    // route to its picker. The OLD agent's model picker must never open while a
    // switch is pending — that is exactly the stale-models bug.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    while let Ok(event) = rx.try_recv() {
        assert!(
            !matches!(event, AppEvent::OpenAcpSessionConfigValuePicker { .. }),
            "must not route to the OLD agent's model picker while a switch is pending"
        );
    }

    // The popup explains that a session must start first, naming the agent.
    let popup = render_bottom_popup(&chat, 80);
    assert!(
        popup.contains("New Agent"),
        "popup should name the pending agent:\n{popup}"
    );
    assert!(
        popup.contains("session"),
        "popup should mention starting a session:\n{popup}"
    );
}

/// Snapshot: the model picker shown while an agent switch is pending.
#[test]
fn model_popup_pending_agent_snapshot() {
    let (mut chat, _rx, _op_rx) = make_chatwidget_manual();

    chat.set_pending_agent("newagent".to_string(), "New Agent".to_string());
    chat.open_model_popup();

    let popup = render_bottom_popup(&chat, 80);
    assert_snapshot!("model_popup_pending_agent", popup);
}

/// Capabilities matching the nori cloud contract: `loadSession: false` with
/// `sessionCapabilities.{list,resume,close}` all advertised.
fn cloud_session_capabilities() -> nori_protocol::SessionCapabilitiesView {
    nori_protocol::SessionCapabilitiesView {
        agent: nori_protocol::AgentCapabilitiesView {
            http_mcp: false,
            load_session: false,
            session_list: true,
            session_resume: true,
            session_close: true,
        },
        nori_client: nori_protocol::NoriClientCapabilitiesView::default(),
        builtin_commands: std::collections::HashMap::new(),
    }
}

/// The /resume picker must source rows from the agent's `session/list` when
/// the agent advertises list+resume WITHOUT `load_session` — the nori cloud
/// contract. Requiring `load_session` would silently fall back to the local
/// transcript picker, hiding every cloud session.
#[tokio::test]
async fn resume_picker_sources_from_agent_list_with_resume_capability() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_session_capabilities(),
    ));

    let (command_tx, mut command_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::chatwidget::agent::AcpAgentCommand>();
    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            if let crate::chatwidget::agent::AcpAgentCommand::ListSessions { response_tx, .. } =
                command
            {
                let _ = response_tx.send(Ok(vec![nori_harness::AcpSessionSummary {
                    session_id: "cloud-sess-1".to_string(),
                    cwd: std::path::PathBuf::from("/"),
                    title: Some("slack · claude".to_string()),
                    updated_at: None,
                    meta: None,
                }]));
            }
        }
    });
    chat.acp_handle = Some(crate::chatwidget::agent::AcpAgentHandle::from_command_tx(
        command_tx,
    ));

    chat.open_resume_session_picker();

    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for the picker event")
        .expect("channel closed");
    assert_matches!(event, AppEvent::ShowAcpResumeSessionPicker { sessions } => {
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "cloud-sess-1");
    });
}

/// Stub agent task that forwards each `CloseSession` response sender to the
/// test, so the test controls exactly when (and how) the close resolves.
fn stub_close_handle(
    chat: &mut ChatWidget,
) -> tokio::sync::mpsc::UnboundedReceiver<tokio::sync::oneshot::Sender<anyhow::Result<()>>> {
    let (command_tx, mut command_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::chatwidget::agent::AcpAgentCommand>();
    let (pending_tx, pending_rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            if let crate::chatwidget::agent::AcpAgentCommand::CloseSession { response_tx } = command
            {
                let _ = pending_tx.send(response_tx);
            }
        }
    });
    chat.acp_handle = Some(crate::chatwidget::agent::AcpAgentHandle::from_command_tx(
        command_tx,
    ));
    pending_rx
}

/// /close releases the live session over `session/close` and, once the agent
/// confirms, reports `SessionClosed` so the app can return to the session
/// picker. It must NOT start a fresh chat: on a cloud agent an automatic
/// NewSession would silently claim a brand-new VM the user never asked for.
#[tokio::test]
async fn close_command_returns_to_the_picker_not_a_new_session() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_session_capabilities(),
    ));
    let mut pending_rx = stub_close_handle(&mut chat);

    chat.dispatch_command(SlashCommand::Close);

    let response_tx = tokio::time::timeout(std::time::Duration::from_secs(2), pending_rx.recv())
        .await
        .expect("session/close must be requested on the agent handle")
        .expect("stub agent task closed unexpectedly");

    // While the close is still in flight nothing may proceed, and
    // session-switching commands are blocked so a deferred follow-up can't
    // clobber a conversation the user switches to.
    while let Ok(event) = rx.try_recv() {
        assert!(
            !matches!(event, AppEvent::NewSession | AppEvent::SessionClosed),
            "the close outcome must wait for the agent's response"
        );
    }
    chat.dispatch_command(SlashCommand::Resume);
    let cells = drain_insert_history(&mut rx);
    let rendered = lines_to_single_string(cells.last().expect("expected a blocked-command cell"));
    assert!(
        rendered.contains("disabled while the session closes"),
        "session-switching must be blocked mid-close, got: {rendered}"
    );

    response_tx
        .send(Ok(()))
        .expect("widget dropped the close response receiver");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let event = tokio::time::timeout(remaining, rx.recv())
            .await
            .expect("timed out waiting for AppEvent::SessionClosed after /close")
            .expect("channel closed");
        match event {
            AppEvent::SessionClosed => break,
            AppEvent::NewSession => {
                panic!("/close must return to the session picker, not auto-start a new session")
            }
            _ => {}
        }
    }

    // A trailing NewSession sneaking in AFTER SessionClosed would still claim
    // a fresh VM. Give the close task a chance to run to completion, then
    // require silence.
    tokio::task::yield_now().await;
    while let Ok(event) = rx.try_recv() {
        assert!(
            !matches!(event, AppEvent::NewSession),
            "/close must not follow SessionClosed with a NewSession"
        );
    }
}

/// A failed /close reports the error and does NOT start a new chat —
/// otherwise /close is just /new with extra steps and the session leaks.
#[tokio::test]
async fn close_command_failure_reports_error_and_keeps_session() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        cloud_session_capabilities(),
    ));
    let mut pending_rx = stub_close_handle(&mut chat);

    chat.dispatch_command(SlashCommand::Close);

    let response_tx = tokio::time::timeout(std::time::Duration::from_secs(2), pending_rx.recv())
        .await
        .expect("session/close must be requested on the agent handle")
        .expect("stub agent task closed unexpectedly");
    response_tx
        .send(Err(anyhow::anyhow!("session not found")))
        .expect("widget dropped the close response receiver");

    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for the close-failure event")
        .expect("channel closed");
    let message = match event {
        AppEvent::SessionCloseFailed { message } => message,
        AppEvent::NewSession => panic!("a failed close must not start a new chat"),
        other => panic!("unexpected event after failed close: {other:?}"),
    };
    // Let the close task finish before requiring silence — an immediate
    // try_recv would only prove nothing was queued *yet*.
    tokio::task::yield_now().await;
    assert_matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    );

    // Route the failure back to the widget (as the app would): the error is
    // surfaced and session-switching commands are unblocked again.
    chat.on_session_close_failed(message);
    let cells = drain_insert_history(&mut rx);
    let rendered = lines_to_single_string(cells.last().expect("expected a close-failure cell"));
    assert!(
        rendered.contains("Failed to close the session"),
        "expected a close-failure message, got: {rendered}"
    );
    chat.dispatch_command(SlashCommand::Resume);
    while let Ok(event) = rx.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            let rendered = lines_to_single_string(&cell.display_lines(80));
            assert!(
                !rendered.contains("disabled while the session closes"),
                "a failed close must unblock session-switching commands"
            );
        }
    }
}

/// `session/list` alone (without `session/resume` or `load_session`) must NOT
/// route /resume to the agent picker — the rows would be unresumable.
#[tokio::test]
async fn resume_picker_ignores_agent_list_without_a_resume_path() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();
    let mut capabilities = cloud_session_capabilities();
    capabilities.agent.session_resume = false;
    chat.handle_client_event(nori_protocol::ClientEvent::SessionCapabilitiesChanged(
        capabilities,
    ));

    let (command_tx, mut command_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::chatwidget::agent::AcpAgentCommand>();
    let (listed_tx, mut listed_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            if let crate::chatwidget::agent::AcpAgentCommand::ListSessions { .. } = command {
                let _ = listed_tx.send(());
            }
        }
    });
    chat.acp_handle = Some(crate::chatwidget::agent::AcpAgentHandle::from_command_tx(
        command_tx,
    ));

    chat.open_resume_session_picker();

    // The local-transcript fallback path must run instead: it responds (via a
    // history cell or the local picker event) without ever calling the agent.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let event = tokio::time::timeout(remaining, rx.recv())
            .await
            .expect("timed out waiting for the local fallback to respond")
            .expect("channel closed");
        match event {
            AppEvent::ShowAcpResumeSessionPicker { .. } => {
                panic!("list without a resume path must not open the agent picker")
            }
            AppEvent::InsertHistoryCell(_) | AppEvent::ShowResumeSessionPicker { .. } => break,
            _ => {}
        }
    }
    assert_matches!(
        listed_rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty),
        "the agent's session/list must not be called without a resume path"
    );
}

/// /close on an agent that does not advertise `session/close` must explain
/// itself instead of sending an unsupported request.
#[test]
fn close_command_is_gated_on_the_close_capability() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    chat.dispatch_command(SlashCommand::Close);

    let cells = drain_insert_history(&mut rx);
    assert!(
        !cells.is_empty(),
        "expected a user-visible message explaining /close is unavailable"
    );
    let rendered = lines_to_single_string(cells.last().unwrap());
    assert!(
        rendered.contains("/close is unavailable") && rendered.contains("session/close"),
        "expected the scoped unsupported-capability explanation, got: {rendered}"
    );
}
