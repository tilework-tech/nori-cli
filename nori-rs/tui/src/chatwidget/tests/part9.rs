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
