use crate::app::Model;
use std::collections::HashMap;

mod exit;
mod switch_model;

pub use exit::ExitCommand;
pub use switch_model::SwitchModelCommand;

/// Parses a potential slash command from user input
/// Returns Some(command_name) if input starts with "/", None otherwise
pub fn parse_slash_command(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if let Some(command) = trimmed.strip_prefix('/') {
        // Extract command name after the "/"
        let command = command.trim(); // Trim any whitespace after "/"
        if command.is_empty() {
            None
        } else {
            Some(command.to_string())
        }
    } else {
        None
    }
}

/// Standard interface for all slash commands
pub trait CommandHandler {
    /// Returns the name of the command (without the "/" prefix)
    fn name(&self) -> &'static str;

    /// Executes the command, modifying the model state as needed
    fn execute(&self, model: &mut Model) -> Result<(), String>;
}

/// Registry for slash commands - routes command names to handlers
pub struct CommandRegistry {
    commands: HashMap<String, Box<dyn CommandHandler>>,
}

impl CommandRegistry {
    /// Creates a new empty registry
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Registers a command handler
    pub fn register(&mut self, handler: Box<dyn CommandHandler>) {
        let name = handler.name().to_string();
        self.commands.insert(name, handler);
    }

    /// Executes a command by name, returning an error if the command is not found
    pub fn execute(&self, name: &str, model: &mut Model) -> Result<(), String> {
        match self.commands.get(name) {
            Some(handler) => handler.execute(model),
            None => Err(format!("Unknown command: /{}", name)),
        }
    }
}

impl Default for CommandRegistry {
    /// Creates a registry with all built-in commands registered
    fn default() -> Self {
        let mut registry = Self::new();
        registry.register(Box::new(ExitCommand));
        registry.register(Box::new(SwitchModelCommand));
        registry
    }
}
