//! Applies the persisted default model (`[default_models]` in config.toml)
//! when an ACP session starts.
//!
//! Agents advertise model selection through the stable session config options
//! mechanism (a select option with `category: "model"`); older agents may only
//! expose the unstable `session/set_model` API. The stable mechanism is
//! preferred: when the agent advertises a Model-category option, it owns model
//! selection and the unstable API is not used. Application is best-effort —
//! an unknown model id or a wire error must never block session startup.

use super::*;

pub(super) async fn apply_default_model(
    connection: &SacpConnection,
    session_id: &acp::SessionId,
    default_model: &str,
) {
    let config_options = connection.config_options();
    let model_option = config_options
        .iter()
        .find(|option| option.category == Some(acp::SessionConfigOptionCategory::Model));

    if let Some(option) = model_option {
        apply_via_config_option(connection, session_id, option, default_model).await;
        return;
    }

    #[cfg(feature = "unstable")]
    apply_via_set_model(connection, session_id, default_model).await;
}

async fn apply_via_config_option(
    connection: &SacpConnection,
    session_id: &acp::SessionId,
    option: &acp::SessionConfigOption,
    default_model: &str,
) {
    let acp::SessionConfigKind::Select(select) = &option.kind else {
        debug!("Model config option is not a select; skipping default model '{default_model}'");
        return;
    };
    if select.current_value.to_string() == default_model {
        debug!("Default model '{default_model}' is already active");
        return;
    }
    let available = match &select.options {
        acp::SessionConfigSelectOptions::Ungrouped(values) => values
            .iter()
            .any(|value| value.value.to_string() == default_model),
        acp::SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter())
            .any(|value| value.value.to_string() == default_model),
        _ => false,
    };
    if !available {
        debug!("Default model '{default_model}' not in config option values, skipping");
        return;
    }

    match connection
        .set_config_option(session_id, option.id.clone(), default_model.to_string())
        .await
    {
        Ok(()) => debug!("Applied default model from config: {default_model}"),
        Err(e) => warn!("Failed to apply default model '{default_model}': {e}"),
    }
}

#[cfg(feature = "unstable")]
async fn apply_via_set_model(
    connection: &SacpConnection,
    session_id: &acp::SessionId,
    default_model: &str,
) {
    let model_state = connection.model_state();
    let model_available = model_state
        .available_models
        .iter()
        .any(|m| m.model_id.to_string() == default_model);
    if !model_available {
        debug!("Default model '{default_model}' not in available models, skipping");
        return;
    }
    let model_id = acp::ModelId::from(default_model.to_string());
    match connection.set_model(session_id, &model_id).await {
        Ok(()) => debug!("Applied default model from config: {default_model}"),
        Err(e) => warn!("Failed to apply default model '{default_model}': {e}"),
    }
}
