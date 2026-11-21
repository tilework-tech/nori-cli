use super::*;
use crate::nori::token_provider_picker::TokenProviderChoice;
use crate::nori::token_provider_picker::persist_token_provider_choice;

impl App {
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
        let previous = self
            .config
            .agent_token_providers
            .iter()
            .find(|(bound, _)| nori_harness::resolve_agent_slug(bound) == agent)
            .map(|(_, provider)| provider.clone());
        self.config
            .agent_token_providers
            .retain(|bound, _| nori_harness::resolve_agent_slug(bound) != agent);
        match choice {
            Some(choice) => {
                self.config.token_providers.insert(
                    choice.preset_id.clone(),
                    nori_config::TokenProviderToml {
                        preset: choice.preset_id.clone(),
                        api_key_env: choice.api_key_env,
                        base_url: None,
                    },
                );
                self.config
                    .agent_token_providers
                    .insert(agent.clone(), choice.preset_id.clone());
                self.apply_saved_model(
                    agent.clone(),
                    &format!("{}/{}", choice.preset_id, choice.model),
                );
            }
            None => {
                let provider_model = previous.is_some_and(|provider| {
                    self.config
                        .default_models
                        .get(&agent)
                        .is_some_and(|model| model.starts_with(&format!("{provider}/")))
                });
                if provider_model {
                    self.config.default_models.remove(&agent);
                }
                self.sync_runtime_config();
            }
        }
        self.begin_agent_candidate(agent, display_name);
    }
}
