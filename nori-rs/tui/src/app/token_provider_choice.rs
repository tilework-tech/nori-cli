use super::*;
use crate::nori::token_provider_picker::TokenProviderChoice;
use crate::nori::token_provider_picker::TokenProviderKeyInputView;
use crate::nori::token_provider_picker::persist_token_provider_choice;
use crate::nori::token_provider_picker::token_provider_model_picker_params;
use crate::nori::token_provider_picker::token_provider_picker_params;

impl App {
    pub(super) fn open_token_provider_picker(&mut self, agent_name: &str, display_name: &str) {
        let agent = nori_harness::resolve_agent_slug(agent_name);
        let current = self
            .config
            .agent_token_providers
            .get(&agent)
            .map(|entry| entry.name.as_str());
        self.chat_widget
            .show_selection_view(token_provider_picker_params(
                &agent,
                display_name,
                current,
                &self.config.token_providers,
            ));
    }

    pub(super) fn open_token_provider_key_input(
        &mut self,
        agent_name: String,
        display_name: String,
        preset_id: String,
    ) {
        let saved = self
            .config
            .agent_token_providers
            .get(&agent_name)
            .filter(|entry| nori_harness::token_provider::supports_pi_connection(entry))
            .or_else(|| {
                self.config.token_providers.iter().find(|entry| {
                    entry.name == preset_id
                        && nori_harness::token_provider::supports_pi_connection(entry)
                })
            })
            .map(|entry| entry.env_key.clone());
        let view = TokenProviderKeyInputView::new(
            agent_name,
            display_name,
            preset_id,
            saved,
            Box::new(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty())),
            self.app_event_tx.clone(),
        );
        self.chat_widget.show_view(Box::new(view));
    }

    pub(super) fn open_token_provider_model_picker(
        &mut self,
        agent_name: &str,
        display_name: &str,
        preset_id: &str,
        api_key_env: &str,
    ) {
        let current_model = self
            .config
            .agent_token_providers
            .get(agent_name)
            .filter(|entry| nori_harness::token_provider::supports_pi_connection(entry))
            .map(|provider| provider.model.as_str());
        self.chat_widget
            .show_selection_view(token_provider_model_picker_params(
                agent_name,
                display_name,
                preset_id,
                api_key_env,
                current_model,
            ));
    }

    /// Save an agent's token provider choice, then start the agent with it.
    pub(super) fn choose_token_provider(
        &mut self,
        agent_name: String,
        display_name: String,
        choice: Option<TokenProviderChoice>,
    ) {
        let agent = nori_harness::resolve_agent_slug(&agent_name);
        match persist_token_provider_choice(&self.config.nori_home, &agent, choice.as_ref()) {
            Ok(Some(provider)) => {
                if let Some(existing) = self
                    .config
                    .token_providers
                    .iter_mut()
                    .find(|p| p.name == provider.name)
                {
                    *existing = provider.clone();
                } else {
                    self.config.token_providers.push(provider.clone());
                }
                self.config
                    .agent_token_providers
                    .insert(agent.clone(), provider);
            }
            Ok(None) => {
                self.config.agent_token_providers.remove(&agent);
            }
            Err(error) => {
                self.chat_widget
                    .add_error_message(format!("Could not save the model provider: {error:#}"));
                return;
            }
        }
        self.sync_runtime_config();
        self.begin_agent_candidate(agent, display_name);
    }
}
