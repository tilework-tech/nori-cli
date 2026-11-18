#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;
use tempfile::TempDir;

fn bounded_output(command: &mut Command) -> Output {
    // Files avoid waiting for pipe EOF if a broken daemon inherits stdout.
    let mut stdout = tempfile::tempfile().unwrap();
    let mut stderr = tempfile::tempfile().unwrap();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone().unwrap())
        .stderr(stderr.try_clone().unwrap())
        .spawn()
        .unwrap();
    let timeout = Duration::from_secs(35);
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            break child.wait().unwrap();
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    stdout.seek(SeekFrom::Start(0)).unwrap();
    stderr.seek(SeekFrom::Start(0)).unwrap();
    let mut output = Output {
        status,
        stdout: Vec::new(),
        stderr: Vec::new(),
    };
    stdout.read_to_end(&mut output.stdout).unwrap();
    stderr.read_to_end(&mut output.stderr).unwrap();
    if timed_out && !std::thread::panicking() {
        panic!(
            "command {command:?} did not exit within {timeout:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    output
}

struct Host(TempDir);

impl Host {
    fn new() -> Self {
        Self(TempDir::new().unwrap())
    }

    fn run(&self, args: &[&str]) -> Output {
        bounded_output(&mut self.command(args))
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nori"));
        command
            .env("NORI_HOME", self.0.path())
            .args(["-c", "features.multi_session=true"])
            .args(args);
        command
    }

    /// A running host whose mock agent writes `marker.txt` (relative to its
    /// session folder) for every prompt.
    fn writing_files() -> Self {
        let host = Self::new();
        std::fs::write(
            host.0.path().join("config.toml"),
            "agent = \"mock-model\"\n",
        )
        .unwrap();
        success(bounded_output(
            host.command(&["host", "--port", "0"])
                .env("MOCK_AGENT_WRITE_FILE", "marker.txt"),
        ));
        host
    }

    fn client(&self) -> nori_harness::remote_agent::ManagedClient {
        let state: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(self.0.path().join("host/state.json")).unwrap(),
        )
        .unwrap();
        let token = std::fs::read_to_string(self.0.path().join("host/token")).unwrap();
        let runtime = tokio::runtime::Handle::current();
        runtime.block_on(async {
            let mut client = nori_harness::remote_agent::ManagedClient::connect(
                state["endpoint"].as_str().unwrap(),
                token.trim(),
            )
            .await
            .unwrap();
            client
                .request("initialize", serde_json::json!({}))
                .await
                .unwrap();
            client
        })
    }
}

fn wait_for_file(path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} was never written",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn canonical(path: &std::path::Path) -> String {
    path.canonicalize().unwrap().display().to_string()
}

fn failure(output: Output) -> String {
    assert!(
        !output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    String::from_utf8(output.stderr).unwrap()
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.run(&["host", "stop"]);
    }
}

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn detached_host_stays_alive_empty_and_stops_explicitly() {
    let host = Host::new();
    let started = success(host.run(&["host", "--port", "0"]));
    assert!(started.contains("started"), "{started}");
    let listing = success(host.run(&["host", "ls"]));
    assert_eq!(
        listing.split_whitespace().collect::<Vec<_>>(),
        ["ID", "AGENT", "STATUS", "AGE", "CWD", "TITLE"]
    );
    assert!(!listing.contains('\u{1b}'), "piped output must be plain");
    let again = success(host.run(&["host"]));
    assert!(again.contains("already running"), "{again}");
    assert_eq!(listing, success(host.run(&["host", "ls"])));
    success(host.run(&["host", "stop"]));
    assert!(!host.run(&["host", "ls"]).status.success());
    success(host.run(&["host", "--port", "0"]));
    success(host.run(&["host", "stop"]));
}

