//! Pi launches as a built-in and can be bound to a token provider such as
//! Subconscious without the key ever reaching a file.
use nori_acp_host::registry::AgentKind;
use nori_acp_host::registry::get_agent_config;
use nori_acp_host::registry::list_available_agents;
use nori_acp_host::token_provider::TokenProviderSelection;
use nori_acp_host::token_provider::apply_token_provider;
use nori_acp_host::token_provider::compatible_presets;
use nori_acp_host::token_provider::resolve_token_provider;
use nori_config::TokenProviderToml;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

fn subconscious(api_key_env: &str) -> BTreeMap<String, TokenProviderToml> {
    BTreeMap::from([(
        "subconscious".to_string(),
        TokenProviderToml {
            preset: "subconscious".to_string(),
            api_key_env: api_key_env.to_string(),
            base_url: None,
        },
    )])
}

fn pi_bound_to(provider: &str) -> BTreeMap<String, String> {
    BTreeMap::from([("pi".to_string(), provider.to_string())])
}

fn present(name: &'static str, value: &'static str) -> impl Fn(&str) -> Option<String> {
    move |requested| (requested == name).then(|| value.to_string())
}

#[expect(
    clippy::unwrap_used,
    reason = "the test wrote these files moments earlier"
)]
fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn pi_is_a_built_in_that_runs_the_upstream_adapter_with_pi_on_path() {
    assert!(
        list_available_agents()
            .iter()
            .any(|agent| agent.agent_name == "pi" && agent.display_name == "Pi")
    );
    for name in ["pi", "pi-acp", "PI"] {
        let config = get_agent_config(name).unwrap();
        assert_eq!(config.provider_slug, "pi");
        assert_eq!(config.command, "npx");
        assert_eq!(
            config.args,
            [
                "--yes",
                "--package=pi-acp@latest",
                "--package=@earendil-works/pi-coding-agent@latest",
                "pi-acp",
            ]
        );
    }
}

#[test]
fn only_pi_offers_subconscious_in_this_cli() {
    let ids = |agent: &str| {
        compatible_presets(agent)
            .iter()
            .map(|preset| preset.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids("pi"), ["subconscious"]);
    assert_eq!(ids("pi-acp"), ["subconscious"]);
    assert!(ids("claude-code").is_empty());
    assert!(ids("codex").is_empty());
}

#[test]
fn an_unbound_pi_keeps_its_own_configuration() {
    let selection = resolve_token_provider(
        "pi",
        Some(AgentKind::Pi),
        &subconscious("SUBC_KEY"),
        &BTreeMap::new(),
        None,
    )
    .unwrap();
    assert_eq!(selection, None);
}

#[test]
fn a_subconscious_binding_points_pi_at_a_private_directory_that_names_the_key_variable() {
    let variable = "NORI_TEST_SUBC_KEY_PRESENT";
    let secret = "subc-synthetic-key-0001";
    let nori_home = tempfile::tempdir().unwrap();

    let selection = resolve_token_provider(
        "pi",
        Some(AgentKind::Pi),
        &subconscious(variable),
        &pi_bound_to("subconscious"),
        Some("subconscious/subconscious/glm-5.2"),
    )
    .unwrap()
    .expect("explicit binding");
    let mut config = get_agent_config("pi").unwrap();
    apply_token_provider(
        &mut config,
        &selection,
        nori_home.path(),
        &present(variable, secret),
    )
    .unwrap();

    let agent_dir = Path::new(config.env.get("PI_CODING_AGENT_DIR").expect("agent dir"));
    assert!(agent_dir.starts_with(nori_home.path()));
    let models = read_json(&agent_dir.join("models.json"));
    let provider = &models["providers"]["subconscious"];
    assert_eq!(provider["baseUrl"], "https://api.subconscious.dev/v1");
    assert_eq!(provider["api"], "openai-completions");
    assert_eq!(provider["apiKey"], format!("${variable}"));
    assert_eq!(provider["headers"]["x-subconscious-client"], "pi");
    let model_ids: Vec<&str> = provider["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["id"].as_str().unwrap())
        .collect();
    assert!(model_ids.contains(&"subconscious/glm-5.3-marathon"));
    assert!(model_ids.contains(&"subconscious/glm-5.2"));
    let settings = read_json(&agent_dir.join("settings.json"));
    assert_eq!(settings["defaultProvider"], "subconscious");
    assert_eq!(settings["defaultModel"], "subconscious/glm-5.2");

    assert!(config.env.values().all(|value| !value.contains(secret)));
    let mut pending = vec![nori_home.path().to_path_buf()];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(
                std::fs::read_dir(&path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
        } else {
            assert!(!std::fs::read_to_string(&path).unwrap().contains(secret));
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&path).unwrap().permissions().mode();
                assert_eq!(mode & 0o077, 0, "{} is readable by others", path.display());
            }
        }
    }
}

