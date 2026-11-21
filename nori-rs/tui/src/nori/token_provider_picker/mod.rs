//! Choosing a token provider for an agent that can use one: the provider, the
//! environment variable that holds its key, and the model.
//!
//! The flow runs after an agent is picked in `/agent`. The chosen variable
//! name is written to config; the key stays in the environment.

use std::path::Path;

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use nori_config::token_providers::Protocol;
use nori_config::token_providers::TokenProvider;
use nori_harness::token_provider::compatible_presets;
use nori_harness::token_provider::preset;
use nori_harness::token_provider::supports_pi_connection;
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
pub(crate) enum TokenProviderChoice {
    Saved(String),
    Preset {
        preset_id: String,
        api_key_env: String,
        model: String,
    },
}

/// The key variable the key step starts from for a preset: the agent's bound
/// connection when it uses that preset, else a saved connection named after
/// the preset, else `None` for the preset's suggested variable.
pub(crate) fn saved_key_env(
    preset_id: &str,
    bound: Option<&TokenProvider>,
    saved: &[TokenProvider],
) -> Option<String> {
    let uses_preset = |entry: &&TokenProvider| {
        serde_json::to_value(entry.preset)
            .ok()
            .is_some_and(|value| value == preset_id)
            && supports_pi_connection(entry)
    };
    bound
        .filter(uses_preset)
        .or_else(|| {
            saved
                .iter()
                .find(|entry| entry.name == preset_id && uses_preset(entry))
        })
        .map(|entry| entry.env_key.clone())
}

/// The provider step: the agent's own settings, or one of its token providers.
pub(crate) fn token_provider_picker_params(
    agent_name: &str,
    display_name: &str,
    current: Option<&str>,
    saved: &[TokenProvider],
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
    for provider in saved.iter().filter(|entry| supports_pi_connection(entry)) {
        let agent_name = agent_name.to_string();
        let display_name = display_name.to_string();
        let name = provider.name.clone();
        items.push(SelectionItem {
            name: format!("{} (saved)", provider.name),
            description: Some(format!(
                "{} · key from ${}",
                provider.model, provider.env_key
            )),
            is_current: current == Some(&provider.name),
            actions: vec![Box::new(move |tx| {
                tx.send(AppEvent::ChooseTokenProvider {
                    agent_name: agent_name.clone(),
                    display_name: display_name.clone(),
                    choice: Some(TokenProviderChoice::Saved(name.clone())),
                });
            })],
            dismiss_on_select: true,
            ..Default::default()
        });
    }
    let bound = current.and_then(|name| saved.iter().find(|entry| entry.name == name));
    items.extend(compatible_presets(agent_name).into_iter().map(|preset| {
        let key_env = saved_key_env(preset.id, bound, saved)
            .unwrap_or_else(|| preset.api_key_env.to_string());
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
            description: Some(format!("Token provider · key from ${key_env}")),
            is_current: false,
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
            let choice = TokenProviderChoice::Preset {
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

/// Save the named route independently of the agent's native default model.
pub(crate) fn persist_token_provider_choice(
    nori_home: &Path,
    agent: &str,
    choice: Option<&TokenProviderChoice>,
) -> anyhow::Result<Option<(TokenProvider, Protocol)>> {
    nori_config::NoriConfigEdits::select_token_provider(nori_home, agent, |saved| match choice {
        None => Ok(None),
        Some(TokenProviderChoice::Saved(name)) => {
            let provider = saved
                .iter()
                .find(|entry| &entry.name == name && supports_pi_connection(entry))
                .ok_or_else(|| {
                    anyhow::anyhow!("the saved connection is no longer available for Pi")
                })?;
            Ok(Some(provider.clone()))
        }
        Some(TokenProviderChoice::Preset {
            preset_id,
            api_key_env,
            model,
        }) => {
            let preset = preset(preset_id)
                .ok_or_else(|| anyhow::anyhow!("unknown Pi token provider preset"))?;
            let mut connection = preset.connection(preset_id, api_key_env, model);
            anyhow::ensure!(
                supports_pi_connection(&connection),
                "this connection cannot launch Pi"
            );
            let mut suffix = 2;
            while let Some(existing) = saved.iter().find(|entry| entry.name == connection.name) {
                if existing == &connection {
                    break;
                }
                connection.name = format!("{preset_id}-{suffix}");
                suffix += 1;
            }
            Ok(Some(connection))
        }
    })
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
    /// `saved` is the variable already configured for this provider, if any.
    pub(crate) fn new(
        agent_name: String,
        display_name: String,
        preset_id: String,
        saved: Option<String>,
        is_set: Box<dyn Fn(&str) -> bool>,
        app_event_tx: AppEventSender,
    ) -> Self {
        let preset = preset(&preset_id);
        Self {
            agent_name,
            display_name,
            provider_name: preset.map_or_else(|| preset_id.clone(), |preset| preset.name.into()),
            input: saved
                .or_else(|| preset.map(|preset| preset.api_key_env.to_string()))
                .unwrap_or_default(),
            preset_id,
            error: None,
            complete: false,
            is_set,
            app_event_tx,
        }
    }

    fn reject_non_name(&mut self) {
        // The text may be the key itself, so it is cleared, not echoed.
        self.input.clear();
        self.error = Some("That is not an environment variable name; enter the name.".to_string());
    }

    fn submit(&mut self) {
        let name = self.input.trim().to_string();
        if !is_variable_name(&name) {
            self.reject_non_name();
            return;
        }
        if !(self.is_set)(&name) {
            self.input.clear();
            self.error = Some("That variable is not set in nori's environment.".to_string());
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

fn is_variable_name(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
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
                self.error = None;
            }
            KeyCode::Char('u') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.clear();
                self.error = None;
            }
            KeyCode::Char(character)
                if !key_event
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.input.push(character);
                self.error = None;
            }
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
        let combined = format!("{}{}", self.input, pasted.trim());
        if is_variable_name(&combined) {
            self.input = combined;
            self.error = None;
        } else {
            self.reject_non_name();
        }
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
