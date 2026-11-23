//! Applies the persisted default model (`[default_models]` in config.toml)
//! when an ACP session starts.
//!
//! Agents advertise model selection through the stable session config options
//! mechanism (a select option with `category: "model"`). When the agent
//! advertises a Model-category option, it owns model selection. Application is
//! best-effort — an unknown model id or a wire error must never block session
//! startup.

use super::*;

pub(super) async fn apply_default_model(
    connection: &AcpConnection,
    session_id: &acp::SessionId,
    default_model: &str,
    builtin: bool,
) {
    let config_options = connection.config_options();
    let model_option = config_options
        .iter()
        .find(|option| option.category == Some(acp::SessionConfigOptionCategory::Model));

    let Some(option) = model_option else { return };
    let acp::SessionConfigKind::Select(select) = &option.kind else {
        warn!("Model config option is not a select; skipping default model '{default_model}'");
        return;
    };
    if select.current_value.to_string() == default_model {
        debug!("Default model '{default_model}' is already active");
        return;
    }

    if builtin {
        let offered = |values: &[acp::SessionConfigSelectOption]| {
            values
                .iter()
                .any(|choice| choice.value.0.as_ref() == default_model)
        };
        let available = match &select.options {
            acp::SessionConfigSelectOptions::Ungrouped(values) => offered(values),
            acp::SessionConfigSelectOptions::Grouped(groups) => {
                groups.iter().any(|group| offered(&group.options))
            }
            _ => false,
        };
        if !available {
            warn!(
                "The agent did not offer the default model '{default_model}'; keeping its current model"
            );
            return;
        }
    }

    match connection
        .set_config_option(session_id, option.id.clone(), default_model.to_string())
        .await
    {
        Ok(()) => debug!("Applied default model from config: {default_model}"),
        Err(e) => warn!("Failed to apply default model '{default_model}': {e}"),
    }
}
