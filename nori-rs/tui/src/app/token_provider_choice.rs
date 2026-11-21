use super::*;
use crate::nori::token_provider_picker::TokenProviderChoice;
use crate::nori::token_provider_picker::TokenProviderKeyInputView;
use crate::nori::token_provider_picker::persist_token_provider_choice;
use crate::nori::token_provider_picker::token_provider_model_picker_params;
use crate::nori::token_provider_picker::token_provider_picker_params;

impl App {
    fn bound_provider_id(&self, agent: &str) -> Option<String> {
        self.config
            .agent_token_providers
            .iter()
            .find(|(bound, _)| nori_harness::resolve_agent_slug(bound) == agent)
            .map(|(_, provider)| provider.clone())
    }

    pub(super) fn open_token_provider_picker(&mut self, agent_name: &str, display_name: &str) {
        let agent = nori_harness::resolve_agent_slug(agent_name);
        let current = self
            .bound_provider_id(&agent)
            .and_then(|provider| self.config.token_providers.get(&provider))
            .map(|provider| provider.preset.clone());
        self.chat_widget
            .show_selection_view(token_provider_picker_params(
                &agent,
                display_name,
                current.as_deref(),
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
            .token_providers
            .get(&preset_id)
            .map(|provider| provider.api_key_env.clone());
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
            .default_models
            .get(agent_name)
            .and_then(|model| model.strip_prefix(&format!("{preset_id}/")))
            .map(str::to_owned);
        self.chat_widget
            .show_selection_view(token_provider_model_picker_params(
                agent_name,
                display_name,
                preset_id,
                api_key_env,
                current_model.as_deref(),
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
        if let Err(error) =
            persist_token_provider_choice(&self.config.nori_home, &agent, choice.as_ref())
        {
            self.chat_widget
                .add_error_message(format!("Could not save the model provider: {error:#}"));
            return;
        }
        let previous = self.bound_provider_id(&agent);
        self.config
            .agent_token_providers
            .retain(|bound, _| nori_harness::resolve_agent_slug(bound) != agent);
        match choice {
            Some(choice) => {
                let provider = self
                    .config
                    .token_providers
                    .entry(choice.preset_id.clone())
                    .or_insert_with(|| nori_config::TokenProviderToml {
                        preset: choice.preset_id.clone(),
                        api_key_env: choice.api_key_env.clone(),
                        base_url: None,
                    });
                provider.preset = choice.preset_id.clone();
                provider.api_key_env = choice.api_key_env;
                self.config
                    .agent_token_providers
                    .insert(agent.clone(), choice.preset_id.clone());
                self.apply_saved_model(
                    agent.clone(),
                    &format!("{}/{}", choice.preset_id, choice.model),
                );
            }
            None => {
                if let Some(provider) = previous {
                    let prefix = format!("{provider}/");
                    self.config.default_models.retain(|key, model| {
                        nori_harness::resolve_agent_slug(key) != agent
                            || !model.starts_with(&prefix)
                    });
                }
                self.sync_runtime_config();
            }
        }
        self.begin_agent_candidate(agent, display_name);
    }
}
