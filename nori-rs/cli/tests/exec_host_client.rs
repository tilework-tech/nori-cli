//! One-shot `nori exec "prompt"` as a client of `nori host`: it runs its
//! prompt on the home's running host when that host can take it, and on a
//! host of its own otherwise, with the same output either way.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::process::Output;
use std::process::Stdio;

use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;

fn mock_agent_binary() -> String {
    std::env::var("MOCK_ACP_AGENT_BIN").unwrap_or_else(|_| {
        Path::new(env!("CARGO_BIN_EXE_nori"))
            .with_file_name("mock_acp_agent")
            .display()
            .to_string()
    })
}

fn nori(home: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_nori"));
    command
        .env("NORI_HOME", home)
        .env("MOCK_ACP_AGENT_BIN", mock_agent_binary())
        .env_remove("MOCK_AGENT_WRITE_FILE")
        .stdin(Stdio::null());
    command
}

/// A home whose running host gives its agents `MOCK_AGENT_WRITE_FILE`, so a
/// prompt that runs there leaves `marker.txt` in its folder and one that
/// runs anywhere else does not.
struct RunningHost(TempDir);

impl RunningHost {
    fn start() -> Self {
        Self::start_with(&[])
    }

    fn start_with(env: &[(&str, &str)]) -> Self {
        let home = TempDir::new().unwrap();
        std::fs::write(home.path().join("config.toml"), "agent = \"mock-model\"\n").unwrap();
        let started = nori(home.path())
            .args(["host", "--port", "0"])
            .env("MOCK_AGENT_WRITE_FILE", "marker.txt")
            .envs(env.iter().copied())
            .output()
            .unwrap();
        assert!(started.status.success(), "{started:?}");
        Self(home)
    }

    fn exec(&self, folder: &Path, args: &[&str]) -> Output {
        nori(self.0.path())
            .current_dir(folder)
            .arg("exec")
            .args(args)
            .output()
            .unwrap()
    }

    /// A client that takes control of the host and keeps it while alive.
    fn controller(&self) -> nori_harness::remote_agent::ManagedClient {
        let discovery: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(self.0.path().join("host/host.json")).unwrap(),
        )
        .unwrap();
        tokio::runtime::Handle::current().block_on(async {
            let mut client = nori_harness::remote_agent::ManagedClient::connect(
                discovery["endpoint"].as_str().unwrap(),
                discovery["token"].as_str().unwrap(),
            )
            .await
            .unwrap();
            let initialized = client
                .request("initialize", json!({"protocolVersion": 1}))
                .await
                .unwrap();
            assert_eq!(initialized["control"]["role"], "controller");
            client
        })
    }
}

impl Drop for RunningHost {
    fn drop(&mut self) {
        let _ = nori(self.0.path()).args(["host", "stop"]).output();
    }
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "exec failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn exec_runs_its_prompt_on_the_homes_running_host() {
    let host = RunningHost::start();
    let folder = TempDir::new().unwrap();

    let output = host.exec(folder.path(), &["hello"]);

    assert!(
        stdout(&output).starts_with("Test message 1Test message 2"),
        "{output:?}"
    );
    assert!(
        folder.path().join("marker.txt").exists(),
        "the running host's agent ran the prompt"
    );
    let stopped = nori(host.0.path()).args(["host", "stop"]).output().unwrap();
    assert!(
        String::from_utf8_lossy(&stopped.stdout).contains("Host stopped"),
        "the running host outlives the exec: {stopped:?}"
    );
}

impl RunningHost {
    /// Rewrite `host.json` so the host advertises `contract`, as an older or
    /// newer `nori` would.
    fn advertise_contract(&self, contract: &str) {
        let path = self.0.path().join("host/host.json");
        let mut discovery: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        discovery["contract"] = json!(contract);
        std::fs::write(&path, discovery.to_string()).unwrap();
    }
}

#[test]
fn exec_runs_on_its_own_host_when_the_running_one_speaks_another_contract() {
    let host = RunningHost::start();
    host.advertise_contract("2.0.0");
    let folder = TempDir::new().unwrap();

    let output = host.exec(folder.path(), &["hello"]);

    assert!(
        stdout(&output).starts_with("Test message 1Test message 2"),
        "{output:?}"
    );
    assert!(
        !folder.path().join("marker.txt").exists(),
        "the incompatible host was left alone"
    );
    let stopped = nori(host.0.path()).args(["host", "stop"]).output().unwrap();
    assert!(
        String::from_utf8_lossy(&stopped.stdout).contains("Host stopped"),
        "{stopped:?}"
    );
}

#[test]
fn exec_runs_on_a_running_host_of_a_newer_compatible_contract() {
    let host = RunningHost::start();
    host.advertise_contract("1.7.0");
    let folder = TempDir::new().unwrap();

    let output = host.exec(folder.path(), &["hello"]);

    assert!(
        stdout(&output).starts_with("Test message 1Test message 2"),
        "{output:?}"
    );
    assert!(
        folder.path().join("marker.txt").exists(),
        "the running host's agent ran the prompt"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn exec_runs_on_its_own_host_while_another_client_controls_the_running_one() {
    let host = RunningHost::start();
    let folder = TempDir::new().unwrap();
    let _controller = tokio::task::block_in_place(|| host.controller());

    let output = tokio::task::block_in_place(|| host.exec(folder.path(), &["hello"]));

    assert_eq!(stdout(&output), "Test message 1Test message 2\n");
    assert!(
        !folder.path().join("marker.txt").exists(),
        "the controlled host was left alone"
    );
}

#[test]
fn exec_with_configuration_overrides_runs_on_its_own_host() {
    let host = RunningHost::start();
    let folder = TempDir::new().unwrap();

    let output = host.exec(
        folder.path(),
        &["-c", "approval_policy=\"on-request\"", "hello"],
    );

    assert_eq!(stdout(&output), "Test message 1Test message 2\n");
    assert!(
        !folder.path().join("marker.txt").exists(),
        "the overrides could not reach the running host"
    );
}

#[test]
fn exec_on_the_running_host_rejects_permission_requests_and_fails() {
    let host = RunningHost::start();
    let folder = TempDir::new().unwrap();

    let output = host.exec(folder.path(), &["mock:request-permission"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("option: reject"),
        "{output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("permission request was rejected"),
        "{output:?}"
    );
    assert!(folder.path().join("marker.txt").exists());
}

#[test]
fn stopping_exec_ends_its_turn_on_the_running_host() {
    let host = RunningHost::start_with(&[("MOCK_AGENT_STREAM_UNTIL_CANCEL", "1")]);
    let folder = TempDir::new().unwrap();
    let mut exec = nori(host.0.path())
        .current_dir(folder.path())
        .args(["exec", "mock:stream-until-cancel"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // The marker appears once the host's agent is working on the turn.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !folder.path().join("marker.txt").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "the turn never started"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let killed = std::process::Command::new("kill")
        .args(["-TERM", &exec.id().to_string()])
        .status()
        .unwrap();
    let status = exec.wait().unwrap();
    let listed = nori(host.0.path()).args(["host", "ls"]).output().unwrap();

    assert!(killed.success());
    assert!(!status.success(), "{status}");
    let listing = String::from_utf8_lossy(&listed.stdout);
    let row = listing
        .lines()
        .find(|line| line.contains(&canonical(folder.path())))
        .unwrap_or_else(|| panic!("the exec session is listed: {listing}"));
    assert!(row.contains("closed"), "the turn ended with exec: {row}");
}

fn canonical(path: &Path) -> String {
    path.canonicalize().unwrap().display().to_string()
}
