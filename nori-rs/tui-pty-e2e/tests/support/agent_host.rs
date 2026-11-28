//! A background host whose config names three synthetic ACP agent servers
//! (Amber, the default, and Birch, both started from the deterministic
//! fixture, and Cedar, whose launch command is not installed), and
//! `nori attach --host` terminals with their own homes and folders, whose
//! config knows none of them. Anything an attached terminal shows about
//! these agents came from the host.
//!
//! Each fixture process traces its pid and ACP methods to
//! `fixture-traffic-<slug>` in the host's project folder.
#![allow(dead_code)]

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

pub const PAUSE: Duration = Duration::from_millis(900);

pub struct AgentHost {
    pub nori_home: tempfile::TempDir,
    pub home: tempfile::TempDir,
    binary: PathBuf,
}

/// One attached terminal and the homes only it uses.
pub struct Attached {
    pub terminal: TuiSession,
    _nori_home: tempfile::TempDir,
    _home: tempfile::TempDir,
    _folder: tempfile::TempDir,
}

impl Attached {
    /// The attaching terminal's own folder.
    pub fn folder(&self) -> &Path {
        self._folder.path()
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        self.terminal.terminate_and_wait(Duration::from_secs(2));
    }
}

impl AgentHost {
    /// A host whose project folder holds the empty files in `gates`.
    pub fn new(gates: &[&str]) -> Self {
        let nori_home = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let project = nori_home.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let project = project.canonicalize().unwrap();
        for gate in ["fixture-traffic-amber", "fixture-traffic-birch"]
            .iter()
            .chain(gates)
        {
            std::fs::write(project.join(gate), "").unwrap();
        }
        let script = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cli/tests/standard_acp/fixture.py"
        );
        let fixture = |name: &str, slug: &str| {
            format!(
                "[[agents]]\nname = \"{name}\"\nslug = \"{slug}\"\n\
                 [agents.distribution.local]\ncommand = \"python3\"\n\
                 args = [{script:?}, \"{slug}\", {project:?}]\n"
            )
        };
        let quoted = serde_json::to_string(&project.to_string_lossy()).unwrap();
        std::fs::write(
            nori_home.path().join("config.toml"),
            format!(
                "agent = \"amber\"\n[projects.{quoted}]\ntrust_level = \"trusted\"\n{}{}\
                 [[agents]]\nname = \"Cedar\"\nslug = \"cedar\"\n\
                 [agents.distribution.local]\ncommand = \"cedar-adapter-absent\"\n",
                fixture("Amber", "amber"),
                fixture("Birch", "birch"),
            ),
        )
        .unwrap();
        let binary = std::env::current_exe()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("nori");
        Self {
            nori_home,
            home,
            binary,
        }
    }

    pub fn project(&self) -> PathBuf {
        self.nori_home
            .path()
            .join("project")
            .canonicalize()
            .unwrap()
    }

    fn spawn(&self, cwd: &Path, args: &[&str], nori_home: &Path, home: &Path) -> TuiSession {
        let mut config = SessionConfig::new()
            .with_command_args(args.iter().copied().map(str::to_owned))
            .with_agent_env("NORI_HOME", nori_home.to_string_lossy())
            .with_agent_env("CODEX_HOME", nori_home.to_string_lossy())
            .with_agent_env("HOME", home.to_string_lossy())
            .with_agent_env("NORI_NO_ANALYTICS", "1")
            .with_extra_path(self.binary.parent().unwrap().to_path_buf());
        config.cwd = Some(cwd.to_path_buf());
        TuiSession::spawn_with_config(40, 120, config).unwrap()
    }

    pub fn start(&self) {
        let mut terminal = self.spawn(
            &self.project(),
            &["--agent", "amber", "host", "--port", "0"],
            self.nori_home.path(),
            self.home.path(),
        );
        terminal
            .wait_for(|screen| screen.to_lowercase().contains("started"), TIMEOUT)
            .expect("nori host reports a ready background host");
        assert!(terminal.wait_for_process_exit(TIMEOUT), "host must detach");
    }

    /// `nori attach --host` from a terminal whose config names only the
    /// mock agent, once the host's session list is on screen.
    pub fn attach(&self) -> Attached {
        let state: serde_json::Value = serde_json::from_slice(
            &std::fs::read(self.nori_home.path().join("host/state.json")).unwrap(),
        )
        .unwrap();
        let endpoint = state["endpoint"].as_str().unwrap().to_owned();
        let token = self.nori_home.path().join("host/token");
        let nori_home = tempfile::tempdir().unwrap();
        std::fs::write(
            nori_home.path().join("config.toml"),
            "agent = \"mock-model\"\n",
        )
        .unwrap();
        let home = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let mut terminal = self.spawn(
            folder.path(),
            &[
                "attach",
                "--host",
                &endpoint,
                "--token-file",
                token.to_str().unwrap(),
            ],
            nori_home.path(),
            home.path(),
        );
        terminal.wait_for_text("Sessions", TIMEOUT).unwrap();
        Attached {
            terminal,
            _nori_home: nori_home,
            _home: home,
            _folder: folder,
        }
    }

    /// Plain `nori` in the host's project folder with the host's config, once
    /// its first session is ready. It runs its own private host.
    pub fn plain(&self) -> TuiSession {
        let mut terminal = self.spawn(
            &self.project(),
            &["--agent", "amber"],
            self.nori_home.path(),
            self.home.path(),
        );
        terminal.wait_until_ready().unwrap();
        terminal
    }

    /// The ACP methods `agent`'s fixture processes received, in order.
    pub fn trace(&self, agent: &str) -> Vec<String> {
        std::fs::read_to_string(self.project().join(format!("fixture-traffic-{agent}")))
            .unwrap()
            .lines()
            .filter_map(|line| line.split_once(' '))
            .map(|(_pid, request)| request.to_string())
            .collect()
    }

    /// Wait until `agent`'s fixture has received `line`.
    pub fn wait_for_trace(&self, agent: &str, line: &str) {
        let deadline = std::time::Instant::now() + TIMEOUT;
        while !self.trace(agent).iter().any(|seen| seen == line) {
            assert!(
                std::time::Instant::now() < deadline,
                "{agent} never received {line:?}: {:?}",
                self.trace(agent)
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The host's session rows, without the header.
    pub fn sessions(&self) -> Vec<String> {
        let output = std::process::Command::new(&self.binary)
            .args(["host", "ls"])
            .current_dir(self.project())
            .env("NORI_HOME", self.nori_home.path())
            .env("HOME", self.home.path())
            .env("NORI_NO_ANALYTICS", "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "host ls failed");
        String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .skip(1)
            .map(str::to_owned)
            .collect()
    }
}

impl Drop for AgentHost {
    fn drop(&mut self) {
        let _ = std::process::Command::new(&self.binary)
            .args(["host", "stop"])
            .current_dir(self.project())
            .env("NORI_HOME", self.nori_home.path())
            .env("HOME", self.home.path())
            .output();
    }
}

/// Type `text`, then press Enter after the paste window has passed.
pub fn ask(tui: &mut TuiSession, text: &str) {
    tui.send_str(text).unwrap();
    tui.settle(PAUSE);
    tui.send_key(Key::Enter).unwrap();
}

/// Close the session list the host opens with, leaving no session on screen.
pub fn close_list(tui: &mut TuiSession) {
    tui.press(
        Key::Escape,
        |screen| screen.contains("No session selected. Use"),
        "the session list closes",
    )
    .unwrap();
}

/// Move the focus of the picker titled `title` onto the row naming `row`,
/// down and then up; false when it never gets there.
pub fn focus(tui: &mut TuiSession, title: &str, row: &str) -> bool {
    let focused = |screen: &str| {
        screen
            .lines()
            .skip_while(|line| !line.contains(title))
            .any(|line| line.contains('›') && line.contains(row))
    };
    for key in [Key::Down, Key::Up] {
        for _ in 0..20 {
            if focused(&tui.screen_contents()) {
                return true;
            }
            tui.send_key(key).unwrap();
            tui.settle(PAUSE / 3);
        }
    }
    focused(&tui.screen_contents())
}
