#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn bound_exec_uses_only_the_explicit_responses_route() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path();
    let runner = path.join("npx");
    std::fs::write(
        &runner,
        r#"#!/usr/bin/env python3
import json, os, sys
cfg = json.loads(os.environ["CODEX_CONFIG"])
provider = cfg["model_providers"][os.environ["MODEL_PROVIDER"]]
assert provider["wire_api"] == "responses" and provider["base_url"] == "https://example.invalid/v1"
assert cfg["model"] == "sample-model" and os.environ[provider["env_key"]] == "fixture-selected"
assert "fixture-selected" not in os.environ["CODEX_CONFIG"]
assert not any(os.environ.get(k) for k in ["OPENAI_API_KEY", "CODEX_API_KEY", "DEFAULT_AUTH_REQUEST"])
open(os.environ["PROBE"], "w").write("selected")
sys.stderr.write("adapter startup fixture-selected\n")
sys.exit(1)
"#,
    )?;
    std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755))?;
    std::fs::write(
        path.join("config.toml"),
        r#"
[agent_token_providers]
codex = "lab"
[default_models]
codex = "native-default"
[[token_providers]]
name = "lab"
preset = "custom"
protocol = "openai-responses"
base_url = "https://example.invalid/v1"
model = "sample-model"
env_key = "EXAMPLE_ROUTE_KEY"
"#,
    )?;
    let probe = path.join("probe");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_nori"));
    cmd.env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", path.display()))
        .env("HOME", path)
        .env("NORI_HOME", path)
        .env("NORI_MANAGED_BY_NPM", "1")
        .env("PROBE", &probe)
        .env("OPENAI_API_KEY", "fixture-competing")
        .env("CODEX_API_KEY", "fixture-competing")
        .env("DEFAULT_AUTH_REQUEST", "fixture-competing")
        .args(["--agent", "codex-acp", "exec", "hello"]);
    for value in [None, Some("  "), Some("fixture-selected")] {
        if let Some(value) = value {
            cmd.env("EXAMPLE_ROUTE_KEY", value);
        }
        let output = cmd.output()?;
        assert!(!output.status.success());
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        assert!(
            !diagnostics.contains("fixture-selected") && !diagnostics.contains("fixture-competing")
        );
        if value == Some("fixture-selected") {
            assert_eq!(std::fs::read_to_string(&probe)?, "selected");
            assert!(diagnostics.contains("adapter startup"), "{diagnostics}");
        } else {
            assert!(diagnostics.contains("EXAMPLE_ROUTE_KEY"), "{diagnostics}");
            assert!(!probe.exists());
        }
    }
    std::fs::remove_file(&probe)?;
    let config_path = path.join("config.toml");
    let saved = std::fs::read_to_string(&config_path)?;
    assert!(!saved.contains("fixture-selected"));
    std::fs::write(
        &config_path,
        format!(
            "{saved}\n[[agents]]\nname='Custom'\nslug='codex'\n[agents.distribution.local]\ncommand='npx'\n"
        ),
    )?;
    let shadow = cmd.output()?;
    assert!(!shadow.status.success());
    assert!(
        String::from_utf8_lossy(&shadow.stderr).contains("no named token provider launch adapter")
    );
    assert!(!probe.exists());
    Ok(())
}

#[test]
fn named_binding_reports_eligibility_and_adapter_failures_before_spawn() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let path = dir.path();
    for (slug, protocol, expected) in [
        ("hermes", "openai-responses", "protocol-mismatch"),
        (
            "hermes",
            "openai-completions",
            "no named token provider launch adapter",
        ),
        (
            "pi",
            "openai-responses",
            "no named token provider launch adapter",
        ),
    ] {
        let config = format!(
            r#"
[agent_token_providers]
{slug} = "lab"

[[agents]]
name = "Fixture"
slug = "{slug}"
[agents.distribution.local]
command = "fixture-command-must-not-run"

[[token_providers]]
name = "lab"
preset = "custom"
protocol = "{protocol}"
base_url = "http://127.0.0.1:9/v1"
env_key = "FIXTURE_KEY"
model = "fixture-model"
"#
        );
        std::fs::write(path.join("config.toml"), config)?;
        let output = Command::new(env!("CARGO_BIN_EXE_nori"))
            .env_clear()
            .env("HOME", path)
            .env("NORI_HOME", path)
            .env("PATH", path)
            .env("NORI_MANAGED_BY_NPM", "1")
            .args(["--agent", slug, "exec", "hello"])
            .output()?;
        let diagnostics = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(
            diagnostics.contains(expected),
            "{slug} {protocol}: {diagnostics}"
        );
    }
    Ok(())
}