#[test]
fn host_token_is_private_and_never_printed() {
    let host = Host::new();
    let output = success(host.run(&["host", "--port", "0"]));
    let token_path = host.0.path().join("host/token");
    let token = std::fs::read_to_string(&token_path).unwrap();
    assert!(token.trim().len() >= 43);
    assert!(!output.contains(token.trim()));
    assert_eq!(
        std::fs::metadata(token_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(host.0.path().join("host"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn occupied_port_fails_without_publishing_a_host() {
    let host = Host::new();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    assert!(!host.run(&["host", "--port", &port]).status.success());
    assert!(!host.run(&["host", "ls"]).status.success());
    drop(listener);
    success(host.run(&["host", "--port", &port]));
}

#[test]
fn attach_requires_multi_session_before_opening_terminal() {
    let host = Host::new();
    let result = bounded_output(
        Command::new(env!("CARGO_BIN_EXE_nori"))
            .env("NORI_HOME", host.0.path())
            .args(["attach"]),
    );
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("multi_session"));
}

#[test]
fn token_symlink_is_refused_without_overwriting_target() {
    let host = Host::new();
    let directory = host.0.path().join("host");
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let target = host.0.path().join("unrelated");
    std::fs::write(&target, "leave me alone").unwrap();
    std::os::unix::fs::symlink(&target, directory.join("token")).unwrap();
    assert!(!host.run(&["host", "--port", "0"]).status.success());
    assert_eq!(std::fs::read_to_string(target).unwrap(), "leave me alone");
}

#[test]
fn host_can_be_stopped_even_if_configuration_was_broken_after_start() {
    let host = Host::new();
    success(host.run(&["host", "--port", "0"]));
    std::fs::write(
        host.0.path().join("config.toml"),
        "this is not valid TOML = [",
    )
    .unwrap();
    success(host.run(&["host", "ls"]));
    success(host.run(&["host", "stop"]));
}

#[test]
fn sessions_started_in_two_folders_each_work_in_their_own_folder() {
    let host = Host::writing_files();
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    let first_id = success(host.run(&[
        "host",
        "new",
        "--cwd",
        first.path().to_str().unwrap(),
        "--prompt",
        "write a marker",
    ]));
    let second_id = success(host.run(&[
        "host",
        "new",
        "--cwd",
        second.path().to_str().unwrap(),
        "--prompt",
        "write a marker",
    ]));
    let (first_id, second_id) = (first_id.trim(), second_id.trim());
    assert!(uuid::Uuid::parse_str(first_id).is_ok(), "{first_id:?}");
    assert_ne!(first_id, second_id);
    wait_for_file(&first.path().join("marker.txt"));
    wait_for_file(&second.path().join("marker.txt"));

    let listing = success(host.run(&["host", "ls"]));
    for (id, folder) in [(first_id, &first), (second_id, &second)] {
        let row = listing
            .lines()
            .find(|line| line.starts_with(id))
            .unwrap_or_else(|| panic!("{id} is listed: {listing}"));
        assert!(row.contains(&canonical(folder.path())), "{row}");
    }
}

#[test]
fn new_session_folder_defaults_to_and_resolves_against_the_callers_directory() {
    let host = Host::writing_files();
    let caller = TempDir::new().unwrap();
    std::fs::create_dir(caller.path().join("nested")).unwrap();
    let new_in = |args: &[&str]| -> serde_json::Value {
        let output = success(bounded_output(
            host.command(&[&["host", "new", "--json"], args].concat())
                .current_dir(caller.path()),
        ));
        serde_json::from_str(&output).unwrap()
    };

    let plain = new_in(&[]);
    assert_eq!(plain["cwd"], canonical(caller.path()));
    assert!(plain["session_id"].is_string());
    let relative = new_in(&["--cwd", "nested"]);
    assert_eq!(relative["cwd"], canonical(&caller.path().join("nested")));
}

#[test]
fn new_session_in_a_missing_folder_or_a_file_fails_without_starting_one() {
    let host = Host::writing_files();
    let scratch = TempDir::new().unwrap();
    let missing = scratch.path().join("missing");
    let file = scratch.path().join("file.txt");
    std::fs::write(&file, "").unwrap();

    let error = failure(host.run(&["host", "new", "--cwd", missing.to_str().unwrap()]));
    assert!(
        error.contains("does not exist") && error.contains("missing"),
        "{error}"
    );
    let error = failure(host.run(&["host", "new", "--cwd", file.to_str().unwrap()]));
    assert!(
        error.contains("not a directory") && error.contains("file.txt"),
        "{error}"
    );
    assert_eq!(success(host.run(&["host", "ls"])).lines().count(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_host_refuses_a_malformed_new_session_without_starting_one() {
    let host = Host::writing_files();
    let mut client = tokio::task::block_in_place(|| host.client());
    for (params, expected) in [
        (serde_json::json!({"cwd": "relative/path"}), "absolute"),
        (
            serde_json::json!({"prompt": [{"type": "text", "text": "hi"}]}),
            "prompt must be a string",
        ),
    ] {
        let error = client
            .request("_nori/session/new", params.clone())
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains(expected), "{params}: {error}");
    }
    let listing = client
        .request("_nori/session/list", serde_json::json!({}))
        .await
        .unwrap();
    assert_eq!(listing["sessions"], serde_json::json!([]));
}

#[test]
fn a_named_host_starts_in_its_own_folder_unless_one_is_given() {
    let host = Host::writing_files();
    let state: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(host.0.path().join("host/state.json")).unwrap(),
    )
    .unwrap();
    let token = host.0.path().join("host/token");
    let caller = TempDir::new().unwrap();
    std::fs::create_dir(caller.path().join("nested")).unwrap();
    let new_on_host = |args: &[&str]| -> serde_json::Value {
        let remote = [
            "host",
            "new",
            "--json",
            "--host",
            state["endpoint"].as_str().unwrap(),
            "--token-file",
            token.to_str().unwrap(),
        ];
        let output = success(bounded_output(
            host.command(&[&remote[..], args].concat())
                .current_dir(caller.path()),
        ));
        serde_json::from_str(&output).unwrap()
    };

    let host_folder = canonical(&std::env::current_dir().unwrap());
    assert_eq!(new_on_host(&[])["cwd"], host_folder);
    assert_eq!(
        new_on_host(&["--cwd", "nested"])["cwd"],
        canonical(&caller.path().join("nested"))
    );
}

/// Send `text` to session `id`, waiting while it is still busy with the
/// previous prompt.
async fn send_prompt(client: &mut nori_harness::remote_agent::ManagedClient, id: &str, text: &str) {
    let prompt = serde_json::json!({"session_id": id, "prompt": [{"type": "text", "text": text}]});
    let deadline = Instant::now() + Duration::from_secs(30);
    while let Err(error) = client.request("_nori/session/prompt", prompt.clone()).await {
        assert!(
            error.to_string().contains("busy") && Instant::now() < deadline,
            "{text}: {error}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_session_keeps_its_folder() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let marker = folder.path().join("marker.txt");
    let id = success(host.run(&[
        "host",
        "new",
        "--cwd",
        folder.path().to_str().unwrap(),
        "--prompt",
        "write a marker",
    ]));
    let id = id.trim();
    wait_for_file(&marker);
    std::fs::remove_file(&marker).unwrap();

    let mut client = tokio::task::block_in_place(|| host.client());
    send_prompt(&mut client, id, "mock:exit-during-prompt").await;
    let status = |listing: &serde_json::Value| {
        listing["sessions"][0]["status"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let listing = client
            .request("_nori/session/list", serde_json::json!({}))
            .await
            .unwrap();
        if status(&listing) == "failed" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the agent crash fails the session: {listing}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    client
        .request(
            "_nori/session/resume",
            serde_json::json!({"session_id": id}),
        )
        .await
        .unwrap();
    send_prompt(&mut client, id, "write again").await;
    tokio::task::block_in_place(|| wait_for_file(&marker));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_delegated_child_works_in_its_parents_folder() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let marker = folder.path().join("marker.txt");
    let id = success(host.run(&[
        "host",
        "new",
        "--cwd",
        folder.path().to_str().unwrap(),
        "--prompt",
        "write a marker",
    ]));
    let id = id.trim();
    wait_for_file(&marker);
    std::fs::remove_file(&marker).unwrap();

    let mut client = tokio::task::block_in_place(|| host.client());
    client
        .request(
            "_nori/session/delegate",
            serde_json::json!({"session_id": id, "agent": "mock-model", "task": "help"}),
        )
        .await
        .unwrap();
    let listing = client
        .request("_nori/session/list", serde_json::json!({}))
        .await
        .unwrap();
    let child = listing["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["parent"] == id)
        .unwrap_or_else(|| panic!("delegated child is listed: {listing}"));
    assert_eq!(child["cwd"], canonical(folder.path()));
    tokio::task::block_in_place(|| wait_for_file(&marker));
}

#[path = "host_lifecycle/managed.rs"]
mod managed;
