use super::*;
use crate::app_event::AppEvent;
use crate::bottom_pane::BottomPaneView;
use crate::render::renderable::Renderable;
use crate::test_backend::VT100Backend;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::Terminal;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::unbounded_channel;

fn sender() -> (AppEventSender, UnboundedReceiver<AppEvent>) {
    let (tx, rx) = unbounded_channel();
    (AppEventSender::new(tx), rx)
}

fn choose(params: &crate::bottom_pane::SelectionViewParams, name: &str, tx: &AppEventSender) {
    let item = params
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no row named {name}"));
    for action in &item.actions {
        action(tx);
    }
}

fn key_input(
    present: &'static [&'static str],
) -> (TokenProviderKeyInputView, UnboundedReceiver<AppEvent>) {
    let (tx, rx) = sender();
    let view = TokenProviderKeyInputView::new(
        "pi".to_string(),
        "Pi".to_string(),
        "subconscious".to_string(),
        None,
        Box::new(move |name| present.contains(&name)),
        tx,
    );
    (view, rx)
}

fn press(view: &mut TokenProviderKeyInputView, code: KeyCode) {
    view.handle_key_event(KeyEvent::new(code, KeyModifiers::NONE));
}

fn retype(view: &mut TokenProviderKeyInputView, text: &str) {
    view.handle_key_event(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    for character in text.chars() {
        press(view, KeyCode::Char(character));
    }
}

fn render(view: &TokenProviderKeyInputView) -> String {
    let mut terminal = Terminal::new(VT100Backend::new(72, view.desired_height(72))).unwrap();
    terminal
        .draw(|frame| view.render(frame.area(), frame.buffer_mut()))
        .unwrap();
    terminal.backend().to_string()
}

#[test]
fn choosing_pi_settings_starts_pi_without_a_token_provider() {
    let (tx, mut rx) = sender();
    let params = token_provider_picker_params("pi", "Pi", Some("subconscious"));
    let current: Vec<&str> = params
        .items
        .iter()
        .filter(|item| item.is_current)
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(current, ["Subconscious"]);
    choose(&params, "Pi settings", &tx);
    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::ChooseTokenProvider { agent_name, choice: None, .. }) if agent_name == "pi"
    ));
}

#[test]
fn choosing_subconscious_asks_for_its_key_variable() {
    let (tx, mut rx) = sender();
    let params = token_provider_picker_params("pi", "Pi", None);
    let current: Vec<&str> = params
        .items
        .iter()
        .filter(|item| item.is_current)
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(current, ["Pi settings"]);
    choose(&params, "Subconscious", &tx);
    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::OpenTokenProviderKeyInput { agent_name, preset_id, .. })
            if agent_name == "pi" && preset_id == "subconscious"
    ));
}

#[test]
fn the_key_input_suggests_the_provider_variable_and_accepts_it_when_set() {
    let (mut view, mut rx) = key_input(&["SUBCONSCIOUS_API_KEY"]);
    insta::assert_snapshot!("token_provider_key_input", render(&view));
    press(&mut view, KeyCode::Enter);
    assert!(view.is_complete());
    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::OpenTokenProviderModelPicker { preset_id, api_key_env, .. })
            if preset_id == "subconscious" && api_key_env == "SUBCONSCIOUS_API_KEY"
    ));
}

#[test]
fn the_key_input_refuses_a_variable_that_is_not_set() {
    let (mut view, mut rx) = key_input(&[]);
    retype(&mut view, "MY_SUBC_KEY");
    press(&mut view, KeyCode::Enter);
    assert!(!view.is_complete());
    assert!(rx.try_recv().is_err());
    insta::assert_snapshot!("token_provider_key_input_unset", render(&view));
}

#[test]
fn an_unset_variable_is_reported_without_repeating_what_was_typed() {
    let (mut view, _rx) = key_input(&[]);
    retype(&mut view, "subc_synthetic_0004");
    press(&mut view, KeyCode::Enter);
    let rendered = render(&view);
    assert!(!rendered.contains("subc_synthetic_0004"), "{rendered}");
    assert!(rendered.contains("not set"), "{rendered}");
}

#[test]
fn pasted_text_that_is_not_a_variable_name_is_never_shown() {
    let (mut view, mut rx) = key_input(&[]);
    retype(&mut view, "");
    view.handle_paste("sk-synthetic-0005".to_string());
    let rendered = render(&view);
    assert!(!rendered.contains("sk-synthetic-0005"), "{rendered}");
    press(&mut view, KeyCode::Enter);
    assert!(rx.try_recv().is_err());
}

#[test]
fn the_key_input_starts_from_the_saved_variable() {
    let (tx, mut rx) = sender();
    let mut view = TokenProviderKeyInputView::new(
        "pi".to_string(),
        "Pi".to_string(),
        "subconscious".to_string(),
        Some("TEAM_SUBC_KEY".to_string()),
        Box::new(|name| name == "TEAM_SUBC_KEY"),
        tx,
    );
    press(&mut view, KeyCode::Enter);
    assert!(matches!(
        rx.try_recv(),
        Ok(AppEvent::OpenTokenProviderModelPicker { api_key_env, .. }) if api_key_env == "TEAM_SUBC_KEY"
    ));
}

