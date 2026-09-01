#![cfg(unix)]

//! Prepared-by-default ACP lifecycle tests.
//!
//! The fake `nori-handroll` executable wraps the normal mock ACP agent while
//! recording process and argv boundaries. Registering it as an ordinary local
//! ACP distribution exercises the same subprocess path used by
//! `nori-handroll acp --type remote` without requiring a live remote endpoint.

use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

fn mock_agent_path() -> PathBuf {
    let test_exe = std::env::current_exe().expect("current exe");
    test_exe
        .parent()
        .and_then(|path| path.parent())
        .expect("target directory")
        .join("mock_acp_agent")
}

struct FakeRemoteHandroll {
    dir: tempfile::TempDir,
    script: PathBuf,
}

impl FakeRemoteHandroll {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create fixture directory");
        let mock = mock_agent_path();
        assert!(mock.exists(), "missing mock agent at {}", mock.display());
        let script = dir.path().join("nori-handroll");
        let body = format!(
            "#!/bin/sh\n\
             printf '%s ' \"$@\" >> '{dir}/argv'\n\
             printf '\\n' >> '{dir}/argv'\n\
             echo $$ >> '{dir}/pids'\n\
             '{mock}' 2>>'{dir}/agent_stderr'\n",
            dir = dir.path().display(),
            mock = mock.display(),
        );
        std::fs::write(&script, body).expect("write fake handroll");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("make fake handroll executable");
        }
        Self { dir, script }
    }

    fn config(&self) -> SessionConfig {
        let config_toml = format!(
            r#"
agent = "remote-handroll"

[[agents]]
name = "Remote Handroll"
slug = "remote-handroll"

[agents.distribution.local]
command = "{}"
args = ["acp", "--type", "remote", "ws://microvm.test/acp"]

[notice]
hide_full_access_warning = true
"#,
            self.script.display(),
        );
        SessionConfig::new()
            .with_agent("remote-handroll".to_string())
            .with_config_toml(config_toml)
    }

    fn read(&self, marker: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(marker)).unwrap_or_default()
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn wait_for_stderr(&self, needle: &str, timeout: Duration) -> String {
        let deadline = Instant::now() + timeout;
        loop {
            let stderr = self.read("agent_stderr");
            if stderr.contains(needle) || Instant::now() >= deadline {
                return stderr;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn pid_count(&self) -> usize {
        self.read("pids").lines().count()
    }

    fn wait_for_pid_count(&self, count: usize, timeout: Duration) -> usize {
        let deadline = Instant::now() + timeout;
        loop {
            let actual = self.pid_count();
            if actual >= count || Instant::now() >= deadline {
                return actual;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn first_pid(&self) -> u32 {
        self.read("pids")
            .lines()
            .next()
            .expect("recorded adapter pid")
            .parse()
            .expect("numeric adapter pid")
    }

    fn wait_for_recorded_pid(&self, marker: &str, timeout: Duration) -> u32 {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(contents) = std::fs::read_to_string(self.path(marker))
                && let Ok(pid) = contents.trim().parse()
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {marker}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn child_is_alive(pid: u32) -> bool {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .is_ok_and(|status| status.success())
    }
}

fn assert_lifecycle_order(stderr: &str) {
    let initialize = stderr.find("Mock agent: initialize").expect("initialize");
    let list = stderr
        .find("Mock agent: session/list")
        .expect("session/list");
    let new = stderr
        .find("Mock agent: new_session id=")
        .expect("session/new");
    let prompt = stderr.find("Mock agent: prompt").expect("prompt");
    assert!(
        initialize < list && list < new && new < prompt,
        "expected initialize → list → new → prompt, got:\n{stderr}"
    );
}

#[test]
fn remote_adapter_prepares_without_activating_a_session() {
    let fake = FakeRemoteHandroll::new();
    let config = fake
        .config()
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env("MOCK_AGENT_FAIL_NEW_SESSION_FROM", "0");
    let mut session = TuiSession::spawn_with_config(24, 90, config).expect("spawn nori");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("composer should remain usable while the agent is prepared");
    let _ = fake.wait_for_stderr("Mock agent: session/list", TIMEOUT);
    std::thread::sleep(Duration::from_millis(300));
    let stderr = fake.read("agent_stderr");

    assert_eq!(fake.pid_count(), 1, "startup must spawn one adapter child");
    assert_eq!(
        fake.read("argv").trim(),
        "acp --type remote ws://microvm.test/acp",
        "the registered remote adapter invocation must be preserved"
    );
    assert!(
        !stderr.contains("new_session"),
        "preparation must not activate a session:\n{stderr}"
    );
}

#[test]
fn agent_without_session_list_prepares_without_activating_a_session() {
    let fake = FakeRemoteHandroll::new();
    let config = fake
        .config()
        .with_agent_env("MOCK_AGENT_FAIL_NEW_SESSION_FROM", "0");
    let mut session = TuiSession::spawn_with_config(24, 90, config).expect("spawn nori");

    session.wait_for_text("›", TIMEOUT).expect("composer");
    let stderr = fake.wait_for_stderr("Mock agent: initialize", TIMEOUT);

    assert_eq!(fake.pid_count(), 1);
    assert!(!stderr.contains("session/list"));
    assert!(!stderr.contains("new_session"));
}

#[test]
fn first_prompt_waits_for_preparation_then_activates_once() {
    let fake = FakeRemoteHandroll::new();
    let config = fake
        .config()
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env("MOCK_AGENT_STARTUP_DELAY_MS", "1200")
        .with_agent_env(
            "MOCK_AGENT_EXPECT_LAST_PROMPT_TEXT_BLOCK",
            "one deferred prompt",
        )
        .with_mock_response("remote deferred turn complete");
    let mut session = TuiSession::spawn_with_config(24, 90, config).expect("spawn nori");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("the composer should remain usable during preparation");
    assert_eq!(fake.wait_for_pid_count(1, TIMEOUT), 1);
    session.send_str("one deferred prompt").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("remote deferred turn complete", TIMEOUT)
        .expect("the original prompt should run after session activation");

    std::thread::sleep(Duration::from_millis(300));
    let stderr = fake.read("agent_stderr");
    assert_eq!(
        fake.pid_count(),
        1,
        "prompt activation must reuse the child"
    );
    assert_eq!(
        stderr.matches("Mock agent: initialize").count(),
        1,
        "the child must initialize once"
    );
    assert_eq!(
        stderr.matches("Mock agent: prompt").count(),
        1,
        "the deferred prompt must be submitted exactly once"
    );
    assert_lifecycle_order(&stderr);
}

#[test]
fn slash_new_reuses_in_flight_preparation() {
    let fake = FakeRemoteHandroll::new();
    let config = fake
        .config()
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env("MOCK_AGENT_STARTUP_DELAY_MS", "1200");
    let mut session = TuiSession::spawn_with_config(24, 90, config).expect("spawn nori");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("the composer should remain usable during preparation");
    assert_eq!(fake.wait_for_pid_count(1, TIMEOUT), 1);
    session.send_str("/new").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    let _ = fake.wait_for_stderr("Mock agent: new_session id=", TIMEOUT);
    std::thread::sleep(Duration::from_millis(300));
    let stderr = fake.read("agent_stderr");
    assert_eq!(
        fake.pid_count(),
        1,
        "/new must not replace the prepared child"
    );
    assert_eq!(stderr.matches("Mock agent: initialize").count(), 1);
    assert_eq!(stderr.matches("Mock agent: new_session id=").count(), 1);
}

#[test]
fn slash_resume_reuses_in_flight_preparation_catalog() {
    let fake = FakeRemoteHandroll::new();
    let config = fake
        .config()
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1")
        .with_agent_env("MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT", "1")
        .with_agent_env("MOCK_AGENT_STARTUP_DELAY_MS", "1200");
    let mut session = TuiSession::spawn_with_config(24, 100, config).expect("spawn nori");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("the composer should remain usable during preparation");
    assert_eq!(fake.wait_for_pid_count(1, TIMEOUT), 1);
    session.send_str("/resume").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("First mock session", TIMEOUT)
        .expect("/resume should open the catalog prepared on startup");
    session.send_key(Key::Down).unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("replay chunk 0", TIMEOUT)
        .expect("the selected catalog row should load on the prepared connection");

    std::thread::sleep(Duration::from_millis(300));
    let stderr = fake.read("agent_stderr");
    assert_eq!(fake.pid_count(), 1, "/resume must reuse the prepared child");
    assert_eq!(stderr.matches("Mock agent: initialize").count(), 1);
    assert_eq!(stderr.matches("Mock agent: session/list").count(), 1);
    assert!(
        !stderr.contains("new_session"),
        "/resume must not implicitly activate a new session:\n{stderr}"
    );
}

#[test]
fn slash_resume_uses_resume_capability_on_the_prepared_connection() {
    let fake = FakeRemoteHandroll::new();
    let config = fake
        .config()
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_RESUME", "1")
        .with_agent_env("MOCK_AGENT_STARTUP_DELAY_MS", "1200");
    let mut session = TuiSession::spawn_with_config(24, 100, config).expect("spawn nori");

    session.wait_for_text("›", TIMEOUT).expect("composer");
    assert_eq!(fake.wait_for_pid_count(1, TIMEOUT), 1);
    session.send_str("/resume").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("First mock session", TIMEOUT)
        .expect("catalog");
    session.send_key(Key::Down).unwrap();
    session.send_key(Key::Enter).unwrap();

    let stderr = fake.wait_for_stderr("Mock agent: resume_session id=", TIMEOUT);
    assert_eq!(fake.pid_count(), 1);
    assert_eq!(stderr.matches("Mock agent: initialize").count(), 1);
    assert_eq!(stderr.matches("Mock agent: session/list").count(), 1);
    assert_eq!(stderr.matches("Mock agent: resume_session id=").count(), 1);
    assert!(!stderr.contains("new_session"));
}

#[test]
fn positional_prompt_activates_after_preparation_exactly_once() {
    let fake = FakeRemoteHandroll::new();
    let image = fake.path("attachment.png");
    std::fs::write(&image, b"mock png bytes").expect("write image fixture");
    let config = fake
        .config()
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env(
            "MOCK_AGENT_EXPECT_LAST_PROMPT_TEXT_BLOCK",
            "one positional prompt",
        )
        .with_agent_env("MOCK_AGENT_EXPECT_MATCHING_TEXT_BLOCK_COUNT", "1")
        .with_agent_env("MOCK_AGENT_EXPECT_IMAGE_BLOCK_COUNT", "1")
        .with_agent_env("MOCK_AGENT_EXPECT_IMAGE_MIME_TYPE", "image/png")
        .with_agent_env("MOCK_AGENT_EXPECT_IMAGE_DATA", "bW9jayBwbmcgYnl0ZXM=")
        .with_mock_response("remote positional turn complete")
        .with_arg("one positional prompt")
        .with_arg("-i")
        .with_arg(image.to_string_lossy());
    let mut session = TuiSession::spawn_with_config(24, 90, config).expect("spawn nori");

    session
        .wait_for_text("remote positional turn complete", TIMEOUT)
        .expect("the positional prompt should run after activation");

    std::thread::sleep(Duration::from_millis(300));
    let stderr = fake.read("agent_stderr");
    assert_eq!(fake.pid_count(), 1);
    assert_eq!(stderr.matches("Mock agent: initialize").count(), 1);
    assert_eq!(stderr.matches("Mock agent: prompt").count(), 1);
    assert_lifecycle_order(&stderr);
}

#[test]
fn local_and_slash_commands_do_not_implicitly_activate() {
    let fake = FakeRemoteHandroll::new();
    let config = fake
        .config()
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env("MOCK_AGENT_FAIL_NEW_SESSION_FROM", "0");
    let mut session = TuiSession::spawn_with_config(24, 90, config).expect("spawn nori");

    session.wait_for_text("›", TIMEOUT).expect("composer");
    assert_eq!(fake.wait_for_pid_count(1, TIMEOUT), 1);
    let _ = fake.wait_for_stderr("Mock agent: session/list", TIMEOUT);
    session.send_str("/status").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("/status", TIMEOUT)
        .expect("the local status command should render before activation");
    session.send_str("!pwd").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("No active harness session", TIMEOUT)
        .expect("local shell command should report why it cannot run yet");
    std::thread::sleep(Duration::from_millis(300));

    let stderr = fake.read("agent_stderr");
    assert_eq!(fake.pid_count(), 1);
    assert!(
        !stderr.contains("new_session"),
        "local and slash commands must not activate a session:\n{stderr}"
    );
}

#[test]
fn sessionless_policy_change_is_refreshed_before_activation() {
    let fake = FakeRemoteHandroll::new();
    let config = fake
        .config()
        .with_agent_env("MOCK_AGENT_REQUEST_PERMISSION", "1");
    let mut session = TuiSession::spawn_with_config(24, 100, config).expect("spawn nori");

    session.wait_for_text("›", TIMEOUT).expect("composer");
    let _ = fake.wait_for_stderr("Mock agent: initialize", TIMEOUT);
    session.send_str("/approvals").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Select approval mode", TIMEOUT)
        .expect("approval picker");
    std::thread::sleep(tui_pty_e2e::TIMEOUT_PRESNAPSHOT);
    insta::assert_snapshot!(
        "sessionless_approval_picker",
        tui_pty_e2e::normalize_for_input_snapshot(session.screen_contents())
    );
    session.send_key(Key::Down).unwrap();
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Approvals: Full Access", TIMEOUT)
        .expect("full-access policy should be applied while sessionless");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("exercise refreshed policy").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Permission granted with option: allow", TIMEOUT)
        .expect("the refreshed never-ask policy should auto-approve");

    let stderr = fake.read("agent_stderr");
    assert_eq!(fake.pid_count(), 1);
    assert_eq!(stderr.matches("Mock agent: initialize").count(), 1);
    assert_eq!(stderr.matches("Mock agent: new_session id=").count(), 1);
}

#[test]
fn exit_reaps_in_flight_preparation() {
    let fake = FakeRemoteHandroll::new();
    let descendant_marker = fake.path("descendant_pid");
    let config = fake
        .config()
        .with_agent_env("MOCK_AGENT_STARTUP_DELAY_MS", "60000")
        .with_agent_env(
            "MOCK_AGENT_DESCENDANT_PID_FILE",
            descendant_marker.to_string_lossy(),
        );
    let mut session = TuiSession::spawn_with_config(24, 90, config).expect("spawn nori");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("the composer should remain usable during preparation");
    assert_eq!(fake.wait_for_pid_count(1, TIMEOUT), 1);
    let pid = fake.first_pid();
    let descendant_pid = fake.wait_for_recorded_pid("descendant_pid", TIMEOUT);
    session.send_str("/quit").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    assert!(
        session.wait_for_process_exit(Duration::from_secs(5)),
        "the TUI should exit while preparation is pending"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while FakeRemoteHandroll::child_is_alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !FakeRemoteHandroll::child_is_alive(pid),
        "exit must reap the in-flight adapter child {pid}"
    );
    assert!(
        !FakeRemoteHandroll::child_is_alive(descendant_pid),
        "exit must reap the adapter process tree"
    );
}

#[test]
fn ordinary_agent_prepares_before_running_a_positional_prompt() {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_string())
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env(
            "MOCK_AGENT_EXPECT_LAST_PROMPT_TEXT_BLOCK",
            "ordinary positional prompt",
        )
        .with_mock_response("ordinary prepared turn complete")
        .with_arg("ordinary positional prompt");
    let mut session = TuiSession::spawn_with_config(24, 90, config).expect("spawn nori");

    session
        .wait_for_text("ordinary prepared turn complete", TIMEOUT)
        .expect("ordinary agents should run positional input after preparation");
    let contents = session.screen_contents();
    assert!(
        !contents.contains("expected final user prompt text block"),
        "the positional prompt must arrive intact:\n{contents}"
    );
}

#[test]
fn primary_preparation_timeout_reaps_the_child_without_activating() {
    let fake = FakeRemoteHandroll::new();
    let descendant_marker = fake.path("descendant_pid");
    let config = fake
        .config()
        .with_agent_env("MOCK_AGENT_STARTUP_DELAY_MS", "60000")
        .with_agent_env(
            "MOCK_AGENT_DESCENDANT_PID_FILE",
            descendant_marker.to_string_lossy(),
        );
    let mut session = TuiSession::spawn_with_config(24, 90, config).expect("spawn nori");

    session.wait_for_text("›", TIMEOUT).expect("composer");
    assert_eq!(fake.wait_for_pid_count(1, TIMEOUT), 1);
    let pid = fake.first_pid();
    let descendant_pid = fake.wait_for_recorded_pid("descendant_pid", TIMEOUT);
    session
        .wait_for_text(
            "timed out preparing agent after 20s",
            Duration::from_secs(25),
        )
        .expect("primary preparation should time out visibly");

    let deadline = Instant::now() + Duration::from_secs(5);
    while FakeRemoteHandroll::child_is_alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!FakeRemoteHandroll::child_is_alive(pid));
    assert!(!FakeRemoteHandroll::child_is_alive(descendant_pid));
    assert!(!fake.read("agent_stderr").contains("new_session"));
}
