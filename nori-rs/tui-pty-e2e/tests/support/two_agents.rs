//! Plain `nori` with two synthetic adapters, Amber (the default) and Birch,
//! each tracing its pid and ACP methods to `fixture-traffic-<slug>`.
#![allow(dead_code)]
use std::fs;
use std::path::Path;
use std::time::Duration;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

pub const PAUSE: Duration = Duration::from_millis(900);

pub fn terminal(gates: &[&str]) -> (tempfile::TempDir, TuiSession) {
    let (home, mut tui) = launch(gates, &[]);
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    (home, tui)
}

/// Plain `nori` with `args` after it, once its composer is on screen.
pub fn launch(gates: &[&str], args: &[&str]) -> (tempfile::TempDir, TuiSession) {
    let home = tempfile::tempdir().unwrap();
    let folder = home.path().canonicalize().unwrap();
    for gate in ["fixture-traffic-amber", "fixture-traffic-birch"]
        .iter()
        .chain(gates)
    {
        fs::write(folder.join(gate), "").unwrap();
    }
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cli/tests/standard_acp/fixture.py"
    );
    let agent = |name: &str, slug: &str| {
        format!(
            r#"[[agents]]
name = "{name}"
slug = "{slug}"
[agents.distribution.local]
command = "python3"
args = [{script:?}, "{slug}", {folder:?}]
"#
        )
    };
    fs::write(
        folder.join("config.toml"),
        format!(
            "agent = \"amber\"\n[projects.{folder:?}]\ntrust_level = \"trusted\"\n{}{}",
            agent("Amber", "amber"),
            agent("Birch", "birch"),
        ),
    )
    .unwrap();
    let mut config = SessionConfig::new()
        .with_agent("amber".into())
        .with_cwd(&folder)
        .with_nori_home(&folder);
    for arg in args {
        config = config.with_arg(*arg);
    }
    // Two rows taller than the transcript needs, so the welcome card stays
    // on screen above the composer and its hint row. A positional prompt
    // can reach the composer before its first frame, so the row may already
    // show the editing keys.
    let mut tui = TuiSession::spawn_with_config(32, 110, config).unwrap();
    tui.wait_for(
        |screen| screen.contains("/ commands · ") || screen.contains("Enter send · "),
        TIMEOUT,
    )
    .unwrap();
    (home, tui)
}

/// The ACP methods the owned host received from this terminal, in order.
pub fn host_requests(tui: &TuiSession) -> Vec<String> {
    let Some(path) = tui.acp_log_path() else {
        return Vec::new();
    };
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.contains("host ACP request"))
        .filter_map(|line| line.split(" method=").nth(1))
        .map(|method| {
            method
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

/// Wait until the owned host has received `method`.
pub fn wait_for_host_request(tui: &mut TuiSession, method: &str) -> Vec<String> {
    let deadline = std::time::Instant::now() + TIMEOUT;
    loop {
        let requests = host_requests(tui);
        if requests.iter().any(|request| request == method) {
            return requests;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the owned host never received {method}: {requests:?} (log {:?})",
            tui.acp_log_path()
        );
        tui.settle(Duration::from_millis(50));
    }
}

/// Type `text`, then press Enter after the paste window has passed.
///
/// The harness reads the terminal only while it polls, so the pause keeps
/// polling. A bare sleep leaves the TUI blocked writing into a full PTY
/// buffer (macOS buffers far less than Linux); the keys then arrive in one
/// burst and Enter becomes part of a paste instead of submitting.
pub fn ask(tui: &mut TuiSession, text: &str) {
    tui.send_str(text).unwrap();
    tui.settle(PAUSE);
    tui.send_key(Key::Enter).unwrap();
}

pub fn trace(home: &Path, agent: &str) -> Vec<String> {
    fs::read_to_string(home.join(format!("fixture-traffic-{agent}")))
        .unwrap()
        .lines()
        .map(|line| line.split_once(' ').unwrap().1.to_string())
        .collect()
}

pub fn pids(home: &Path, agent: &str) -> Vec<String> {
    let mut pids: Vec<String> = fs::read_to_string(home.join(format!("fixture-traffic-{agent}")))
        .unwrap()
        .lines()
        .map(|line| line.split_once(' ').unwrap().0.to_string())
        .collect();
    pids.dedup();
    pids
}

pub fn running(pid: &str) -> bool {
    let out = std::process::Command::new("ps")
        .args(["-p", pid, "-o", "stat="])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&out.stdout);
    !stat.trim().is_empty() && !stat.trim_start().starts_with('Z')
}
