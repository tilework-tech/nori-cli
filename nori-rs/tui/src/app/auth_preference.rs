//! Saving the `/api-key` choice to `[auth.<agent>]` in config.toml.

use super::*;
use nori_config::NoriConfigEdits as ConfigEditsBuilder;

impl App {
    pub(super) async fn persist_api_key_preference(
        &mut self,
        agent: String,
        prefer_api_key: bool,
        api_billing_confirmed: bool,
    ) {
        let mut edits = ConfigEditsBuilder::new(&self.config.nori_home)
            .set_path(&["auth", &agent, "prefer_api_key"], prefer_api_key);
        if api_billing_confirmed {
            edits = edits.set_path(&["auth", &agent, "api_billing_confirmed"], true);
        }
        if let Err(err) = edits.apply().await {
            tracing::error!(error = %err, "failed to persist the API key preference");
            self.chat_widget
                .add_error_message(format!("Failed to save the API key preference: {err}"));
            return;
        }
        let name = nori_harness::get_agent_display_name(&agent);
        let preference = self.config.auth.entry(agent).or_default();
        preference.prefer_api_key = prefer_api_key;
        // A remembered answer is never forgotten, even when another shell
        // confirmed it after this picker opened.
        preference.api_billing_confirmed |= api_billing_confirmed;
        self.sync_runtime_config();
        let choice = if prefer_api_key {
            "the saved API key"
        } else {
            "its own login"
        };
        self.chat_widget.add_info_message(
            format!("{name} uses {choice} from its next start."),
            Some("Start a new session with /new to apply it now.".to_string()),
        );
    }
}
