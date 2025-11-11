use crate::app::Model;
use crate::conversation::ConversationEvent;
use super::CommandHandler;

pub struct DebugCommand;

impl CommandHandler for DebugCommand {
    fn name(&self) -> &'static str {
        "debug"
    }

    fn execute(&self, model: &mut Model) -> Result<(), String> {
        model.show_debug_events = !model.show_debug_events;

        // Add status message to show toggle result
        let status = if model.show_debug_events {
            "enabled"
        } else {
            "disabled"
        };

        model.response_events.push(ConversationEvent::StatusMessage {
            text: format!("Debug logs are now {}", status),
        });

        Ok(())
    }
}
