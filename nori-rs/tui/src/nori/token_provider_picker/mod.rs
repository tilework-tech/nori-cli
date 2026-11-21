//! Choosing a token provider for an agent that can use one: the provider, the
//! environment variable that holds its key, and the model.
//!
//! The flow runs after an agent is picked in `/agent`. The chosen variable
//! name is written to config; the key stays in the environment.

use std::path::Path;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use nori_harness::token_provider::compatible_presets;
use nori_harness::token_provider::preset;
use ratatui::buffer::Buffer;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::Widget;

use crate::app_event::AppEvent;
use crate::app_event_sender::AppEventSender;
use crate::bottom_pane::BottomPaneView;
use crate::bottom_pane::CancellationEvent;
use crate::bottom_pane::SelectionAction;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::bottom_pane::popup_consts::standard_popup_hint_line;
use crate::render::Insets;
use crate::render::RectExt as _;
use crate::render::renderable::Renderable;
use crate::style::user_message_style;

/// A completed token provider choice for one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TokenProviderChoice {
    /// Preset id, also used as the provider id in config.
    pub preset_id: String,
    /// Name of the environment variable that holds the key.
    pub api_key_env: String,
    /// Provider model id.
    pub model: String,
}

/// Whether picking this agent should first ask for a token provider.
pub(crate) fn offers_token_providers(agent_name: &str) -> bool {
    !compatible_presets(agent_name).is_empty()
}

/// The provider step: the agent's own settings, or one of its token providers.
pub(crate) fn token_provider_picker_params(
    agent_name: &str,
    display_name: &str,
    current: Option<&str>,
) -> SelectionViewParams {
    let own_agent = agent_name.to_string();
    let own_display = display_name.to_string();
    let mut items = vec![SelectionItem {
        name: format!("{display_name} settings"),
        description: Some(format!("Use {display_name}'s own sign-in and models")),
        is_current: current.is_none(),
        actions: vec![Box::new(move |tx| {
            tx.send(AppEvent::ChooseTokenProvider {
                agent_name: own_agent.clone(),
                display_name: own_display.clone(),
                choice: None,
            });
        })],
        dismiss_on_select: true,
        ..Default::default()
    }];
    items.extend(compatible_presets(agent_name).into_iter().map(|preset| {
        let agent_name = agent_name.to_string();
        let display_name = display_name.to_string();
        let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
            tx.send(AppEvent::OpenTokenProviderKeyInput {
                agent_name: agent_name.clone(),
                display_name: display_name.clone(),
                preset_id: preset.id.to_string(),
            });
        })];
        SelectionItem {
            name: preset.name.to_string(),
            description: Some(format!("Token provider · key from ${}", preset.api_key_env)),
            is_current: current == Some(preset.id),
            actions,
            dismiss_on_select: true,
            ..Default::default()
        }
    }));
    SelectionViewParams {
        title: Some(format!("Model provider for {display_name}")),
        subtitle: Some("Where the agent gets its models".to_string()),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
    .picker()
}

/// The model step, listing the provider's catalog with its default first.
pub(crate) fn token_provider_model_picker_params(
    agent_name: &str,
    display_name: &str,
    preset_id: &str,
    api_key_env: &str,
    current_model: Option<&str>,
) -> SelectionViewParams {
    let Some(preset) = preset(preset_id) else {
        return SelectionViewParams::default();
    };
    let mut models: Vec<_> = preset.models.iter().collect();
    models.sort_by_key(|model| model.id != preset.default_model);
    let items = models
        .into_iter()
        .map(|model| {
            let agent_name = agent_name.to_string();
            let display_name = display_name.to_string();
            let choice = TokenProviderChoice {
                preset_id: preset.id.to_string(),
                api_key_env: api_key_env.to_string(),
                model: model.id.to_string(),
            };
            let actions: Vec<SelectionAction> = vec![Box::new(move |tx| {
                tx.send(AppEvent::ChooseTokenProvider {
                    agent_name: agent_name.clone(),
                    display_name: display_name.clone(),
                    choice: Some(choice.clone()),
                });
            })];
            let description = if model.id == preset.default_model {
                format!("{} · recommended", model.id)
            } else {
                model.id.to_string()
            };
            SelectionItem {
                name: model.name.to_string(),
                description: Some(description),
                is_current: current_model == Some(model.id),
                actions,
                dismiss_on_select: true,
                ..Default::default()
            }
        })
        .collect();
    SelectionViewParams {
        title: Some(format!("{} model for {display_name}", preset.name)),
        subtitle: Some(format!("Key from ${api_key_env}")),
        footer_hint: Some(standard_popup_hint_line()),
        items,
        ..Default::default()
    }
    .picker()
}

