# Slash Commands System

The slash commands system provides an extensible way to add special commands to the TUI without cluttering keyboard shortcuts.

## Architecture

The system uses a trait-based registry pattern:

```
src/commands/
├── mod.rs          # CommandHandler trait, CommandRegistry, parse_slash_command
├── exit.rs         # /exit command implementation
└── switch_model.rs # /switch-model command implementation
```

## CommandHandler Trait

All slash commands implement the `CommandHandler` trait:

```rust
pub trait CommandHandler {
    fn name(&self) -> &'static str;
    fn execute(&self, model: &mut Model) -> Result<(), String>;
}
```

- `name()`: Returns the command name (without "/" prefix)
- `execute()`: Modifies the model state as needed, returns Ok(()) or Err(msg)

## CommandRegistry

The `CommandRegistry` maintains a HashMap of command name → handler:

- `register(handler)`: Add a command to the registry
- `execute(name, model)`: Look up and execute a command by name
- `default()`: Creates a registry with all built-in commands pre-registered

## Adding New Commands

To add a new slash command:

1. Create `src/commands/your_command.rs`:
   ```rust
   use crate::app::Model;
   use super::CommandHandler;

   pub struct YourCommand;

   impl CommandHandler for YourCommand {
       fn name(&self) -> &'static str {
           "your-command"
       }

       fn execute(&self, model: &mut Model) -> Result<(), String> {
           // Modify model state here
           Ok(())
       }
   }
   ```

2. Add to `src/commands/mod.rs`:
   ```rust
   mod your_command;
   pub use your_command::YourCommand;
   ```

3. Register in `CommandRegistry::default()`:
   ```rust
   registry.register(Box::new(YourCommand));
   ```

4. Update UI instructions in `src/ui.rs` to include the new command

## Built-in Commands

### /exit
- Quits the application
- Implementation: Returns Ok(()), main loop sends Message::Quit
- File: `src/commands/exit.rs`

### /switch-model
- Opens the agent router overlay
- Implementation: Sets `model.show_agent_router = true`
- File: `src/commands/switch_model.rs`

## Command Parsing

The `parse_slash_command(input: &str) -> Option<String>` function:

- Returns `Some(command_name)` if input starts with "/"
- Trims whitespace from input and command name
- Returns `None` for non-command input (regular text)
- Returns `None` for bare "/" with no command name

## Integration with Main Loop

In `src/main.rs`, the SubmitInput handler:

1. Extracts user input from textarea
2. Calls `parse_slash_command()` to check if it's a command
3. If command: execute via registry, handle result, clear textarea
4. If not command: spawn backend subprocess as normal
5. Unknown commands show error with list of available commands

## Testing

Tests are in `tests/slash_commands_test.rs`:

- Command registry execution (success and unknown command)
- Individual command behavior (exit, switch-model)
- Command parsing (valid commands, regular text, edge cases)

All tests use real Model instances to verify actual behavior, not mocks.
