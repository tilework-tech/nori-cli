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
    let mut child = Command::new(env!("CARGO_BIN_EXE_nori"))
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", home.display()))
        .env("HOME", home)
        .env("NORI_HOME", home.join("nori"))
        .env("NORI_CREDENTIALS_STORE", "file")
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

// macOS also probes the real login keychain, so exact Claude results are Linux-only.
#[cfg(target_os = "linux")]
#[test]
fn providers_json_reports_a_claude_subscription_file() {
    let dir = tempfile::tempdir().unwrap();
    let creds = dir.path().join(".claude/.credentials.json");
    std::fs::create_dir_all(creds.parent().unwrap()).unwrap();
    std::fs::write(
        &creds,
        format!(r#"{{"claudeAiOauth":{{"accessToken":"{SECRET}","subscriptionType":"pro"}}}}"#),
    )
    .unwrap();

    let claude = agent(&report(dir.path()), "claude-code");

    assert_eq!(
        (&claude["status"], &claude["credentials"]),
        (
            &json!("signed_in"),
            &json!([{"kind": "subscription", "source": {"type": "file", "path": creds}, "detail": "pro"}])
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
fn saved_key_is_reported_and_can_be_removed() {
    let dir = tempfile::tempdir().unwrap();

    let saved = nori(
        dir.path(),
        &["providers", "set-key", "codex"],
        Some(&format!("{SECRET}\n")),
    );
    assert!(saved.status.success(), "{saved:?}");
    assert_eq!(
        agent(&report(dir.path()), "codex")["credentials"],
        json!([{"kind": "api_key", "source": {"type": "nori_saved", "env": "OPENAI_API_KEY", "store": "file"}}])
    );

    let removed = nori(dir.path(), &["providers", "remove-key", "codex"], None);
    assert!(removed.status.success(), "{removed:?}");
    assert_eq!(
        agent(&report(dir.path()), "codex")["credentials"],
        json!([])
    );
}

#[test]
fn set_key_refuses_agents_without_an_api_key_variable() {
    let dir = tempfile::tempdir().unwrap();
    let output = nori(
        dir.path(),
        &["providers", "set-key", "opencode"],
        Some(&format!("{SECRET}\n")),
    );
    assert!(!output.status.success());
    assert_eq!(
        agent(&report(dir.path()), "opencode")["credentials"],
        json!([])
    );
}

#[test]
fn saved_key_reaches_the_launched_agent() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let runner = home.join("npx");
    std::fs::write(
        &runner,
        format!(
            "#!/bin/sh\n[ \"$OPENAI_API_KEY\" = \"{SECRET}\" ] && echo saved > \"$HOME/probe\"\nexit 1\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        nori(home, &["providers", "set-key", "codex"], Some(SECRET))
            .status
            .success()
    );

    nori(home, &["--agent", "codex", "exec", "hello"], None);

    assert_eq!(
        std::fs::read_to_string(home.join("probe")).ok(),
        Some("saved\n".to_string())
    );
}
