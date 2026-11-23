#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;

use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

const SECRET: &str = "sk-fixture-SECRET";

fn nori(home: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    nori_with_env(home, &[], args, stdin)
}

fn nori_with_env(home: &Path, env: &[(&str, &str)], args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_nori"))
        .env_clear()
        .envs(env.iter().copied())
        .env("PATH", format!("{}:/usr/bin:/bin", home.display()))
        .env("HOME", home)
        .env("NORI_HOME", home.join("nori"))
        .env("NORI_MANAGED_BY_NPM", "1")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    if let Some(input) = stdin {
        pipe.write_all(input.as_bytes()).unwrap();
    }
    drop(pipe);
    let output = child.wait_with_output().unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!all.contains("SECRET"), "secret leaked: {all}");
    output
}

fn agent(report: &Value, slug: &str) -> Value {
    report["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["agent"] == slug)
        .cloned()
        .unwrap()
}

fn report(home: &Path) -> Value {
    let output = nori(home, &["auth", "--json"], None);
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn auth_json_reports_a_codex_chatgpt_login() {
    let dir = tempfile::tempdir().unwrap();
    let auth = dir.path().join(".codex/auth.json");
    std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
    std::fs::write(
        &auth,
        format!(r#"{{"auth_mode":"chatgpt","tokens":{{"access_token":"{SECRET}"}}}}"#),
    )
    .unwrap();

    let codex = agent(&report(dir.path()), "codex");

    assert_eq!(
        (&codex["status"], &codex["credentials"]),
        (
            &json!("signed_in"),
            &json!([{
                "kind": "subscription",
                "source": {"type": "file", "path": auth},
                "provider": "openai",
                "detail": "chatgpt",
                "read_only": true,
                "disconnect": "sign out in Codex to disconnect"
            }])
        )
    );
}

#[test]
fn auth_table_names_each_agent_and_its_status() {
    let dir = tempfile::tempdir().unwrap();
    let output = nori(dir.path(), &["auth"], None);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    let row = |slug: &str| {
        text.lines()
            .find(|line| line.split_whitespace().next() == Some(slug))
            .unwrap_or_else(|| panic!("no {slug} row: {text}"))
            .to_string()
    };
    assert!(row("cursor").contains("unknown"), "{text}");
    assert!(row("codex").contains("signed out"), "{text}");
}

#[test]
fn one_saved_openai_key_is_reported_for_every_openai_agent_and_can_be_removed() {
    let dir = tempfile::tempdir().unwrap();
    for slug in ["codex", "opencode", "pi"] {
        let preferred = nori(dir.path(), &["auth", "prefer-api-key", slug], None);
        assert!(preferred.status.success(), "{preferred:?}");
    }

    let saved = nori(
        dir.path(),
        &["auth", "set-key", "openai", "--json"],
        Some(&format!("{SECRET}\n")),
    );
    assert!(saved.status.success(), "{saved:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&saved.stdout).unwrap(),
        json!({
            "provider": "openai",
            "type": "api_key",
            "store": "file",
            "agents": [
                {"agent": "codex", "env": "OPENAI_API_KEY"},
                {"agent": "opencode", "env": "OPENAI_API_KEY"},
                {"agent": "pi", "env": "OPENAI_API_KEY"},
            ]
        })
    );
    let saved_credential = json!({
        "kind": "api_key",
        "source": {"type": "nori_saved", "provider": "openai", "env": "OPENAI_API_KEY", "store": "file"},
        "provider": "openai",
        "read_only": false,
        "disconnect": "run `nori auth remove-key openai` to disconnect"
    });
    let after_save = report(dir.path());
    assert_eq!(
        ["codex", "opencode", "pi"].map(|slug| agent(&after_save, slug)["credentials"].clone()),
        [
            json!([saved_credential]),
            json!([saved_credential]),
            json!([saved_credential])
        ]
    );

    let removed = nori(dir.path(), &["auth", "remove-key", "openai"], None);
    assert!(removed.status.success(), "{removed:?}");
    assert_eq!(
        agent(&report(dir.path()), "codex")["credentials"],
        json!([])
    );
}

#[test]
fn env_credentials_are_read_only_in_the_table() {
    let dir = tempfile::tempdir().unwrap();
    let output = nori_with_env(dir.path(), &[("XAI_API_KEY", SECRET)], &["auth"], None);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    let row = text
        .lines()
        .find(|line| line.starts_with("grok-build"))
        .unwrap();
    assert!(row.contains("unset XAI_API_KEY to disconnect"), "{row}");
}

#[test]
fn set_key_takes_a_provider_and_points_agent_names_at_theirs() {
    let dir = tempfile::tempdir().unwrap();
    let output = nori(
        dir.path(),
        &["auth", "set-key", "codex"],
        Some(&format!("{SECRET}\n")),
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("openai"), "{output:?}");

    let output = nori(dir.path(), &["auth", "set-key", "nope"], Some(SECRET));
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not a provider"),
        "{output:?}"
    );
    assert!(!dir.path().join("nori/credentials.json").exists());
}

/// Install a fake `npx` that records whether Codex was launched with the saved
/// OpenAI key, then exits.
fn install_codex_probe(home: &Path) {
    let runner = home.join("npx");
    std::fs::write(
        &runner,
        format!(
            "#!/bin/sh\nif [ \"$OPENAI_API_KEY\" = \"{SECRET}\" ]; then echo saved; else echo launched-without-key; fi > \"$HOME/probe\"\nexit 1\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn launch_codex(home: &Path) -> String {
    let _ = std::fs::remove_file(home.join("probe"));
    nori(home, &["--agent", "codex", "exec", "hello"], None);
    std::fs::read_to_string(home.join("probe")).unwrap()
}

#[test]
fn saved_key_reaches_the_launched_agent_only_while_it_prefers_api_keys() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    install_codex_probe(home);
    assert!(
        nori(home, &["auth", "set-key", "openai"], Some(SECRET))
            .status
            .success()
    );
    let by_default = launch_codex(home);

    let preferred = nori(home, &["auth", "prefer-api-key", "codex"], None);
    assert!(preferred.status.success(), "{preferred:?}");
    let after_opt_in = launch_codex(home);

    let back = nori(home, &["auth", "prefer-login", "codex"], None);
    assert!(back.status.success(), "{back:?}");
    let after_opt_out = launch_codex(home);

    assert_eq!(
        [by_default, after_opt_in, after_opt_out],
        [
            "launched-without-key\n",
            "saved\n",
            "launched-without-key\n"
        ]
        .map(str::to_string)
    );
}

#[test]
fn preferring_api_keys_over_a_subscription_needs_confirmation_once() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let auth = home.join(".codex/auth.json");
    std::fs::create_dir_all(auth.parent().unwrap()).unwrap();
    std::fs::write(
        &auth,
        format!(r#"{{"auth_mode":"chatgpt","tokens":{{"access_token":"{SECRET}"}}}}"#),
    )
    .unwrap();

    let plain = nori(home, &["auth", "prefer-api-key", "codex"], None);
    let unconfirmed = nori(home, &["auth", "prefer-api-key", "codex", "--json"], None);
    let still_login = agent(&report(home), "codex")["prefer_api_key"].clone();
    let confirmed = nori(
        home,
        &["auth", "prefer-api-key", "codex", "--yes", "--json"],
        None,
    );
    let switched_back = nori(home, &["auth", "prefer-login", "codex"], None);
    let remembered = nori(home, &["auth", "prefer-api-key", "codex", "--json"], None);

    assert!(!plain.status.success());
    assert!(
        String::from_utf8_lossy(&plain.stderr).contains("--yes"),
        "{plain:?}"
    );
    assert!(!unconfirmed.status.success());
    let refusal = serde_json::from_slice::<Value>(&unconfirmed.stdout).unwrap();
    assert_eq!(
        (&refusal["error"], &refusal["code"]),
        (
            &json!("confirmation_required"),
            &json!("api_key_overrides_subscription")
        )
    );
    assert_eq!(still_login, json!(false));
    assert!(confirmed.status.success(), "{confirmed:?}");
    assert!(switched_back.status.success(), "{switched_back:?}");
    assert!(remembered.status.success(), "{remembered:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&remembered.stdout).unwrap(),
        json!({"agent": "codex", "prefer_api_key": true, "api_billing_confirmed": true})
    );
    assert_eq!(agent(&report(home), "codex")["prefer_api_key"], json!(true));
}

#[test]
fn prefer_api_key_rejects_provider_names_and_agents_without_saved_keys() {
    let dir = tempfile::tempdir().unwrap();
    let outputs = [
        nori(dir.path(), &["auth", "prefer-api-key", "openai"], None),
        nori(dir.path(), &["auth", "prefer-api-key", "hermes"], None),
    ];
    let errors = outputs.map(|output| {
        assert!(!output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stderr).to_string()
    });
    assert!(errors[0].contains("codex"), "{}", errors[0]);
    assert!(errors[1].contains("own login"), "{}", errors[1]);
    assert!(!dir.path().join("nori/config.toml").exists());
}

#[test]
fn prefer_api_key_accepts_an_agent_named_like_its_provider() {
    let dir = tempfile::tempdir().unwrap();
    let output = nori(
        dir.path(),
        &["auth", "prefer-api-key", "cursor", "--json"],
        None,
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        agent(&report(dir.path()), "cursor")["prefer_api_key"],
        json!(true)
    );
}

#[test]
fn set_key_shows_only_the_known_prefix_and_flags_a_key_for_another_provider() {
    let dir = tempfile::tempdir().unwrap();
    let matching = nori(
        dir.path(),
        &["auth", "set-key", "openai"],
        Some(&format!("sk-proj-{SECRET}\n")),
    );
    let mismatched = nori(
        dir.path(),
        &["auth", "set-key", "openai"],
        Some(&format!("sk-ant-api03-{SECRET}\n")),
    );

    assert!(matching.status.success(), "{matching:?}");
    assert!(
        String::from_utf8_lossy(&matching.stdout).contains("sk-proj-****"),
        "{matching:?}"
    );
    assert!(mismatched.status.success(), "{mismatched:?}");
    let stored = std::fs::read_to_string(dir.path().join("nori/credentials.json")).unwrap();
    assert!(
        stored.contains("sk-ant-api03-"),
        "the user's key is saved as entered"
    );
    assert!(
        String::from_utf8_lossy(&mismatched.stderr).contains("looks like an Anthropic key"),
        "{mismatched:?}"
    );
}

#[test]
fn unreadable_store_is_reported_without_hiding_detection() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("nori")).unwrap();
    std::fs::write(
        dir.path().join("nori/saved-keys.json"),
        r#"{"codex":"file"}"#,
    )
    .unwrap();

    let output = nori_with_env(
        dir.path(),
        &[("OPENAI_API_KEY", SECRET)],
        &["auth", "--json"],
        None,
    );

    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["store_error"]
            .as_str()
            .is_some_and(|error| error.contains("saved-keys.json")),
        "{report}"
    );
    assert_eq!(agent(&report, "codex")["status"], "signed_in");
}
