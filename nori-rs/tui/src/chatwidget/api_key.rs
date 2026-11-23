//! `/api-key`: choose whether the current agent uses its own login (the
//! default) or the API key saved through `nori auth set-key`.

use super::*;

/// What the `/api-key` picker shows for one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApiKeyPreference {
    /// Canonical agent slug, the `[auth.<agent>]` key in config.toml.
    pub agent: String,
    pub display_name: String,
    pub prefer_api_key: bool,
    pub api_billing_confirmed: bool,
    /// Providers the agent talks to that have a key saved through Nori.
    pub saved_providers: Vec<String>,
    /// Set when the agent is signed in with a subscription the saved key
    /// would replace.
    pub billing_warning: Option<String>,
}

impl ChatWidget {
    /// Handle `/api-key`: detect the current agent's sign-in off the UI thread,
    /// then open the picker.
    pub(super) fn open_api_key_preference(&mut self) {
        let slug = nori_harness::resolve_agent_slug(&self.config.active_agent);
        // A custom agent registered under a built-in alias is not built in.
        let Some(agent) = nori_providers::AgentKind::from_slug(&slug)
            .filter(|agent| agent.slug() == slug && !nori_providers::key_routes(*agent).is_empty())
        else {
            let name = nori_harness::get_agent_display_name(&self.config.active_agent);
            self.add_info_message(
                format!("{name} signs in through its own login only; it takes no saved API key."),
                None,
            );
            return;
        };
        let preference = self.config.auth_preference(agent.slug());
        let nori_home = self.config.nori_home.clone();
        let tx = self.app_event_tx.clone();
        tokio::task::spawn_blocking(move || {
            let detected = dirs::home_dir()
                .ok_or_else(|| anyhow::anyhow!("could not find the home directory"))
                .and_then(|home| {
                    nori_providers::detect_local_agent(
                        agent,
                        &home,
                        &nori_home,
                        preference.prefer_api_key,
                    )
                });
            match detected {
                Ok(local) => tx.send(AppEvent::OpenApiKeyPreference(ApiKeyPreference {
                    agent: agent.slug().to_string(),
                    display_name: agent.display_name().to_string(),
                    prefer_api_key: preference.prefer_api_key,
                    api_billing_confirmed: preference.api_billing_confirmed,
                    saved_providers: local
                        .saved_providers
                        .iter()
                        .map(|provider| provider.id().to_string())
                        .collect(),
                    billing_warning: local
                        .status
                        .api_key_billing_warning()
                        .map(|notice| notice.message),
                })),
                Err(error) => tx.send(AppEvent::InsertHistoryCell(Box::new(
                    history_cell::new_error_event(format!(
                        "Could not check how {} signs in: {error:#}",
                        agent.display_name()
                    )),
                ))),
            }
        });
    }

    pub(crate) fn open_api_key_picker(&mut self, preference: ApiKeyPreference) {
        let name = preference.display_name.clone();
        let use_key_description = if preference.saved_providers.is_empty() {
            "No key saved yet; add one with `nori auth set-key <provider>`".to_string()
        } else {
            format!(
                "Pass the saved {} key at launch; usage is billed to that API account",
                preference.saved_providers.join(" and ")
            )
        };
        let login = persist_action(&preference, false, preference.api_billing_confirmed);
        let use_key: SelectionAction = if preference.billing_warning.is_some()
            && !preference.api_billing_confirmed
        {
            let preference = preference.clone();
            Box::new(move |tx| tx.send(AppEvent::OpenApiKeyBillingConfirmation(preference.clone())))
        } else {
            persist_action(&preference, true, preference.api_billing_confirmed)
        };
        let items = vec![
            SelectionItem {
                name: format!("Use {name}'s own login"),
                description: Some("Default; a subscription sign-in keeps its plan".to_string()),
                is_current: !preference.prefer_api_key,
                actions: vec![login],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Use the saved API key".to_string(),
                description: Some(use_key_description),
                is_current: preference.prefer_api_key,
                actions: vec![use_key],
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        self.bottom_pane.show_selection_view(SelectionViewParams {
            title: Some(format!("Sign-in for {name}")),
            subtitle: Some("Applies the next time the agent starts.".to_string()),
            footer_hint: Some(standard_popup_hint_line()),
            initial_selected_idx: Some(usize::from(preference.prefer_api_key)),
            items,
            ..Default::default()
        });
    }

    pub(crate) fn open_api_key_billing_confirmation(&mut self, preference: ApiKeyPreference) {
        let warning = preference.billing_warning.clone().unwrap_or_default();
        let header = ColumnRenderable::with(vec![
            Box::new(
                Line::from("WARNING: this changes who you are billed")
                    .bold()
                    .red(),
            ) as Box<dyn Renderable>,
            Box::new(
                Paragraph::new(vec![Line::from(warning.fg(Color::Red))]).wrap(Wrap { trim: false }),
            ),
        ]);
        let items = vec![
            SelectionItem {
                name: "Yes, bill the API key".to_string(),
                description: Some("Remember this answer and use the saved key".to_string()),
                actions: vec![persist_action(&preference, true, true)],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Cancel".to_string(),
                description: Some(format!("Keep {} on its own login", preference.display_name)),
                dismiss_on_select: true,
                ..Default::default()
            },
        ];
        self.bottom_pane.show_selection_view(SelectionViewParams {
            footer_hint: Some(standard_popup_hint_line()),
            // Cancel is selected first so a stray Enter keeps the plan.
            initial_selected_idx: Some(1),
            items,
            header: Box::new(header),
            ..Default::default()
        });
    }
}

fn persist_action(
    preference: &ApiKeyPreference,
    prefer_api_key: bool,
    api_billing_confirmed: bool,
) -> SelectionAction {
    let agent = preference.agent.clone();
    Box::new(move |tx| {
        tx.send(AppEvent::PersistApiKeyPreference {
            agent: agent.clone(),
            prefer_api_key,
            api_billing_confirmed,
        });
    })
}
