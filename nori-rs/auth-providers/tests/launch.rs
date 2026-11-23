#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashMap;

use common::file_store;
use nori_acp_host::registry::get_agent_config;
use nori_acp_host::registry::get_agent_launch_config;
use nori_auth_providers::Credential;
use nori_auth_providers::CredentialStore;
use nori_auth_providers::Provider;
use nori_auth_providers::apply_saved_keys;
use nori_auth_providers::inject_saved_keys;
use nori_config::token_providers::Preset;
use nori_config::token_providers::Protocol;
use nori_config::token_providers::TokenProvider;
use pretty_assertions::assert_eq;

fn store_with(saved: &[(Provider, &str)]) -> (tempfile::TempDir, CredentialStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = file_store(dir.path());
    for (provider, key) in saved {
        store
            .set(*provider, Credential::api_key(key).unwrap())
            .unwrap();
    }
    (dir, store)
}

/// Variables set on `slug`'s launch config by the saved keys, with values,
/// for an agent the user set to prefer API keys.
fn launch_preferring_keys(
    slug: &str,
    store: &CredentialStore,
    ambient: &[(&str, &str)],
) -> (Vec<&'static str>, HashMap<String, String>) {
    launch_with_preference(slug, store, true, ambient)
}

fn launch_with_preference(
    slug: &str,
    store: &CredentialStore,
    prefer_api_key: bool,
    ambient: &[(&str, &str)],
) -> (Vec<&'static str>, HashMap<String, String>) {
    let mut config = get_agent_config(slug).unwrap();
    let before = config.env.clone();
    let env = |key: &str| {
        ambient
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.to_string())
    };
    let applied = apply_saved_keys(&mut config, store, prefer_api_key, &env).unwrap();
    let added = config
        .env
        .into_iter()
        .filter(|(name, value)| before.get(name) != Some(value))
        .collect();
    (applied, added)
}

fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

#[test]
fn saved_keys_stay_out_of_launches_unless_the_agent_prefers_api_keys() {
    let (_dir, store) = store_with(&[
        (Provider::Anthropic, "sk-ant"),
        (Provider::OpenAi, "sk-openai"),
    ]);
    let launched: Vec<_> = ["claude-code", "codex", "opencode", "pi"]
        .map(|slug| launch_with_preference(slug, &store, false, &[]))
        .to_vec();
    assert_eq!(launched, vec![(Vec::new(), HashMap::new()); 4]);
}

#[test]
fn saved_anthropic_key_reaches_claude_code() {
    let (_dir, store) = store_with(&[(Provider::Anthropic, "sk-saved")]);
    assert_eq!(
        launch_preferring_keys("claude-code", &store, &[]),
        (
            vec!["ANTHROPIC_API_KEY"],
            vars(&[("ANTHROPIC_API_KEY", "sk-saved")])
        )
    );
}

#[test]
fn one_openai_key_serves_every_openai_backed_agent() {
    let (_dir, store) = store_with(&[(Provider::OpenAi, "sk-openai")]);
    let launched: Vec<_> = ["codex", "opencode", "pi"]
        .map(|slug| launch_preferring_keys(slug, &store, &[]))
        .to_vec();
    let expected = vec![
        (
            vec!["OPENAI_API_KEY"],
            vars(&[("OPENAI_API_KEY", "sk-openai")])
        );
        3
    ];
    assert_eq!(launched, expected);
}

#[test]
fn google_key_uses_each_agents_own_variable() {
    let (_dir, store) = store_with(&[(Provider::Google, "g-key")]);
    assert_eq!(
        ["gemini", "opencode", "pi"].map(|slug| launch_preferring_keys(slug, &store, &[]).0),
        [
            vec!["GEMINI_API_KEY"],
            vec!["GOOGLE_GENERATIVE_AI_API_KEY"],
            vec!["GEMINI_API_KEY"]
        ]
    );
}

#[test]
fn multi_provider_agents_get_every_saved_provider_key() {
    let (_dir, store) = store_with(&[
        (Provider::Anthropic, "a-key"),
        (Provider::Xai, "x-key"),
        (Provider::Cursor, "c-key"),
    ]);
    assert_eq!(
        launch_preferring_keys("opencode", &store, &[]),
        (
            vec!["ANTHROPIC_API_KEY", "XAI_API_KEY"],
            vars(&[("ANTHROPIC_API_KEY", "a-key"), ("XAI_API_KEY", "x-key")])
        )
    );
}

