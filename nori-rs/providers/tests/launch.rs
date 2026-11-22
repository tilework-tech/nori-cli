#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use codex_keyring_store::tests::MockKeyringStore;
use nori_acp_host::registry::AgentKind;
use nori_acp_host::registry::get_agent_config;
use nori_acp_host::registry::get_agent_launch_config;
use nori_config::token_providers::Preset;
use nori_config::token_providers::Protocol;
use nori_config::token_providers::TokenProvider;
use nori_providers::SavedKeyStore;
use nori_providers::StoreMode;
use nori_providers::apply_saved_key;
use pretty_assertions::assert_eq;

fn store_with(agent: AgentKind, key: &str) -> (tempfile::TempDir, SavedKeyStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(
        dir.path(),
        StoreMode::Auto,
        Arc::new(MockKeyringStore::default()),
    );
    store.save(agent, key).unwrap();
    (dir, store)
}

#[test]
fn saved_key_reaches_the_agent_environment() {
    let (_dir, store) = store_with(AgentKind::ClaudeCode, "sk-saved");
    let mut config = get_agent_config("claude-code").unwrap();

    let applied = apply_saved_key(&mut config, &store, &|_| None).unwrap();

    assert_eq!(applied, Some("ANTHROPIC_API_KEY"));
    assert_eq!(
        config.env.get("ANTHROPIC_API_KEY"),
        Some(&"sk-saved".to_string())
    );
}

#[test]
fn ambient_environment_wins_over_the_saved_key() {
    let (_dir, store) = store_with(AgentKind::ClaudeCode, "sk-saved");
    let mut config = get_agent_config("claude-code").unwrap();
    let before = config.env.clone();

    let applied = apply_saved_key(&mut config, &store, &|key| {
        (key == "ANTHROPIC_API_KEY").then(|| "sk-env".into())
    })
    .unwrap();

    assert_eq!((applied, config.env), (None, before));
}

#[test]
fn key_already_in_the_agent_config_wins() {
    let (_dir, store) = store_with(AgentKind::Gemini, "saved");
    let mut config = get_agent_config("gemini").unwrap();
    config
        .env
        .insert("GEMINI_API_KEY".into(), "configured".into());

    let applied = apply_saved_key(&mut config, &store, &|_| None).unwrap();

    assert_eq!(
        (applied, config.env.get("GEMINI_API_KEY").cloned()),
        (None, Some("configured".to_string()))
    );
}

#[test]
fn named_token_provider_launches_are_left_alone() {
    let (_dir, store) = store_with(AgentKind::Codex, "sk-saved");
    let binding = TokenProvider {
        name: "router".into(),
        preset: Preset::Custom,
        protocol: Protocol::OpenaiResponses,
        base_url: "http://127.0.0.1:4567/v1".into(),
        env_key: "ROUTER_KEY".into(),
        model: "m".into(),
    };
    let mut config = get_agent_launch_config("codex", Some(&binding)).unwrap();
    let before = config.env.clone();

    let applied = apply_saved_key(&mut config, &store, &|_| None).unwrap();

    assert_eq!((applied, config.env), (None, before));
}

#[test]
fn nothing_saved_leaves_the_environment_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let store = SavedKeyStore::new(
        dir.path(),
        StoreMode::Auto,
        Arc::new(MockKeyringStore::default()),
    );
    let mut config = get_agent_config("codex").unwrap();
    let before: HashMap<_, _> = config.env.clone();

    let applied = apply_saved_key(&mut config, &store, &|_| None).unwrap();

    assert_eq!((applied, config.env), (None, before));
}
