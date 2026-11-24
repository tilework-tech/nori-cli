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
    let config = SessionConfig::new()
        .with_agent("amber".into())
        .with_cwd(&folder)
        .with_nori_home(&folder);
    let mut tui = TuiSession::spawn_with_config(30, 110, config).unwrap();
    tui.wait_for_text("›", TIMEOUT).unwrap();
    ask(&mut tui, "whoami");
    tui.wait_for_text("amber is answering (1).", TIMEOUT)
        .unwrap();
    (home, tui)
}

pub fn ask(tui: &mut TuiSession, text: &str) {
    tui.send_str(text).unwrap();
    std::thread::sleep(PAUSE);
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
