//! E2E tests for `nori cloud` delegating to `nori-handroll cloud-acp`.
//!
//! `nori cloud` spawns the `nori-handroll` binary (resolved from
//! `NORI_HANDROLL_BIN` or `$PATH`) as an ordinary stdio ACP child and runs
//! the normal TUI against it. These tests drive that path with a fake
//! handroll script that wraps the `mock_acp_agent` binary, recording its
//! invocation and lifecycle to marker files.

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

/// Path to the `mock_acp_agent` binary next to the test binary.
fn mock_agent_path() -> PathBuf {
    let test_exe = std::env::current_exe().expect("current exe");
    test_exe
        .parent() // deps
        .and_then(|p| p.parent()) // debug or release
        .expect("target directory")
        .join("mock_acp_agent")
}

/// A fake `nori-handroll` binary backed by `mock_acp_agent`.
///
/// The script records its argv to `argv`, the `NORI_BROKER_URL` it received
/// to `broker_url`, then runs the mock agent on its stdio. After the mock
/// agent exits (it exits 0 on stdin EOF), the script writes a `released`
/// marker — the same "stdin EOF means clean release" contract that
/// `nori-handroll cloud-acp` implements. A SIGKILLed process group never
/// writes the marker.
struct FakeHandroll {
    dir: tempfile::TempDir,
    script: PathBuf,
}

impl FakeHandroll {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("create fixture dir");
        let mock = mock_agent_path();
        assert!(
            mock.exists(),
            "mock_acp_agent binary not found at {} — build it with `cargo build -p mock-acp-agent`",
            mock.display()
        );
        let script = dir.path().join("nori-handroll");
        let body = format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$@\" > '{dir}/argv'\n\
             printenv NORI_BROKER_URL > '{dir}/broker_url' 2>/dev/null\n\
             '{mock}'\n\
             status=$?\n\
             echo done > '{dir}/released'\n\
             exit $status\n",
            dir = dir.path().display(),
            mock = mock.display(),
        );
        std::fs::write(&script, body).expect("write fake handroll script");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake handroll");
        }
        Self { dir, script }
    }

    /// A fake handroll that runs normally until the `die` marker file
    /// appears, then prints to stderr and SIGKILLs itself — simulating a
    /// mid-session child death (e.g. a collapsed tunnel).
    fn crash_on_trigger() -> Self {
        let fake = Self::new();
        let mock = mock_agent_path();
        let body = format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$@\" > '{dir}/argv'\n\
             ( while [ ! -f '{dir}/die' ]; do sleep 0.1; done; \
               echo 'tunnel collapsed' >&2; kill -9 $$ ) &\n\
             exec '{mock}'\n",
            dir = fake.dir.path().display(),
            mock = mock.display(),
        );
        std::fs::write(&fake.script, body).expect("write crash-on-trigger script");
        fake
    }

    /// A fake handroll that prints an auth error to stderr and exits 1
    /// immediately, like an unauthenticated `nori-handroll cloud-acp`.
    fn unauthenticated() -> Self {
        let fake = Self::new();
        let body = "#!/bin/sh\n\
             echo 'Error: not authenticated \u{2014} run: nori-handroll login' >&2\n\
             exit 1\n";
        std::fs::write(&fake.script, body).expect("write unauthenticated script");
        fake
    }

    fn marker(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn wait_for_marker(&self, name: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.marker(name).exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }
}

fn cloud_session_config(fake: &FakeHandroll) -> SessionConfig {
    SessionConfig::new()
        .with_subcommand("cloud")
        .with_agent_env("NORI_HANDROLL_BIN", fake.script.to_string_lossy())
}

/// Find transcript files under NORI_HOME (same layout as transcript_persistence tests).
fn transcripts_exist(nori_home: &Path) -> bool {
    let by_project = nori_home.join("transcripts").join("by-project");
    let Ok(projects) = std::fs::read_dir(&by_project) else {
        return false;
    };
    projects
        .flatten()
        .filter_map(|p| std::fs::read_dir(p.path().join("sessions")).ok())
        .flat_map(|sessions| sessions.flatten())
        .any(|f| f.path().extension().is_some_and(|e| e == "jsonl"))
}

/// `nori cloud` spawns the handroll binary with the `cloud-acp` argument and
/// round-trips a prompt through it like any local ACP agent.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_mode_round_trips_prompt_through_handroll() {
    let fake = FakeHandroll::new();
    let config = cloud_session_config(&fake).with_mock_response("Hello from cloud handroll!");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("nori cloud should start the TUI against the fake handroll");

    assert!(
        fake.wait_for_marker("argv", TIMEOUT),
        "nori cloud should have spawned the handroll binary"
    );
    let argv = std::fs::read_to_string(fake.marker("argv")).expect("read argv marker");
    assert!(
        argv.lines().any(|l| l == "cloud-acp"),
        "handroll should be invoked with the cloud-acp subcommand, got argv: {argv:?}"
    );

    std::thread::sleep(TIMEOUT_INPUT);
    session.send_str("ping across the tunnel").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("Hello from cloud handroll!", TIMEOUT)
        .expect("prompt should round-trip through the handroll child");
}

