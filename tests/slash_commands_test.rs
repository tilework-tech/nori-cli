use nori_cli::app::Model;
use nori_cli::commands::CommandRegistry;

#[test]
fn test_registry_executes_registered_command() {
    // Arrange: Create a model and registry
    let mut model = Model::default();
    let registry = CommandRegistry::default();

    // Act: Execute the "exit" command
    let result = registry.execute("exit", &mut model);

    // Assert: Command executed successfully
    assert!(result.is_ok(), "exit command should execute successfully");
}

#[test]
fn test_registry_returns_error_for_unknown_command() {
    // Arrange: Create a model and registry
    let mut model = Model::default();
    let registry = CommandRegistry::default();

    // Act: Execute an unknown command
    let result = registry.execute("unknown", &mut model);

    // Assert: Returns error for unknown command
    assert!(result.is_err(), "unknown command should return error");
    assert!(result.unwrap_err().contains("Unknown command"), "error should mention unknown command");
}

#[test]
fn test_switch_model_command_opens_agent_router() {
    // Arrange: Create a model with agent router closed
    let mut model = Model::default();
    assert_eq!(model.show_agent_router, false, "agent router should start closed");

    let registry = CommandRegistry::default();

    // Act: Execute switch-model command
    let result = registry.execute("switch-model", &mut model);

    // Assert: Agent router is now open
    assert!(result.is_ok(), "switch-model command should execute successfully");
    assert_eq!(model.show_agent_router, true, "agent router should be open after switch-model");
}