#[test]
fn a_binding_without_a_saved_model_starts_on_the_preset_default() {
    let variable = "NORI_TEST_SUBC_KEY_DEFAULT_MODEL";
    let nori_home = tempfile::tempdir().unwrap();
    let selection = resolve_token_provider(
        "pi",
        Some(AgentKind::Pi),
        &subconscious(variable),
        &pi_bound_to("subconscious"),
        None,
    )
    .unwrap()
    .unwrap();
    let mut config = get_agent_config("pi").unwrap();
    apply_token_provider(
        &mut config,
        &selection,
        nori_home.path(),
        &present(variable, "subc-synthetic-key-0002"),
    )
    .unwrap();
    let agent_dir = Path::new(config.env.get("PI_CODING_AGENT_DIR").unwrap());
    let settings = read_json(&agent_dir.join("settings.json"));
    assert_eq!(settings["defaultModel"], "subconscious/glm-5.3-marathon");
}

#[test]
fn a_missing_credential_stops_the_launch_and_names_the_variable() {
    let variable = "NORI_TEST_SUBC_KEY_ABSENT";
    let nori_home = tempfile::tempdir().unwrap();
    let selection: TokenProviderSelection = resolve_token_provider(
        "pi",
        Some(AgentKind::Pi),
        &subconscious(variable),
        &pi_bound_to("subconscious"),
        None,
    )
    .unwrap()
    .unwrap();
    let mut config = get_agent_config("pi").unwrap();

    let error = apply_token_provider(&mut config, &selection, nori_home.path(), &|_| None)
        .unwrap_err()
        .to_string();
    assert!(error.contains(variable), "{error}");
    assert!(error.contains("Subconscious"), "{error}");
    assert!(!config.env.contains_key("PI_CODING_AGENT_DIR"));
    assert_eq!(std::fs::read_dir(nori_home.path()).unwrap().count(), 0);
}

#[test]
fn explicit_bindings_that_cannot_be_honored_are_errors() {
    let providers = subconscious("SUBC_KEY");
    let unknown = resolve_token_provider(
        "pi",
        Some(AgentKind::Pi),
        &providers,
        &pi_bound_to("missing"),
        None,
    );
    assert!(unknown.unwrap_err().to_string().contains("missing"));

    let unsupported = resolve_token_provider(
        "claude-code",
        Some(AgentKind::ClaudeCode),
        &providers,
        &BTreeMap::from([("claude-code".to_string(), "subconscious".to_string())]),
        None,
    );
    assert!(unsupported.unwrap_err().to_string().contains("Claude Code"));

    let foreign_model = resolve_token_provider(
        "pi",
        Some(AgentKind::Pi),
        &providers,
        &pi_bound_to("subconscious"),
        Some("subconscious/not-a-subconscious-model"),
    );
    assert!(
        foreign_model
            .unwrap_err()
            .to_string()
            .contains("not-a-subconscious-model")
    );
}

#[test]
fn a_catalog_model_id_without_the_provider_prefix_is_accepted() {
    let selection = resolve_token_provider(
        "pi",
        Some(AgentKind::Pi),
        &subconscious("SUBC_KEY"),
        &pi_bound_to("subconscious"),
        Some("subconscious/glm-5.2"),
    )
    .unwrap()
    .unwrap();
    assert_eq!(selection.model.id, "subconscious/glm-5.2");
}

#[test]
fn a_custom_agent_that_reuses_the_pi_slug_cannot_be_bound() {
    let error = resolve_token_provider(
        "pi",
        None,
        &subconscious("SUBC_KEY"),
        &pi_bound_to("subconscious"),
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("custom"), "{error}");
}

#[test]
fn provider_ids_must_be_safe_directory_names() {
    let providers = BTreeMap::from([(
        "../outside".to_string(),
        TokenProviderToml {
            preset: "subconscious".to_string(),
            api_key_env: "SUBC_KEY".to_string(),
            base_url: None,
        },
    )]);
    let error = resolve_token_provider(
        "pi",
        Some(AgentKind::Pi),
        &providers,
        &pi_bound_to("../outside"),
        None,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("../outside"), "{error}");
}