/// On TUI exit the handroll child must be released gracefully: stdin closed
/// and the child given time to run its EOF→release path — not SIGKILLed.
/// A killed process group never writes the `released` marker.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_mode_releases_handroll_gracefully_on_exit() {
    let fake = FakeHandroll::new();
    let config = cloud_session_config(&fake);

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("nori cloud should start");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("/exit").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // The marker appears while nori is still gracefully waiting on the child;
    // assert before dropping the PTY so a slow shutdown can't be SIGHUPed by
    // the closing master and produce a false failure.
    assert!(
        fake.wait_for_marker("released", Duration::from_secs(10)),
        "handroll child should see stdin EOF and complete its release path on exit; \
         missing marker means the child was killed before it could release the session"
    );
    drop(session);
}

/// A handroll child that dies mid-session must surface a loud error in the
/// TUI — not leave the session silently hung.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_mode_child_death_mid_session_surfaces_error() {
    let fake = FakeHandroll::crash_on_trigger();
    let config = cloud_session_config(&fake).with_mock_response("still alive");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("nori cloud should start");
    std::thread::sleep(TIMEOUT_INPUT);

    // Prove the session is live first.
    session.send_str("are you there").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("still alive", TIMEOUT)
        .expect("session should be live before the crash");

    // Trigger the crash and expect a visible error, including the child's
    // last words on stderr.
    std::fs::write(fake.marker("die"), "now").expect("write crash trigger");
    session
        .wait_for_text("exited unexpectedly", TIMEOUT)
        .expect("child death must surface as a visible error, not a hang");
    session
        .wait_for_text("tunnel collapsed", TIMEOUT)
        .expect("the error must include the child's recent stderr");
}

/// Cloud sessions record local transcripts like any other agent session
/// (intentional duplication with broker-side recording).
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_mode_writes_local_transcript() {
    let fake = FakeHandroll::new();
    let config = cloud_session_config(&fake).with_mock_response("transcribed response");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("nori cloud should start");
    let nori_home = session.nori_home_path().expect("nori home");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("write me down").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("transcribed response", TIMEOUT)
        .expect("should receive response");

    // Assert while the session (and its temp NORI_HOME) is still alive:
    // the transcript .jsonl is written during the session, not at exit.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut found = transcripts_exist(&nori_home);
    while !found && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
        found = transcripts_exist(&nori_home);
    }
    assert!(
        found,
        "cloud session should write a local transcript under {}",
        nori_home.display()
    );
}

/// Missing handroll binary fails with an actionable error before the TUI starts.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_mode_requires_handroll_binary() {
    // No NORI_HANDROLL_BIN override and no nori-handroll on PATH: the
    // install hint must appear. (A dangling override is a different error,
    // covered by the cli unit tests.)
    let mut config = SessionConfig::new().with_subcommand("cloud");
    config.exclude_binaries.push("nori-handroll".to_string());

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("nori-handroll", TIMEOUT)
        .expect("missing handroll binary should produce an actionable error");
    session
        .wait_for_text("Nori Sessions", TIMEOUT)
        .expect("error should tell the user to install Nori Sessions");
    let contents = session.screen_contents();
    assert!(
        !contents.contains("›"),
        "the TUI must not start when the handroll binary is missing, got: {contents}"
    );
}

/// `--agent` cannot bypass the handroll adapter in cloud mode.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_mode_overrides_agent_flag() {
    let fake = FakeHandroll::new();
    // The harness passes `--agent definitely-not-an-agent`; cloud mode must
    // ignore it and pin the handroll adapter.
    let config = cloud_session_config(&fake).with_model("definitely-not-an-agent".to_string());

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("cloud mode should start with the handroll adapter despite --agent");
    assert!(
        fake.wait_for_marker("argv", TIMEOUT),
        "the handroll binary should be spawned even when --agent names another agent"
    );
}

/// `[cloud] broker_url` from config.toml reaches the child as NORI_BROKER_URL.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_mode_passes_broker_url_from_config() {
    let fake = FakeHandroll::new();
    let config = cloud_session_config(&fake)
        .with_extra_config_toml("[cloud]\nbroker_url = \"http://broker.test:19400\"\n");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("nori cloud should start");
    assert!(
        fake.wait_for_marker("broker_url", TIMEOUT),
        "fake handroll should have dumped its NORI_BROKER_URL"
    );
    let broker_url = std::fs::read_to_string(fake.marker("broker_url")).expect("read broker_url");
    assert_eq!(
        broker_url.trim(),
        "http://broker.test:19400",
        "[cloud] broker_url must be passed to the child as NORI_BROKER_URL"
    );
}

/// When the child exits immediately (unauthenticated), its stderr message —
/// including the login hint — must reach the user.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_mode_surfaces_auth_error_from_child_stderr() {
    let fake = FakeHandroll::unauthenticated();
    let config = cloud_session_config(&fake);

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("nori-handroll login", TIMEOUT)
        .expect("the child's stderr auth message should be surfaced to the user");
}
