use nori_cli::app::{BACKEND_OPTIONS, Model};

#[test]
fn test_backend_options_ordering() {
    // Verify the backend names are in the correct order
    let expected_names = vec![
        "Claude Code ACP",
        "Codex ACP",
        "Gemini ACP",
        "Mock ACP Agent",
        "Claude Code",
    ];

    let actual_names: Vec<&str> = BACKEND_OPTIONS.iter().map(|option| option.name).collect();

    assert_eq!(
        actual_names, expected_names,
        "Backend names should be in the expected order"
    );
}

#[test]
fn test_get_backend_returns_correct_types() {
    let model = Model::default();

    // Test that get_backend returns the correct backend type for each index
    let backend_0 = model.get_backend();
    assert_eq!(
        backend_0.name(),
        "Claude Code ACP",
        "Index 0 should return Claude Code ACP backend"
    );

    // Test other indices by changing selected_agent_index
    let mut model = model;
    model.selected_agent_index = 1;
    let backend_1 = model.get_backend();
    assert_eq!(
        backend_1.name(),
        "Codex ACP",
        "Index 1 should return Codex ACP backend"
    );

    model.selected_agent_index = 2;
    let backend_2 = model.get_backend();
    assert_eq!(
        backend_2.name(),
        "Gemini ACP",
        "Index 2 should return Gemini ACP backend"
    );

    model.selected_agent_index = 3;
    let backend_3 = model.get_backend();
    assert_eq!(
        backend_3.name(),
        "Mock ACP Agent",
        "Index 3 should return Mock ACP Agent backend"
    );

    model.selected_agent_index = 4;
    let backend_4 = model.get_backend();
    assert_eq!(
        backend_4.name(),
        "Claude Code",
        "Index 4 should return Claude Code backend"
    );
}