#[test]
fn choosing_pi_settings_clears_bindings_saved_under_an_alias() {
    let home = tempfile::tempdir().unwrap();
    let config_path = home.path().join("config.toml");
    std::fs::write(
        &config_path,
        "[token_providers.subconscious]\npreset = 'subconscious'\napi_key_env = 'SUBCONSCIOUS_API_KEY'\n\n[agent_token_providers]\npi-acp = 'subconscious'\n\n[default_models]\npi-acp = 'subconscious/subconscious/glm-5.2'\n",
    )
    .unwrap();
    persist_token_provider_choice(home.path(), "pi", None).unwrap();
    let cleared = nori_config::NoriConfig::load_from_path(&config_path).unwrap();
    assert!(cleared.agent_token_providers.is_empty());
    assert!(cleared.default_models.is_empty());
}

#[test]
fn the_key_input_refuses_text_that_is_not_a_variable_name() {
    for text in ["sk-synthetic-0001", "subc synthetic", "1SUBC_KEY"] {
        let (mut view, mut rx) = key_input(&["sk-synthetic-0001", "subc synthetic", "1SUBC_KEY"]);
        retype(&mut view, text);
        press(&mut view, KeyCode::Enter);
        assert!(!view.is_complete(), "{text}");
        assert!(rx.try_recv().is_err(), "{text}");
        let rendered = render(&view);
        assert!(
            rendered.contains("not an environment variable name"),
            "{rendered}"
        );
    }
}

#[test]
fn escape_leaves_the_key_input_without_choosing() {
    let (mut view, mut rx) = key_input(&["SUBCONSCIOUS_API_KEY"]);
    press(&mut view, KeyCode::Esc);
    assert!(view.is_complete());
    assert!(rx.try_recv().is_err());
}

#[test]
fn the_model_picker_offers_the_catalog_and_records_the_full_choice() {
    let (tx, mut rx) = sender();
    let params = token_provider_model_picker_params(
        "pi",
        "Pi",
        "subconscious",
        "SUBCONSCIOUS_API_KEY",
        None,
    );
    let names: Vec<&str> = params.items.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names[0], "GLM-5.3 Marathon");
    assert!(names.contains(&"GLM-5.2"));
    choose(&params, "GLM-5.2", &tx);
    let Ok(AppEvent::ChooseTokenProvider {
        agent_name,
        choice: Some(choice),
        ..
    }) = rx.try_recv()
    else {
        panic!("expected a token provider choice");
    };
    assert_eq!(agent_name, "pi");
    assert_eq!(
        choice,
        TokenProviderChoice {
            preset_id: "subconscious".to_string(),
            api_key_env: "SUBCONSCIOUS_API_KEY".to_string(),
            model: "subconscious/glm-5.2".to_string(),
        }
    );
}

#[test]
fn a_persisted_choice_loads_back_as_the_agent_binding_and_can_be_cleared() {
    let home = tempfile::tempdir().unwrap();
    let config_path = home.path().join("config.toml");
    let choice = TokenProviderChoice {
        preset_id: "subconscious".to_string(),
        api_key_env: "SUBCONSCIOUS_API_KEY".to_string(),
        model: "subconscious/glm-5.2".to_string(),
    };
    std::fs::write(
        &config_path,
        "# kept\nagent = \"codex\"\n\n[default_models]\ncodex = \"gpt-synthetic\"\n",
    )
    .unwrap();
    persist_token_provider_choice(home.path(), "pi", Some(&choice)).unwrap();
    let written = std::fs::read_to_string(&config_path).unwrap();
    assert!(written.contains("# kept"), "{written}");

    let loaded = nori_config::NoriConfig::load_from_path(&config_path).unwrap();
    assert_eq!(
        loaded.agent_token_providers.get("pi").map(String::as_str),
        Some("subconscious")
    );
    assert_eq!(
        loaded.token_providers.get("subconscious"),
        Some(&nori_config::TokenProviderToml {
            preset: "subconscious".to_string(),
            api_key_env: "SUBCONSCIOUS_API_KEY".to_string(),
            base_url: None,
        })
    );
    assert_eq!(
        loaded.default_models.get("pi").map(String::as_str),
        Some("subconscious/subconscious/glm-5.2")
    );

    persist_token_provider_choice(home.path(), "pi", None).unwrap();
    let cleared = nori_config::NoriConfig::load_from_path(&config_path).unwrap();
    assert!(cleared.agent_token_providers.is_empty());
    assert!(cleared.token_providers.contains_key("subconscious"));
    assert_eq!(cleared.default_models.get("pi"), None);
    assert_eq!(
        cleared.default_models.get("codex").map(String::as_str),
        Some("gpt-synthetic")
    );
}
