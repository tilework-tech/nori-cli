//! Plain nori waits for the adapter to release its session. A stuck adapter
//! cannot hold the private host open when the terminal quits.
#![cfg(unix)]
use std::fs;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

fn terminal(gate: &str) -> (tempfile::TempDir, TuiSession) {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().canonicalize().unwrap();
    for path in [gate, "fixture-traffic"] {
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
