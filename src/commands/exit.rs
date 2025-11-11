use crate::app::Model;
use super::CommandHandler;

/// Command that exits the application
pub struct ExitCommand;

impl CommandHandler for ExitCommand {
    fn name(&self) -> &'static str {
        "exit"
    }

    fn execute(&self, _model: &mut Model) -> Result<(), String> {
        // The exit command doesn't modify the model directly
        // It will be handled by sending Message::Quit in the main loop
        Ok(())
    }
}
