use nori_cli::app::{InstallChoice, Message, Model};

#[test]
fn test_navigate_install_choice_with_install_cmd() {
    let mut model = Model::default();

    // Setup: Show install prompt with install_cmd (3 options scenario)
    model.update(Message::ShowInstallPrompt {
        backend: "GPT Codex".to_string(),
        url: "https://example.com".to_string(),
        install_cmd: Some(vec!["npm".to_string(), "install".to_string()]),
    });

    // Should start with RunInstallation by default when install_cmd exists
    assert_eq!(model.install_prompt_choice, InstallChoice::RunInstallation);

    // Navigate down: RunInstallation -> OpenInstallPage
    model.update(Message::NavigateInstallChoiceNext);
    assert_eq!(model.install_prompt_choice, InstallChoice::OpenInstallPage);

    // Navigate down: OpenInstallPage -> Cancel
    model.update(Message::NavigateInstallChoiceNext);
    assert_eq!(model.install_prompt_choice, InstallChoice::Cancel);

    // Navigate down: Cancel -> RunInstallation (wrap around)
    model.update(Message::NavigateInstallChoiceNext);
    assert_eq!(model.install_prompt_choice, InstallChoice::RunInstallation);

    // Navigate up: RunInstallation -> Cancel (reverse wrap)
    model.update(Message::NavigateInstallChoicePrevious);
    assert_eq!(model.install_prompt_choice, InstallChoice::Cancel);

    // Navigate up: Cancel -> OpenInstallPage
    model.update(Message::NavigateInstallChoicePrevious);
    assert_eq!(model.install_prompt_choice, InstallChoice::OpenInstallPage);

    // Navigate up: OpenInstallPage -> RunInstallation
    model.update(Message::NavigateInstallChoicePrevious);
    assert_eq!(model.install_prompt_choice, InstallChoice::RunInstallation);
}

#[test]
fn test_navigate_install_choice_without_install_cmd() {
    let mut model = Model::default();

    // Setup: Show install prompt WITHOUT install_cmd (2 options scenario)
    model.update(Message::ShowInstallPrompt {
        backend: "GPT Codex".to_string(),
        url: "https://example.com".to_string(),
        install_cmd: None,
    });

    // Should start with OpenInstallPage when no install_cmd
    assert_eq!(model.install_prompt_choice, InstallChoice::OpenInstallPage);

    // Navigate down: OpenInstallPage -> Cancel
    model.update(Message::NavigateInstallChoiceNext);
    assert_eq!(model.install_prompt_choice, InstallChoice::Cancel);

    // Navigate down: Cancel -> OpenInstallPage (wrap, skip RunInstallation)
    model.update(Message::NavigateInstallChoiceNext);
    assert_eq!(model.install_prompt_choice, InstallChoice::OpenInstallPage);

    // Navigate up: OpenInstallPage -> Cancel (reverse)
    model.update(Message::NavigateInstallChoicePrevious);
    assert_eq!(model.install_prompt_choice, InstallChoice::Cancel);

    // Navigate up: Cancel -> OpenInstallPage (reverse wrap)
    model.update(Message::NavigateInstallChoicePrevious);
    assert_eq!(model.install_prompt_choice, InstallChoice::OpenInstallPage);
}
