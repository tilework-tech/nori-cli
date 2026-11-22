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
    let output = nori(home, &["providers", "--json"], None);
    assert!(output.status.success(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn providers_json_reports_a_codex_chatgpt_login() {
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
fn providers_table_names_each_agent_and_its_status() {
    let dir = tempfile::tempdir().unwrap();
    let output = nori(dir.path(), &["providers"], None);
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

    let saved = nori(
        dir.path(),
        &["providers", "set-key", "openai", "--json"],
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
        "disconnect": "run `nori providers remove-key openai` to disconnect"
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

    let removed = nori(dir.path(), &["providers", "remove-key", "openai"], None);
    assert!(removed.status.success(), "{removed:?}");
    assert_eq!(
        agent(&report(dir.path()), "codex")["credentials"],
        json!([])
    );
}

#[test]
fn env_credentials_are_read_only_in_the_table() {
    let dir = tempfile::tempdir().unwrap();
    let output = nori_with_env(dir.path(), &[("XAI_API_KEY", SECRET)], &["providers"], None);
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
        &["providers", "set-key", "codex"],
        Some(&format!("{SECRET}\n")),
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("openai"), "{output:?}");

    let output = nori(dir.path(), &["providers", "set-key", "nope"], Some(SECRET));
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not a provider"),
        "{output:?}"
    );
    assert!(!dir.path().join("nori/credentials.json").exists());
}

#[test]
fn saved_key_reaches_the_launched_agent() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let runner = home.join("npx");
    std::fs::write(
        &runner,
        format!(
            "#!/bin/sh\nif [ \"$OPENAI_API_KEY\" = \"{SECRET}\" ]; then echo saved; else echo launched-without-key; fi > \"$HOME/probe\"\nexit 1\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        nori(home, &["providers", "set-key", "openai"], Some(SECRET))
            .status
            .success()
    );

    nori(home, &["--agent", "codex", "exec", "hello"], None);

    assert_eq!(
        std::fs::read_to_string(home.join("probe")).ok(),
        Some("saved\n".to_string())
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
        &["providers", "--json"],
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
