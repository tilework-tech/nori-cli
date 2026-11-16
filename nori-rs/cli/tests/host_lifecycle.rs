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
        bounded_output(
            Command::new(env!("CARGO_BIN_EXE_nori"))
                .env("NORI_HOME", self.0.path())
                .args(["-c", "features.multi_session=true"])
                .args(args),
        )
    }
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
        ["ID", "AGENT", "STATUS", "AGE", "TITLE"]
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