#[test]
fn ambient_credential_for_a_provider_wins_over_its_saved_key() {
    let (_dir, store) = store_with(&[(Provider::Anthropic, "a-key"), (Provider::OpenAi, "o-key")]);
    assert_eq!(
        [
            launch_preferring_keys(
                "claude-code",
                &store,
                &[("CLAUDE_CODE_OAUTH_TOKEN", "oauth")]
            ),
            launch_preferring_keys("codex", &store, &[("CODEX_API_KEY", "env")]),
            launch_preferring_keys("pi", &store, &[("OPENAI_API_KEY", "env")]),
        ],
        [
            (vec![], vars(&[])),
            (vec![], vars(&[])),
            (
                vec!["ANTHROPIC_API_KEY"],
                vars(&[("ANTHROPIC_API_KEY", "a-key")])
            ),
        ]
    );
}

#[test]
fn key_already_in_the_agent_config_wins() {
    let (_dir, store) = store_with(&[(Provider::Google, "saved")]);
    let mut config = get_agent_config("gemini").unwrap();
    config
        .env
        .insert("GEMINI_API_KEY".into(), "configured".into());

    let applied = apply_saved_keys(&mut config, &store, true, &|_| None).unwrap();

    assert_eq!(
        (applied, config.env.get("GEMINI_API_KEY").cloned()),
        (vec![], Some("configured".to_string()))
    );
}

#[test]
fn blank_ambient_variable_does_not_block_the_saved_key() {
    let (_dir, store) = store_with(&[(Provider::Cursor, "saved")]);
    assert_eq!(
        launch_preferring_keys("cursor", &store, &[("CURSOR_API_KEY", " ")]),
        (vec!["CURSOR_API_KEY"], vars(&[("CURSOR_API_KEY", "saved")]))
    );
}

#[test]
fn named_token_provider_launches_are_left_alone() {
    let (_dir, store) = store_with(&[(Provider::OpenAi, "sk-saved")]);
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

    let applied = apply_saved_keys(&mut config, &store, true, &|_| None).unwrap();

    assert_eq!((applied, config.env), (vec![], before));
}

#[test]
fn variable_the_launch_removes_is_not_reintroduced() {
    let (_dir, store) = store_with(&[(Provider::Xai, "saved")]);
    let mut config = get_agent_config("grok-build").unwrap();
    config.launch.env_remove.push("XAI_API_KEY".into());
    let before = config.env.clone();

    let applied = apply_saved_keys(&mut config, &store, true, &|_| None).unwrap();

    assert_eq!((applied, config.env), (vec![], before));
}

#[test]
fn nothing_saved_or_no_route_leaves_the_environment_unchanged() {
    let (_dir, empty) = store_with(&[]);
    let (_dir2, everything) = store_with(
        &Provider::all()
            .iter()
            .map(|provider| (*provider, "k"))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        [
            launch_preferring_keys("codex", &empty, &[]),
            launch_preferring_keys("hermes", &everything, &[]),
        ],
        [(vec![], vars(&[])), (vec![], vars(&[]))]
    );
}

#[test]
fn records_of_unknown_types_are_not_injected() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("credentials.json"),
        r#"{"anthropic":{"type":"oauth","access":"SECRET"}}"#,
    )
    .unwrap();
    let store = file_store(dir.path());
    assert_eq!(
        launch_preferring_keys("claude-code", &store, &[]),
        (vec![], vars(&[]))
    );
}

#[test]
fn custom_base_url_keeps_the_providers_saved_key_away_from_that_endpoint() {
    let (_dir, store) = store_with(&[(Provider::Anthropic, "a-key"), (Provider::OpenAi, "o-key")]);
    assert_eq!(
        [
            launch_preferring_keys(
                "claude-code",
                &store,
                &[("ANTHROPIC_BASE_URL", "https://gateway.example")]
            ),
            launch_preferring_keys(
                "opencode",
                &store,
                &[("OPENAI_BASE_URL", "https://gateway.example")]
            ),
        ],
        [
            (vec![], vars(&[])),
            (
                vec!["ANTHROPIC_API_KEY"],
                vars(&[("ANTHROPIC_API_KEY", "a-key")])
            ),
        ]
    );
}

#[test]
fn unreadable_store_launches_the_agent_without_saved_keys() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("saved-keys.json"), r#"{"codex":"file"}"#).unwrap();
    let mut config = get_agent_config("codex").unwrap();
    let before = config.env.clone();

    inject_saved_keys(&mut config, dir.path(), true, &|_| None);

    assert_eq!(config.env, before);
}
