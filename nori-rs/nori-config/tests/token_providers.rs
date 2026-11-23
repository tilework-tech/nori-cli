use nori_config::NoriConfig;
use pretty_assertions::assert_eq;
use tempfile::tempdir;

fn load(agent: &str, preset: &str, protocol: &str, extra: &str) -> anyhow::Result<NoriConfig> {
    let dir = tempdir()?;
    let path = dir.path().join("config.toml");
    let endpoint = match preset {
        "subconscious" => "https://api.subconscious.dev/v1",
        "cloudflare" => {
            "https://api.cloudflare.com/client/v4/accounts/00000000000000000000000000000000/ai/v1"
        }
        "openrouter" => "https://openrouter.ai/api/v1",
        _ => "https://example.invalid/v1",
    };
    std::fs::write(
        &path,
        format!(
            r#"
[agent_token_providers]
{agent} = "lab"

[[token_providers]]
name = "lab"
preset = "{preset}"
protocol = "{protocol}"
base_url = "{endpoint}"
model = "example/model"
env_key = "EXAMPLE_ROUTE_KEY"
{extra}
"#
        ),
    )?;
    NoriConfig::load_from_path(&path)
}

#[test]
fn known_gateway_negotiates_within_one_connection() -> anyhow::Result<()> {
    for (agent, expected) in [
        ("pi", "openai-completions"),
        ("opencode", "openai-completions"),
        ("codex", "openai-responses"),
        ("claude-code", "anthropic-messages"),
    ] {
        let cfg = load(agent, "subconscious", "openai-responses", "")?;
        let selection = &cfg.agent_token_providers[agent];
        assert_eq!(selection.protocol.as_str(), expected);
        assert_eq!(selection.env_key, "EXAMPLE_ROUTE_KEY");
        assert_eq!(selection.model, "example/model");
    }
    Ok(())
}

#[test]
fn incompatible_routes_and_subscription_credentials_fail_closed() {
    for (agent, preset, protocol) in [
        ("codex", "custom", "openai-completions"),
        ("pi", "cloudflare", "openai-responses"),
        ("codex", "chatgpt", "openai-responses"),
        ("cursor", "custom", "openai-responses"),
    ] {
        assert!(
            load(agent, preset, protocol, "").is_err(),
            "{agent} {preset}"
        );
    }
}

#[test]
fn rejects_literal_secret_fields_without_echoing_input() -> anyhow::Result<()> {
    let error = load(
        "codex",
        "custom",
        "openai-responses",
        r#"api_key = "sensitive-fixture-marker""#,
    )
    .err()
    .ok_or_else(|| anyhow::anyhow!("invalid field must fail"))?;
    assert!(!format!("{error:#}").contains("sensitive-fixture-marker"));
    Ok(())
}

#[test]
fn messages_auth_is_specific_to_the_selected_gateway() -> anyhow::Result<()> {
    let gateway = load("claude-code", "openrouter", "openai-responses", "")?;
    let custom = load("claude-code", "custom", "anthropic-messages", "")?;
    assert!(gateway.agent_token_providers["claude-code"].bearer_messages());
    assert!(!custom.agent_token_providers["claude-code"].bearer_messages());
    Ok(())
}

#[test]
fn provisional_connections_need_a_concrete_local_route() -> anyhow::Result<()> {
    let custom = load("hermes", "custom", "openai-completions", "")?;
    assert_eq!(
        custom.agent_token_providers["hermes"].protocol.as_str(),
        "openai-completions"
    );
    for (preset, reason) in [
        ("custom", "protocol-mismatch"),
        ("openrouter", "no concrete protocol"),
    ] {
        let result = load("hermes", preset, "openai-responses", "");
        let error = result.expect_err("no usable named connection");
        assert!(format!("{error:#}").contains(reason), "{error:#}");
    }
    Ok(())
}

#[test]
fn binding_keys_use_the_registry_canonical_slug() {
    let result = load("codex-acp", "custom", "openai-responses", "");
    let error = result.expect_err("alias binding would be missed at launch");
    assert!(format!("{error:#}").contains("canonical agent slug"));
}
