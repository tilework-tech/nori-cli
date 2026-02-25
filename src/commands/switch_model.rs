use super::CommandHandler;
use crate::app::Model;

/// Command that opens the agent router modal for switching models
pub struct SwitchModelCommand;

impl CommandHandler for SwitchModelCommand {
    fn name(&self) -> &'static str {
        "model"
    }

    fn execute(&self, model: &mut Model) -> Result<(), String> {
        model.show_agent_router = true;
        Ok(())
    }
}
