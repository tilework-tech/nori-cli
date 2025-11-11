use nori_cli::app::Model;
use nori_cli::commands::{CommandRegistry, filter_commands, parse_slash_command};

// Tests for CommandRegistry - listing available commands

#[test]
fn test_command_registry_lists_all_names() {
    // Arrange: Create default registry
    let registry = CommandRegistry::default();

    // Act: Get all command names
    let mut names = registry.get_all_command_names();
    names.sort(); // Ensure stable order

    // Assert: Returns all registered commands
    assert_eq!(names, vec!["debug", "exit", "switch-model"]);
}

// Tests for filter_commands - prefix-based filtering

#[test]
fn test_filter_commands_empty_prefix() {
    // Arrange: List of all commands
    let all_commands = vec!["exit".to_string(), "switch-model".to_string()];

    // Act: Filter with empty prefix
    let result = filter_commands("", &all_commands);

    // Assert: Returns all commands
    assert_eq!(result, vec!["exit", "switch-model"]);
}

#[test]
fn test_filter_commands_partial_match() {
    // Arrange: List of all commands
    let all_commands = vec!["exit".to_string(), "switch-model".to_string()];

    // Act: Filter with "swi" prefix
    let result = filter_commands("swi", &all_commands);

    // Assert: Returns only "switch-model"
    assert_eq!(result, vec!["switch-model"]);
}

#[test]
fn test_filter_commands_no_match() {
    // Arrange: List of all commands
    let all_commands = vec!["exit".to_string(), "switch-model".to_string()];

    // Act: Filter with non-matching prefix
    let result = filter_commands("xyz", &all_commands);

    // Assert: Returns empty vector
    assert!(result.is_empty());
}

#[test]
fn test_filter_commands_case_insensitive() {
    // Arrange: List of all commands
    let all_commands = vec!["exit".to_string(), "switch-model".to_string()];

    // Act: Filter with uppercase prefix
    let result = filter_commands("SWI", &all_commands);

    // Assert: Returns "switch-model" (case-insensitive)
    assert_eq!(result, vec!["switch-model"]);
}

// Tests for CommandRegistry - executing commands

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
    assert!(
        result.unwrap_err().contains("Unknown command"),
        "error should mention unknown command"
    );
}

#[test]
fn test_switch_model_command_opens_agent_router() {
    // Arrange: Create a model with agent router closed
    let mut model = Model::default();
    assert_eq!(
        model.show_agent_router, false,
        "agent router should start closed"
    );

    let registry = CommandRegistry::default();

    // Act: Execute switch-model command
    let result = registry.execute("switch-model", &mut model);

    // Assert: Agent router is now open
    assert!(
        result.is_ok(),
        "switch-model command should execute successfully"
    );
    assert_eq!(
        model.show_agent_router, true,
        "agent router should be open after switch-model"
    );
}

// Tests for command parsing

#[test]
fn test_parse_slash_command_with_exit() {
    // Act: Parse "/exit"
    let result = parse_slash_command("/exit");

    // Assert: Returns command name "exit"
    assert_eq!(
        result,
        Some("exit".to_string()),
        "/exit should parse to 'exit'"
    );
}

#[test]
fn test_parse_slash_command_with_switch_model() {
    // Act: Parse "/switch-model"
    let result = parse_slash_command("/switch-model");

    // Assert: Returns command name "switch-model"
    assert_eq!(
        result,
        Some("switch-model".to_string()),
        "/switch-model should parse to 'switch-model'"
    );
}

#[test]
fn test_parse_slash_command_with_regular_text() {
    // Act: Parse regular text
    let result = parse_slash_command("hello world");

    // Assert: Returns None (not a command)
    assert_eq!(result, None, "regular text should not parse as command");
}

#[test]
fn test_parse_slash_command_with_empty_string() {
    // Act: Parse empty string
    let result = parse_slash_command("");

    // Assert: Returns None
    assert_eq!(result, None, "empty string should not parse as command");
}

#[test]
fn test_parse_slash_command_with_trailing_whitespace() {
    // Act: Parse "/exit   " with trailing whitespace
    let result = parse_slash_command("/exit   ");

    // Assert: Returns "exit" (whitespace trimmed)
    assert_eq!(
        result,
        Some("exit".to_string()),
        "/exit with trailing whitespace should parse to 'exit'"
    );
}