/// Persist a choice and bind the agent to it, or clear the agent's binding.
///
/// Clearing keeps the provider definition and drops the agent's saved model
/// when that model belonged to the provider.
pub(crate) fn persist_token_provider_choice(
    nori_home: &Path,
    agent: &str,
    choice: Option<&TokenProviderChoice>,
) -> anyhow::Result<()> {
    let edits = nori_config::NoriConfigEdits::new(nori_home);
    let edits = match choice {
        Some(choice) => edits
            .set_path(
                &["token_providers", &choice.preset_id, "preset"],
                choice.preset_id.as_str(),
            )
            .set_path(
                &["token_providers", &choice.preset_id, "api_key_env"],
                choice.api_key_env.as_str(),
            )
            .set_path(&["agent_token_providers", agent], choice.preset_id.as_str())
            .set_default_model(agent, &format!("{}/{}", choice.preset_id, choice.model)),
        None => {
            let config_path = nori_home.join(nori_config::CONFIG_FILE);
            let saved = nori_config::NoriConfig::load_from_path(&config_path).ok();
            let bound_model = saved.as_ref().and_then(|config| {
                let provider = config.agent_token_providers.get(agent)?;
                config
                    .default_models
                    .get(agent)
                    .filter(|model| model.starts_with(&format!("{provider}/")))
            });
            let edits = edits.clear_path(&["agent_token_providers", agent]);
            if bound_model.is_some() {
                edits.clear_path(&["default_models", agent])
            } else {
                edits
            }
        }
    };
    edits.apply_blocking()
}

/// Asks for the environment variable that holds a provider's key.
pub(crate) struct TokenProviderKeyInputView {
    agent_name: String,
    display_name: String,
    preset_id: String,
    provider_name: String,
    input: String,
    error: Option<String>,
    complete: bool,
    is_set: Box<dyn Fn(&str) -> bool>,
    app_event_tx: AppEventSender,
}

impl TokenProviderKeyInputView {
    pub(crate) fn new(
        agent_name: String,
        display_name: String,
        preset_id: String,
        is_set: Box<dyn Fn(&str) -> bool>,
        app_event_tx: AppEventSender,
    ) -> Self {
        let preset = preset(&preset_id);
        Self {
            agent_name,
            display_name,
            provider_name: preset.map_or_else(|| preset_id.clone(), |preset| preset.name.into()),
            input: preset
                .map(|preset| preset.api_key_env.to_string())
                .unwrap_or_default(),
            preset_id,
            error: None,
            complete: false,
            is_set,
            app_event_tx,
        }
    }

    fn submit(&mut self) {
        let name = self.input.trim().to_string();
        let mut characters = name.chars();
        let valid = characters
            .next()
            .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
            && characters.all(|character| character == '_' || character.is_ascii_alphanumeric());
        if !valid {
            // The text may be the key itself, so it is cleared, not echoed.
            self.input.clear();
            self.error =
                Some("That is not an environment variable name; enter the name.".to_string());
            return;
        }
        if !(self.is_set)(&name) {
            self.error = Some(format!("{name} is not set in nori's environment."));
            return;
        }
        self.app_event_tx
            .send(AppEvent::OpenTokenProviderModelPicker {
                agent_name: self.agent_name.clone(),
                display_name: self.display_name.clone(),
                preset_id: self.preset_id.clone(),
                api_key_env: name,
            });
        self.complete = true;
    }
}

impl BottomPaneView for TokenProviderKeyInputView {
    fn handle_key_event(&mut self, key_event: KeyEvent) {
        if key_event.kind != KeyEventKind::Press && key_event.kind != KeyEventKind::Repeat {
            return;
        }
        match key_event.code {
            KeyCode::Esc => {
                self.on_ctrl_c();
            }
            KeyCode::Enter => self.submit(),
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(character) => self.input.push(character),
            _ => {}
        }
    }

    fn is_complete(&self) -> bool {
        self.complete
    }

    fn on_ctrl_c(&mut self) -> CancellationEvent {
        self.complete = true;
        CancellationEvent::Handled
    }

    fn handle_paste(&mut self, pasted: String) -> bool {
        self.input.push_str(&pasted);
        true
    }
}

impl Renderable for TokenProviderKeyInputView {
    fn desired_height(&self, _width: u16) -> u16 {
        // Five rows inside the border.
        7
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
        Block::bordered()
            .title(format!(" {} key ", self.provider_name))
            .border_style(user_message_style())
            .render(area, buf);
        let rows = Layout::vertical([Constraint::Length(1); 5]).split(area.inset(Insets::vh(1, 2)));
        Line::from(format!(
            "Environment variable that holds your {} API key",
            self.provider_name
        ))
        .dim()
        .render(rows[0], buf);
        Line::from(format!("› {}│", self.input)).render(rows[2], buf);
        if let Some(error) = &self.error {
            Line::from(error.clone().red()).render(rows[3], buf);
        }
        Line::from("Enter to continue · Esc to cancel · nori stores only the name")
            .dim()
            .render(rows[4], buf);
    }
}

#[cfg(test)]
mod tests;
