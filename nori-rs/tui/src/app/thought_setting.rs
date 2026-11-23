//! The `/settings` toggle for showing agent thought text in history.

use super::*;
use crate::nori::config_picker::SettingsItem;
use nori_config::NoriConfigEdits as ConfigEditsBuilder;

impl App {
    /// Saves `tui.thoughts_in_history`. Thought blocks that already reached
    /// history stay; the setting applies to thoughts that arrive afterwards.
    pub(super) async fn persist_thoughts_in_history_setting(&mut self, enabled: bool) {
        if let Err(err) = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["tui", "thoughts_in_history"], enabled)
            .apply()
            .await
        {
            tracing::error!(error = %err, "failed to persist thoughts_in_history setting");
            self.chat_widget
                .add_error_message(format!("Failed to save thoughts_in_history setting: {err}"));
            return;
        }
        self.config.thoughts_in_history = enabled;
        self.sync_runtime_config();
        let status = if enabled {
            "Thoughts now appear in history."
        } else {
            "Thoughts now appear only in the working indicator."
        };
        self.chat_widget.add_info_message(status.to_string(), None);
        self.chat_widget
            .reopen_settings_focused(SettingsItem::ThoughtsInHistory);
    }
}
