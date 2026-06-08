use ratatui::text::Line;
use strum::IntoEnumIterator;

use super::ChatWidget;
use crate::slash_command::SlashCommand;

const CLOUD_MODE_DISABLED_REASON: &str = "Not available in cloud mode.";

impl ChatWidget {
    pub(super) fn apply_cloud_mode_restrictions(&mut self) {
        if !self.is_cloud_session {
            return;
        }
        for cmd in SlashCommand::iter() {
            if !cmd.available_in_cloud_mode() {
                self.builtin_command_availability.insert(
                    cmd.command().to_string(),
                    nori_protocol::CommandAvailability {
                        enabled: false,
                        reason: Some(CLOUD_MODE_DISABLED_REASON.to_string()),
                    },
                );
                self.bottom_pane.set_builtin_command_disabled(
                    cmd,
                    Some(Line::from(CLOUD_MODE_DISABLED_REASON)),
                );
            }
        }
    }
}
