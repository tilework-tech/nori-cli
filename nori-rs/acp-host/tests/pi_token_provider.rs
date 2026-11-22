//! Built-in Pi accepts native sign-in and named API connections.
use nori_acp_host::registry::get_agent_config;
use nori_acp_host::registry::get_agent_launch_config;
use nori_acp_host::registry::list_available_agents;
use nori_acp_host::token_provider::bound_model_id;
use nori_acp_host::token_provider::prepare_pi_profile;
use nori_config::token_providers::Preset;
use nori_config::token_providers::Protocol;
use nori_config::token_providers::TokenProvider;
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

fn connection(variable: &str) -> TokenProvider {
    TokenProvider {
        name: "lab".into(),
        preset: Preset::Custom,
        protocol: Protocol::OpenaiCompletions,
        base_url: "http://127.0.0.1:4567/v1".into(),
        env_key: variable.into(),
        model: "lab-small".into(),
    }
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
fn a_named_connection_writes_only_the_selected_model_and_a_key_reference() {
    let variable = "SYNTHETIC_API_KEY";
    let secret = "synthetic-0001";
    let nori_home = tempfile::tempdir().unwrap();
    let binding = connection(variable);
    assert_eq!(bound_model_id("pi", &binding), "lab/lab-small");
    let mut config = get_agent_launch_config("pi-acp", Some(&binding)).unwrap();
    prepare_pi_profile(&mut config, nori_home.path(), &|name| {
        (name == variable).then(|| secret.to_owned())
    })
    .unwrap();

    let agent_dir = Path::new(config.env.get("PI_CODING_AGENT_DIR").expect("agent dir"));
    assert!(agent_dir.starts_with(nori_home.path()));
    let models = read_json(&agent_dir.join("models.json"));
    let provider = &models["providers"]["lab"];
    assert_eq!(provider["baseUrl"], binding.base_url);
    assert_eq!(provider["api"], "openai-completions");
    assert_eq!(provider["apiKey"], format!("${variable}"));
    assert!(provider.get("headers").is_none());
    let model_ids: Vec<&str> = provider["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|model| model["id"].as_str().unwrap())
        .collect();
    assert_eq!(model_ids, ["lab-small"]);
    let settings = read_json(&agent_dir.join("settings.json"));
    assert_eq!(settings["defaultProvider"], "lab");
    assert_eq!(settings["defaultModel"], "lab-small");

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
    let binding = connection(variable);
    let mut config = get_agent_launch_config("pi", Some(&binding)).unwrap();
    let error = prepare_pi_profile(&mut config, home.path(), &|_| None).unwrap_err();
    assert!(error.to_string().contains(variable));
    assert!(!config.env.contains_key("PI_CODING_AGENT_DIR"));
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}

#[test]
fn profile_uses_the_protocol_resolved_for_the_saved_connection() {
    let mut binding = connection("SYNTHETIC_PROXY_KEY");
    binding.protocol = Protocol::OpenaiResponses;
    let mut config = get_agent_launch_config("pi", Some(&binding)).unwrap();
    let home = tempfile::tempdir().unwrap();
    prepare_pi_profile(&mut config, home.path(), &|_| Some("synthetic".into())).unwrap();
    let dir = Path::new(&config.env["PI_CODING_AGENT_DIR"]);
    assert_eq!(
        read_json(&dir.join("models.json"))["providers"]["lab"]["api"],
        "openai-responses"
    );
}

#[test]
fn concurrent_profiles_keep_legacy_history_and_clean_up_independently() {
    let home = tempfile::tempdir().unwrap();
    let history = home.path().join("token-providers/lab-pi-acp/sessions");
    std::fs::create_dir_all(&history).unwrap();
    std::fs::write(history.join("existing.jsonl"), "synthetic-history").unwrap();
    let prepare = || {
        let mut config = get_agent_launch_config("pi", Some(&connection("LAB_KEY"))).unwrap();
        prepare_pi_profile(&mut config, home.path(), &|_| Some("test".into())).unwrap();
        let path = std::path::PathBuf::from(&config.env["PI_CODING_AGENT_DIR"]);
        assert_eq!(config.launch.runtime_dir.as_deref(), Some(path.as_path()));
        (config, path)
    };
    let (first, first_path) = prepare();
    let (second, second_path) = prepare();
    assert_ne!(first_path, second_path);
    assert_eq!(
        read_json(&first_path.join("settings.json"))["sessionDir"],
        serde_json::json!(history)
    );
    let summary_launch = first.clone();
    drop(first);
    assert!(first_path.exists());
    drop(summary_launch);
    assert!(!first_path.exists());
    assert!(second_path.join("models.json").is_file());
    drop(second);
    assert!(!second_path.exists());
    assert_eq!(
        std::fs::read_to_string(history.join("existing.jsonl")).unwrap(),
        "synthetic-history"
    );
}

#[test]
fn a_crashed_launch_is_swept_when_an_unbound_agent_starts() {
    use std::process::Command;
    let helper = "NORI_TEST_ABANDON_PROFILE";
    if let Some(home) = std::env::var_os(helper) {
        let home = Path::new(&home);
        let mut config = get_agent_launch_config("pi", Some(&connection("LAB_KEY"))).unwrap();
        prepare_pi_profile(&mut config, home, &|_| Some("test".into())).unwrap();
        std::fs::write(home.join("path"), &config.env["PI_CODING_AGENT_DIR"]).unwrap();
        std::process::exit(0);
    }
    let home = tempfile::tempdir().unwrap();
    let status = Command::new(std::env::current_exe().unwrap())
        .env(helper, home.path())
        .args([
            "--exact",
            "a_crashed_launch_is_swept_when_an_unbound_agent_starts",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    let old = std::path::PathBuf::from(std::fs::read_to_string(home.path().join("path")).unwrap());
    assert!(old.join("settings.json").is_file());
    let unrelated = old
        .parent()
        .unwrap()
        .join("369510de-670d-49e4-a0fa-bf50c2689e9c");
    std::fs::create_dir(&unrelated).unwrap();
    std::fs::write(unrelated.with_extension("lease"), "caller-owned").unwrap();
    let mut config = get_agent_config("pi").unwrap();
    prepare_pi_profile(&mut config, home.path(), &|_| {
        panic!("native Pi needs no binding")
    })
    .unwrap();
    assert!(!old.exists());
    assert!(unrelated.is_dir());
}

#[test]
fn messages_profiles_keep_a_gateway_prefix_without_duplicating_the_api_version() {
    let mut binding = connection("TEST_MESSAGES_KEY");
    binding.protocol = Protocol::AnthropicMessages;
    binding.base_url = "http://127.0.0.1:4567/example/v1/".into();
    let mut config = get_agent_launch_config("pi", Some(&binding)).unwrap();
    let home = tempfile::tempdir().unwrap();
    prepare_pi_profile(&mut config, home.path(), &|_| Some("fixture".into())).unwrap();
    let dir = Path::new(&config.env["PI_CODING_AGENT_DIR"]);
    let profile = read_json(&dir.join("models.json"));
    assert_eq!(
        profile["providers"]["lab"]["baseUrl"],
        "http://127.0.0.1:4567/example"
    );
    assert_eq!(profile["providers"]["lab"]["api"], "anthropic-messages");
}
