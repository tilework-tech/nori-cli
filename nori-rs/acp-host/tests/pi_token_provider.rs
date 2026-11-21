//! Pi launches as a built-in and can be bound to a token provider such as
//! Subconscious without the key ever reaching a file.
use nori_acp_host::registry::get_agent_config;
use nori_acp_host::registry::get_agent_launch_config;
use nori_acp_host::registry::list_available_agents;
use nori_acp_host::token_provider::bound_model_id;
use nori_acp_host::token_provider::compatible_presets;
use nori_acp_host::token_provider::prepare_pi_profile;
use nori_acp_host::token_provider::preset;
use nori_acp_host::token_provider::supports_pi_connection;
use pretty_assertions::assert_eq;
use serde_json::Value;
use std::path::Path;

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
    let mut config = get_agent_config("pi").unwrap();
    let home = tempfile::tempdir().unwrap();
    prepare_pi_profile(&mut config, home.path(), &|_| {
        panic!("native Pi must not read a named key")
    })
    .unwrap();
    assert!(config.token_provider.is_none());
    assert!(!config.env.contains_key("PI_CODING_AGENT_DIR"));
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}

#[test]
fn a_subconscious_binding_points_pi_at_a_private_directory_that_names_the_key_variable() {
    let variable = "NORI_TEST_SUBC_KEY_PRESENT";
    let secret = "subc-synthetic-key-0001";
    let nori_home = tempfile::tempdir().unwrap();
    let binding =
        preset("subconscious")
            .unwrap()
            .connection("lab", variable, "subconscious/glm-5.2");
    assert_eq!(bound_model_id("pi", &binding), "lab/subconscious/glm-5.2");
    let mut config = get_agent_launch_config("pi-acp", Some(&binding)).unwrap();
    prepare_pi_profile(&mut config, nori_home.path(), &|name| {
        (name == variable).then(|| secret.to_owned())
    })
    .unwrap();

    let agent_dir = Path::new(config.env.get("PI_CODING_AGENT_DIR").expect("agent dir"));
    assert!(agent_dir.starts_with(nori_home.path()));
    let models = read_json(&agent_dir.join("models.json"));
    let provider = &models["providers"]["lab"];
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
    assert_eq!(settings["defaultProvider"], "lab");
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
fn a_missing_credential_stops_the_launch_and_names_the_variable() {
    let variable = "NORI_TEST_SUBC_KEY_ABSENT";
    let home = tempfile::tempdir().unwrap();
    let preset = preset("subconscious").unwrap();
    let binding = preset.connection("another-name", variable, preset.default_model);
    let mut config = get_agent_launch_config("pi", Some(&binding)).unwrap();
    let error = prepare_pi_profile(&mut config, home.path(), &|_| None).unwrap_err();
    assert!(error.to_string().contains(variable));
    assert!(!config.env.contains_key("PI_CODING_AGENT_DIR"));
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}

#[test]
fn named_connections_must_match_the_contract_endpoint_and_pi_catalog() {
    let preset = preset("subconscious").unwrap();
    let mut binding = preset.connection("team_gateway", "SUBC_KEY", preset.default_model);
    assert!(supports_pi_connection(&binding));
    binding.model = "other-model".into();
    assert!(!supports_pi_connection(&binding));
    binding.model = preset.default_model.into();
    binding.base_url = "https://example.invalid/v1".into();
    assert!(!supports_pi_connection(&binding));
}

#[test]
fn provider_ids_must_be_safe_directory_names() {
    let preset = preset("subconscious").unwrap();
    let binding = preset.connection("../outside", "SUBC_KEY", preset.default_model);
    assert!(!supports_pi_connection(&binding));
}
