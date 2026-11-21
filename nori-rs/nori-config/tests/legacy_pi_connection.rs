use anyhow::Result;
use nori_config::NoriConfig;
use nori_config::NoriConfigEdits;
use nori_config::token_providers::Preset;
use nori_config::token_providers::Protocol;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

const ORIGINAL: &str = r#"
agent = "codex" # keep my normal agent
[token_providers.subconscious]
preset = "subconscious"
api_key_env = "SAMPLE_PI_KEY"
[token_providers.spare]
preset = "subconscious"
api_key_env = "SPARE_PI_KEY"
[agent_token_providers]
pi = "subconscious"
[default_models]
pi = "subconscious/subconscious/glm-5.2"
codex = "native-codex-model"
"#;

#[test]
fn reads_the_original_pi_picker_configuration_and_preserves_other_agent_defaults() -> Result<()> {
    let home = tempdir()?;
    let path = home.path().join("config.toml");
    std::fs::write(&path, ORIGINAL)?;
    let config = NoriConfig::load_from_path(&path)?;
    let binding = &config.agent_token_providers["pi"];
    assert_eq!(config.active_agent, "codex");
    assert_eq!(binding.name, "subconscious");
    assert_eq!(binding.env_key, "SAMPLE_PI_KEY");
    assert_eq!(binding.protocol, Protocol::OpenaiCompletions);
    assert_eq!(binding.base_url, "https://api.subconscious.dev/v1");
    assert_eq!(binding.model, "subconscious/glm-5.2");
    assert_eq!(config.default_models["codex"], "native-codex-model");
    assert!(!config.default_models.contains_key("pi"));
    assert_eq!(std::fs::read_to_string(path)?, ORIGINAL);
    Ok(())
}

#[test]
fn adding_a_named_connection_saves_the_old_entries_and_can_restore_native_pi() -> Result<()> {
    let home = tempdir()?;
    let path = home.path().join("config.toml");
    std::fs::write(&path, ORIGINAL)?;
    NoriConfigEdits::select_token_provider(home.path(), "pi", |entries| {
        let mut new = entries
            .iter()
            .find(|entry| entry.name == "spare")
            .unwrap()
            .clone();
        new.name = "new-connection".into();
        Ok(Some(new))
    })?;
    let saved = std::fs::read_to_string(&path)?;
    assert!(saved.contains("# keep my normal agent"));
    assert!(saved.contains("[[token_providers]]"));
    let config = NoriConfig::load_from_path(&path)?;
    assert_eq!(config.agent_token_providers["pi"].name, "new-connection");
    assert_eq!(config.token_providers.len(), 3);
    assert_eq!(
        config
            .token_providers
            .iter()
            .find(|p| p.name == "subconscious")
            .unwrap()
            .model,
        "subconscious/glm-5.2"
    );
    NoriConfigEdits::select_token_provider(home.path(), "pi", |_| Ok(None))?;
    let config = NoriConfig::load_from_path(&path)?;
    assert!(config.agent_token_providers.is_empty());
    assert!(!config.default_models.contains_key("pi"));
    assert_eq!(config.default_models["codex"], "native-codex-model");
    Ok(())
}

#[test]
fn keeps_a_legacy_endpoint_override_and_preserves_a_native_pi_preference() -> Result<()> {
    let home = tempdir()?;
    let path = home.path().join("config.toml");
    let legacy = ORIGINAL
        .replace(
            "[token_providers.spare]",
            "base_url = 'http://127.0.0.1:8088/v1'\n[token_providers.spare]",
        )
        .replace("subconscious/subconscious/glm-5.2", "native/model-x");
    std::fs::write(&path, legacy)?;
    let config = NoriConfig::load_from_path(&path)?;
    let binding = &config.agent_token_providers["pi"];
    assert_eq!(binding.preset, Preset::Custom);
    assert_eq!(binding.base_url, "http://127.0.0.1:8088/v1");
    assert_eq!(binding.env_key, "SAMPLE_PI_KEY");
    assert_eq!(config.default_models["pi"], "native/model-x");
    NoriConfigEdits::select_token_provider(home.path(), "pi", |_| Ok(None))?;
    let config = NoriConfig::load_from_path(&path)?;
    assert_eq!(config.default_models["pi"], "native/model-x");
    Ok(())
}

#[test]
fn alias_binding_accepts_a_raw_wire_model_for_a_renamed_connection() -> Result<()> {
    let home = tempdir()?;
    let path = home.path().join("config.toml");
    let original = r#"
[token_providers.team]
preset = "subconscious"
api_key_env = "TEAM_KEY"
[agent_token_providers]
pi-acp = "team"
[default_models]
pi-acp = "subconscious/glm-5.2"
"#;
    std::fs::write(&path, original)?;
    let config = NoriConfig::load_from_path(&path)?;
    assert_eq!(
        config.agent_token_providers["pi"].model,
        "subconscious/glm-5.2"
    );
    assert_eq!(config.agent_token_providers["pi"].name, "team");
    NoriConfigEdits::select_token_provider(home.path(), "pi", |_| Ok(None))?;
    let config = NoriConfig::load_from_path(&path)?;
    assert!(config.default_models.is_empty());
    assert!(config.agent_token_providers.is_empty());
    Ok(())
}

#[test]
fn rejects_literal_keys_without_disclosing_or_rewriting_them() -> Result<()> {
    let home = tempdir()?;
    let path = home.path().join("config.toml");
    let invalid = ORIGINAL.replace(
        "api_key_env = \"SAMPLE_PI_KEY\"",
        "api_key = \"synthetic-sensitive-fixture\"",
    );
    std::fs::write(&path, &invalid)?;
    let error = NoriConfig::load_from_path(&path).unwrap_err();
    assert!(!format!("{error:#}").contains("synthetic-sensitive-fixture"));
    let error =
        NoriConfigEdits::select_token_provider(home.path(), "pi", |_| Ok(None)).unwrap_err();
    assert!(!format!("{error:#}").contains("synthetic-sensitive-fixture"));
    assert_eq!(std::fs::read_to_string(path)?, invalid);
    Ok(())
}
