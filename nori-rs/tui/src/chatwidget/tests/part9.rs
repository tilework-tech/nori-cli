use super::*;
use insta::assert_snapshot;
use nori_acp::SessionConfigOption;
use nori_acp::SessionConfigOptionCategory;
use nori_acp::SessionConfigSelectOption;

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
/// option (and unstable model state is empty), /model should show the
/// "not supported" fallback.
#[tokio::test]
async fn model_popup_falls_back_when_no_model_config_option() {
    let (mut chat, mut rx, _op_rx) = make_chatwidget_manual();

    // Set up a mock ACP handle that responds to GetSessionConfig with a
    // Mode-only config option (no Model category).
    let (command_tx, mut command_rx) =
        tokio::sync::mpsc::unbounded_channel::<crate::chatwidget::agent::AcpAgentCommand>();
    tokio::spawn(async move {
        while let Some(command) = command_rx.recv().await {
            match command {
                crate::chatwidget::agent::AcpAgentCommand::GetSessionConfig { response_tx } => {
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
                #[cfg(feature = "unstable")]
                crate::chatwidget::agent::AcpAgentCommand::GetModelState { response_tx } => {
                    let _ = response_tx.send(nori_acp::AcpModelState::new());
                }
                _ => {}
            }
        }
    });
    chat.acp_handle = Some(crate::chatwidget::agent::AcpAgentHandle::from_command_tx(
        command_tx,
    ));

    chat.open_model_popup();

    // Wait for the event. With no Model-category config option and empty
    // unstable model state, it should fall back to the empty model picker.
    let event = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for event")
        .expect("channel closed");

    assert_matches!(
        event,
        AppEvent::OpenAcpModelPicker { models, .. } => {
            assert!(models.is_empty(), "expected empty models list for fallback");
        }
    );
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
