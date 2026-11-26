//! Plain nori waits for the adapter to release its session. A stuck adapter
//! cannot hold the private host open when the terminal quits.
#![cfg(unix)]
use std::fs;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

fn fixture_home(gates: &[&str]) -> (tempfile::TempDir, std::path::PathBuf) {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().canonicalize().unwrap();
    for path in gates.iter().chain(&["fixture-traffic"]) {
        fs::write(folder.join(path), "").unwrap();
    }
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cli/tests/standard_acp/fixture.py"
    );
    fs::write(
        folder.join("config.toml"),
        format!(
            r#"agent = "amber"
[projects.{folder:?}]
trust_level = "trusted"
[[agents]]
name = "Amber"
slug = "amber"
[agents.distribution.local]
command = "python3"
args = [{script:?}, "close-sample"]
"#
        ),
    )
    .unwrap();
    (home, folder)
}

fn terminal(gate: &str) -> (tempfile::TempDir, TuiSession) {
    let (home, folder) = fixture_home(&[gate]);
    let config = SessionConfig::new()
        .with_agent("amber".into())
        .with_cwd(&folder)
        .with_nori_home(&folder)
        .with_arg("finish");
    let mut tui = TuiSession::spawn_with_config(32, 110, config).unwrap();
    tui.wait_for_text("Sample turn complete.", TIMEOUT).unwrap();
    tui.submit_input("/close").unwrap();
    (home, tui)
}

#[test]
fn refused_close_keeps_the_original_session_until_the_retry_releases_it() {
    let (home, mut tui) = terminal("fixture-refuse-close");
    tui.wait_for_text("sample close declined", TIMEOUT).unwrap();
    tui.submit_input("finish").unwrap();
    tui.wait_for(
        |_| {
            fs::read_to_string(home.path().join("fixture-traffic"))
                .unwrap()
                .matches("session/prompt")
                .count()
                == 2
        },
        TIMEOUT,
    )
    .unwrap();
    tui.wait_for_text("Sample turn complete.", TIMEOUT).unwrap();
    let screen = tui.screen_contents();
    assert_eq!(
        screen.matches("sample close declined").count(),
        1,
        "one refusal per refused close:\n{screen}"
    );
    tui.submit_input("/close").unwrap();
    tui.wait_for_text("Start a new session", TIMEOUT).unwrap();
    let trace = fs::read_to_string(home.path().join("fixture-traffic")).unwrap();
    let calls: Vec<_> = trace
        .lines()
        .filter(|line| {
            line.starts_with("session/close")
                || line.starts_with("session/prompt")
                || line.starts_with("session/cancel")
        })
        .collect();
    assert_eq!(
        calls,
        [
            "session/prompt 0",
            "session/close 0",
            "session/prompt 0",
            "session/close 0"
        ]
    );
}

#[test]
fn quitting_stops_a_private_host_with_an_unanswered_close() {
    let (home, mut tui) = terminal("fixture-hold-close");
    tui.wait_for(
        |_| home.path().join("fixture-close-started").exists(),
        TIMEOUT,
    )
    .unwrap();
    let pid = fs::read_to_string(home.path().join("fixture-close-started")).unwrap();
    tui.submit_input("/quit").unwrap();
    assert!(tui.wait_for_process_exit(TIMEOUT));
    let adapter = std::process::Command::new("ps")
        .args(["-p", &pid, "-o", "stat="])
        .output()
        .unwrap();
    assert!(
        adapter.stdout.is_empty()
            || String::from_utf8_lossy(&adapter.stdout)
                .trim_start()
                .starts_with('Z'),
        "adapter {pid} survived /quit"
    );
    let trace = fs::read_to_string(home.path().join("fixture-traffic")).unwrap();
    assert_eq!(trace.matches("session/close ").count(), 1);
}

/// `nori host` and `nori attach` on the fixture home, with `args` after them.
/// Background hosts are exercised on Linux only, like `host_attach.rs`.
#[cfg(target_os = "linux")]
fn host_terminal(folder: &std::path::Path, args: &[&str]) -> TuiSession {
    let binary = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("nori");
    let config = SessionConfig::new()
        .with_agent("amber".into())
        .with_command_args(args.iter().copied())
        .with_cwd(folder)
        .with_nori_home(folder)
        .with_agent_env("NORI_NO_ANALYTICS", "1")
        .with_extra_path(binary.parent().unwrap().to_path_buf());
    TuiSession::spawn_with_config(32, 110, config).unwrap()
}

#[cfg(target_os = "linux")]
struct StopHost(std::path::PathBuf);

#[cfg(target_os = "linux")]
impl Drop for StopHost {
    fn drop(&mut self) {
        let mut stop = host_terminal(&self.0, &["host", "stop"]);
        stop.wait_for_process_exit(TIMEOUT);
    }
}

#[cfg(target_os = "linux")]
#[test]
fn attach_reports_a_refused_close_once_and_keeps_the_session_for_a_retry() {
    let (_home, folder) = fixture_home(&[]);
    let mut host = host_terminal(&folder, &["host", "--port", "0"]);
    let _stop = StopHost(folder.clone());
    host.wait_for(|screen| screen.to_lowercase().contains("started"), TIMEOUT)
        .unwrap();
    assert!(host.wait_for_process_exit(TIMEOUT));

    let mut tui = host_terminal(&folder, &["attach"]);
    tui.wait_for_text("Sessions", TIMEOUT).unwrap();
    tui.press(
        Key::Escape,
        |screen| screen.contains("No session selected"),
        "No session selected",
    )
    .unwrap();
    tui.submit_input("/new").unwrap();
    tui.wait_for_text("ready", TIMEOUT).unwrap();
    tui.submit_input("finish").unwrap();
    tui.wait_for_text("Sample turn complete.", TIMEOUT).unwrap();

    fs::write(folder.join("fixture-refuse-close"), "").unwrap();
    tui.submit_input("/close").unwrap();
    tui.wait_for_text("sample close declined", TIMEOUT).unwrap();
    tui.submit_input("finish").unwrap();
    let traffic = || fs::read_to_string(folder.join("fixture-traffic")).unwrap();
    tui.wait_for(
        |_| traffic().matches("session/prompt").count() == 2,
        TIMEOUT,
    )
    .unwrap();
    tui.wait_for(
        |screen| screen.matches("Sample turn complete.").count() == 2,
        TIMEOUT,
    )
    .unwrap();
    let screen = tui.screen_contents();
    assert_eq!(
        screen.matches("sample close declined").count(),
        1,
        "one refusal per refused close:\n{screen}"
    );

    tui.submit_input("/close").unwrap();
    tui.wait_for(|_| traffic().matches("session/close").count() == 2, TIMEOUT)
        .unwrap();
    tui.wait_for_text("Session ended. Use /new", TIMEOUT)
        .unwrap();
    let calls: Vec<_> = traffic()
        .lines()
        .filter(|line| line.starts_with("session/close") || line.starts_with("session/prompt"))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        calls,
        [
            "session/prompt 0",
            "session/close 0",
            "session/prompt 0",
            "session/close 0"
        ]
    );
}
