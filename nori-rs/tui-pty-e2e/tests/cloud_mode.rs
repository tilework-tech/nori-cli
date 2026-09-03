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
/// agent exits (it exits 0 on stdin EOF), the script APPENDS an `eof` line to
/// the `released` marker — one line per child that saw EOF and finished its
/// own teardown (post-#1276 that teardown is a *detach*, not a release).
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
             echo $$ >> '{dir}/pids'\n\
             '{mock}' 2>>'{dir}/agent_stderr'\n\
             status=$?\n\
             echo eof >> '{dir}/released'\n\
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

    fn wait_for_marker_text(&self, name: &str, expected: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if std::fs::read_to_string(self.marker(name))
                .is_ok_and(|contents| contents.trim() == expected)
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    fn wait_for_child_pid_count(&self, count: usize, timeout: Duration) -> Vec<u32> {
        let deadline = Instant::now() + timeout;
        loop {
            let pids = std::fs::read_to_string(self.marker("pids"))
                .unwrap_or_default()
                .lines()
                .filter_map(|pid| pid.parse().ok())
                .collect::<Vec<_>>();
            if pids.len() >= count || Instant::now() >= deadline {
                return pids;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn wait_for_child_exit(&self, pid: u32, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !Path::new(&format!("/proc/{pid}")).exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    fn child_is_alive(pid: u32) -> bool {
        let Ok(status) = std::fs::read_to_string(format!("/proc/{pid}/status")) else {
            return false;
        };
        status
            .lines()
            .find(|line| line.starts_with("State:"))
            .is_some_and(|state| !state.contains("Z (zombie)") && !state.contains("Z ("))
    }

    /// Number of children that have completed their stdin-EOF path so far.
    fn released_count(&self) -> usize {
        std::fs::read_to_string(self.marker("released"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }

    fn wait_for_released_above(&self, baseline: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.released_count() > baseline {
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

/// Cloud config where the mock agent advertises the full cloud session
/// lifecycle (`sessionCapabilities.{list,resume,close}`, `loadSession:false`)
/// — the real `nori-handroll cloud-acp` contract. This is what makes the
/// picker-first entry flow eligible.
fn cloud_lifecycle_config(fake: &FakeHandroll) -> SessionConfig {
    cloud_session_config(fake)
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_RESUME", "1")
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_CLOSE", "1")
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

/// On TUI exit the handroll child gets stdin EOF first — the detach signal —
/// and a cooperative child completes its EOF path. (Post-#1276 EOF detaches
/// the session; `session/close` is the only terminal verb. The hard-exit
/// watchdog may SIGKILL a child that *ignores* EOF — covered separately —
/// but a prompt child must be allowed to finish cleanly.)
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_mode_sends_eof_detach_to_handroll_on_exit() {
    let fake = FakeHandroll::new();
    let config = cloud_session_config(&fake).with_mock_response("alive before exit");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("nori cloud should start");
    std::thread::sleep(TIMEOUT_INPUT);
    // Prove the prepared child was activated and is live.
    session.send_str("prove the session is live").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("alive before exit", TIMEOUT)
        .expect("session should be live before exit");
    let baseline = fake.released_count();

    session.send_str("/exit").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // Assert before dropping the PTY so a slow shutdown can't be SIGHUPed by
    // the closing master and produce a false failure.
    assert!(
        fake.wait_for_released_above(baseline, Duration::from_secs(10)),
        "the live handroll child should see stdin EOF and complete its detach path on exit"
    );
    drop(session);
}

/// `nori cloud` against a lifecycle-capable agent must boot into the session
/// picker — live sessions listed by title, an explicit "start new" row — and
/// must NOT claim a session before the user picks.
/// `MOCK_AGENT_FAIL_NEW_SESSION_FROM=0` turns any premature `session/new`
/// into a loud failure, so the picker appearing proves nothing was claimed.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_mode_boots_into_session_picker_without_claiming() {
    let fake = FakeHandroll::new();
    let config =
        cloud_lifecycle_config(&fake).with_agent_env("MOCK_AGENT_FAIL_NEW_SESSION_FROM", "0");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("Start a new session", TIMEOUT)
        .expect("cloud entry should open the session picker with a create-new row");
    session
        .wait_for_text("First mock session", TIMEOUT)
        .expect("the picker should list live sessions by their broker title");

    // The initialized child must remain alive so the selected session can be
    // activated on this exact connection rather than a respawned one.
    let pids = fake.wait_for_child_pid_count(1, TIMEOUT);
    let prepared_pid = *pids.first().expect("prepared handroll child pid");
    assert!(
        FakeHandroll::child_is_alive(prepared_pid),
        "the prepared child must remain alive while the picker is open"
    );
    // Belt and braces on "nothing claimed": the mock logs every session/new
    // to stderr; none may have happened.
    let agent_stderr = std::fs::read_to_string(fake.marker("agent_stderr")).unwrap_or_default();
    assert!(
        !agent_stderr.contains("new_session id="),
        "no session/new may run before the user picks, agent stderr:\n{agent_stderr}"
    );
}

/// Picker-first entry must retain the CLI's positional prompt until the user
/// explicitly starts a new session on the prepared connection.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_start_new_preserves_the_deferred_positional_prompt() {
    let fake = FakeHandroll::new();
    let config = cloud_lifecycle_config(&fake)
        .with_mock_response("deferred prompt reached the prepared session")
        .with_arg("continue from this positional prompt");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("Start a new session", TIMEOUT)
        .expect("cloud entry should open the session picker");
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("deferred prompt reached the prepared session", TIMEOUT)
        .expect("starting new should submit the deferred positional prompt");
}

/// An explicit `/new` waits for picker-first preparation and activates that
/// exact child instead of cancelling, respawning, or reopening the picker.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_new_reuses_in_flight_entry_preparation() {
    let fake = FakeHandroll::new();
    let config = cloud_lifecycle_config(&fake)
        .with_agent_env("MOCK_AGENT_STARTUP_DELAY_MS", "1200")
        .with_mock_response("new session remains active");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("Listing", TIMEOUT)
        .expect("cloud entry should begin preparing the session picker");
    let initial_pids = fake.wait_for_child_pid_count(1, TIMEOUT);
    let prepared_pid = *initial_pids.first().expect("entry-preparation child pid");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_str("/new").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session.send_str("prove the explicit session won").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("new session remains active", TIMEOUT)
        .expect("the explicit new session should remain usable");

    let pids = fake.wait_for_child_pid_count(2, Duration::from_millis(300));
    assert_eq!(
        pids,
        vec![prepared_pid],
        "/new must reuse the prepared child"
    );
    assert!(
        FakeHandroll::child_is_alive(prepared_pid),
        "the prepared child should become the active session"
    );

    let agent_stderr = std::fs::read_to_string(fake.marker("agent_stderr")).unwrap_or_default();
    let creates = agent_stderr
        .lines()
        .filter(|line| line.contains("new_session id="))
        .count();
    assert_eq!(
        creates, 1,
        "only the explicit /new may claim a session, agent stderr:\n{agent_stderr}"
    );
}

/// Plain `nori` (no cloud subcommand) must NOT get picker-first entry, even
/// against an agent that advertises the full session lifecycle — the picker
/// boot is a cloud-entry behavior, not a capability reflex.
#[test]
#[cfg(target_os = "linux")]
fn test_plain_nori_does_not_boot_into_the_agent_session_picker() {
    let config = SessionConfig::new()
        .with_agent("mock-model".to_string())
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_LIST", "1")
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_RESUME", "1")
        .with_agent_env("MOCK_AGENT_SUPPORT_SESSION_CLOSE", "1");

    let mut session = TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("plain nori should boot straight to the composer");
    let contents = session.screen_contents();
    assert!(
        !contents.contains("Start a new session"),
        "plain nori must not open the agent session picker on boot, got:\n{contents}"
    );
}

/// Picking a listed session from the entry picker reattaches to it via
/// `session/load` when the cloud facade advertises replay support (never
/// `session/new` — enforced by the fail-new guard), identifies the reattached
/// session, and round-trips a prompt on it.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_entry_picker_resume_row_reattaches_live() {
    let fake = FakeHandroll::new();
    let config = cloud_lifecycle_config(&fake)
        .with_agent_env("MOCK_AGENT_SUPPORT_LOAD_SESSION", "1")
        .with_agent_env("MOCK_AGENT_FAIL_NEW_SESSION_FROM", "0")
        .with_mock_response("hello from the reattached session");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("First mock session", TIMEOUT)
        .expect("cloud entry should open the session picker");
    std::thread::sleep(TIMEOUT_INPUT);

    // Row 0 is "Start a new session"; row 1 is the first listed session.
    session.send_key(Key::Down).unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("Reattaching to mock-session-1", TIMEOUT)
        .expect("reattach must identify the selected cloud session");
    session
        .wait_for_text("›", TIMEOUT)
        .expect("the composer should be ready after reattach");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("ping the reattached session").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("hello from the reattached session", TIMEOUT)
        .expect("prompt should round-trip on the reattached session");

    // The stderr log covers the one child used for listing and reattachment.
    let agent_stderr = std::fs::read_to_string(fake.marker("agent_stderr")).unwrap_or_default();
    assert!(
        !agent_stderr.contains("new_session id="),
        "reattach must never create a session in any child, agent stderr:\n{agent_stderr}"
    );
}

/// The "Start a new session" row claims a fresh session only when picked —
/// and the session is then fully usable.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_entry_picker_create_new_starts_fresh_session() {
    let fake = FakeHandroll::new();
    let config = cloud_lifecycle_config(&fake).with_mock_response("fresh session says hi");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("Start a new session", TIMEOUT)
        .expect("cloud entry should open the session picker");
    std::thread::sleep(TIMEOUT_INPUT);

    // Row 0 is the create-new row.
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("›", TIMEOUT)
        .expect("picking create-new should start a fresh session");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("hello fresh session").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("fresh session says hi", TIMEOUT)
        .expect("prompt should round-trip on the fresh session");
}

/// Claiming a fresh Cloud box can take several seconds. During that gap the
/// TUI must explain what it is doing and keep an attempted prompt as a draft
/// instead of silently queueing it before the session is ready.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_create_new_shows_connection_progress_and_blocks_early_submit() {
    let fake = FakeHandroll::new();
    let release_file = fake.marker("release_new_session");
    let config = cloud_lifecycle_config(&fake)
        .with_agent_env(
            "MOCK_AGENT_NEW_SESSION_RELEASE_FILE",
            release_file.to_string_lossy(),
        )
        .with_mock_response("draft submitted after cloud connected");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("Start a new session", TIMEOUT)
        .expect("cloud entry should open the session picker");
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("Connecting to Nori Cloud", TIMEOUT)
        .expect("claiming a new cloud session should show connection progress");

    session.send_str("draft while connecting").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        session.screen_contents().contains("draft while connecting"),
        "an early Enter must keep the text in the composer"
    );
    let agent_stderr = std::fs::read_to_string(fake.marker("agent_stderr")).unwrap_or_default();
    assert!(
        !agent_stderr.contains("Mock agent: prompt"),
        "the draft must not be queued during connection:\n{agent_stderr}"
    );

    std::fs::write(release_file, "ready").expect("release delayed session/new");

    session
        .wait_for_text("Directory", TIMEOUT)
        .expect("the connected session should render its initial status card");
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        session
            .screen_contents()
            .contains("Connecting to Nori Cloud"),
        "connection progress should remain in history after the status card appears"
    );
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("draft submitted after cloud connected", TIMEOUT)
        .expect("the preserved draft should submit once the user retries after connection");
    let agent_stderr = std::fs::read_to_string(fake.marker("agent_stderr")).unwrap_or_default();
    assert_eq!(
        agent_stderr.matches("Mock agent: prompt").count(),
        1,
        "the preserved draft should cross the ACP boundary exactly once:\n{agent_stderr}"
    );
}

/// A structured ACP failure from Handroll must remain actionable in terminal
/// history without exposing unrelated machine-readable error metadata.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_create_new_error_shows_message_and_detail_without_json_noise() {
    let fake = FakeHandroll::new();
    let config =
        cloud_lifecycle_config(&fake).with_agent_env("MOCK_AGENT_FAIL_NEW_SESSION_JSON", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("Start a new session", TIMEOUT)
        .expect("cloud entry should open the session picker");
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("broker unreachable", TIMEOUT)
        .expect("the ACP error message should appear in history");
    session
        .wait_for_text("connection reset by broker", TIMEOUT)
        .expect("the ACP error detail should appear in history");

    let contents = session.screen_contents();
    assert!(!contents.contains("retry_after_ms"), "{contents}");
    assert!(!contents.contains("trace_id"), "{contents}");
}

/// /close releases the session and returns to the session picker — it must
/// NOT auto-claim a fresh session ("swap" semantics are gone).
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_close_returns_to_the_picker() {
    let fake = FakeHandroll::new();
    let config = cloud_lifecycle_config(&fake);

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("Start a new session", TIMEOUT)
        .expect("cloud entry should open the session picker");
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();
    session
        .wait_for_text("›", TIMEOUT)
        .expect("create-new should start a session");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("/close").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("Session closed", TIMEOUT)
        .expect("/close should confirm the release");
    session
        .wait_for_text("Start a new session", TIMEOUT)
        .expect("/close should land back on the session picker, not a fresh chat");
}

/// If an agent advertises `session/list` but the request fails, preparation
/// fails loudly and must never fall through to `session/new`.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_list_failure_does_not_claim_a_new_session() {
    let fake = FakeHandroll::new();
    let config = cloud_lifecycle_config(&fake).with_agent_env("MOCK_AGENT_LIST_SESSIONS_FAIL", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("Couldn't prepare agent", TIMEOUT)
        .expect("the advertised list failure should fail preparation");
    session
        .wait_for_text("No session is active", TIMEOUT)
        .expect("list failure must leave explicit retry and new-session choices");

    let agent_stderr = std::fs::read_to_string(fake.marker("agent_stderr")).unwrap_or_default();
    let creates = agent_stderr
        .lines()
        .filter(|line| line.contains("new_session id="))
        .count();
    assert_eq!(
        creates, 0,
        "list failure must not create a session, agent stderr:\n{agent_stderr}"
    );
}

/// Quit must force cleanup after the cloud detach grace even when the child
/// ignores stdin EOF entirely — the old 25s shutdown grace held the whole TUI
/// hostage.
#[test]
#[cfg(target_os = "linux")]
fn test_cloud_quit_exits_fast_even_if_child_ignores_eof() {
    let fake = FakeHandroll::new();
    let config = cloud_session_config(&fake).with_agent_env("MOCK_AGENT_IGNORE_EOF", "1");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("Failed to spawn nori cloud");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("nori cloud should start");
    std::thread::sleep(TIMEOUT_INPUT);

    session.send_str("/quit").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    // The owned child lifecycle caps detach grace at ~1s; 5s here is CI slack.
    // The old behavior waited out a 25s child-exit grace, which this must never
    // do.
    assert!(
        session.wait_for_process_exit(Duration::from_secs(5)),
        "the TUI must finish bounded cleanup even when the agent child ignores EOF"
    );
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
    assert!(
        !session.wait_for_process_exit(Duration::from_millis(500)),
        "an agent connection failure must not exit the client interface"
    );
    assert!(
        !session.screen_contents().contains("Goodbye!"),
        "an agent connection failure must preserve the error state, not render an exit summary"
    );
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
    let config = cloud_session_config(&fake).with_agent("definitely-not-an-agent".to_string());

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
        fake.wait_for_marker_text("broker_url", "http://broker.test:19400", TIMEOUT),
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
        .wait_for(
            |screen| {
                let visible_text: String = screen.split_whitespace().collect();
                visible_text
                    .contains("AuthenticationrequiredforNoriCloudACP.run:nori-handrolllogin")
            },
            TIMEOUT,
        )
        .expect("the auth error should include an actionable login command");
}
