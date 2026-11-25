//! A background host and `nori attach --host` terminals that share no home
//! with it. The host runs with its own `NORI_HOME` and `HOME`; every
//! attaching terminal gets a different `NORI_HOME`, `HOME` and folder. So
//! anything an attached terminal shows that lives in a home came from the
//! host.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TuiSession;

pub const ANSWER: &str = "joined host answer";

pub struct JoinedHost {
    pub nori_home: tempfile::TempDir,
    pub home: tempfile::TempDir,
    binary: PathBuf,
    agent_env: Vec<(String, String)>,
    real_instruction_files: bool,
    agent: &'static str,
}

/// One attached terminal and the homes only it uses.
pub struct Attached {
    pub terminal: TuiSession,
    _nori_home: tempfile::TempDir,
    _home: tempfile::TempDir,
    _folder: tempfile::TempDir,
}

impl Drop for Attached {
    fn drop(&mut self) {
        self.terminal.terminate_and_wait(Duration::from_secs(2));
    }
}

impl JoinedHost {
    /// A host whose agent processes also get `agent_env`.
    pub fn new(agent_env: &[(&str, &str)]) -> Self {
        let nori_home = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let project = nori_home.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let quoted = serde_json::to_string(&project.to_string_lossy()).unwrap();
        std::fs::write(
            nori_home.path().join("config.toml"),
            format!("agent = \"mock-model\"\n[projects.{quoted}]\ntrust_level = \"trusted\"\n"),
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
            agent_env: agent_env
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            real_instruction_files: false,
            agent: "mock-model",
        }
    }

    /// The host reads the instruction files that are really on disk rather
    /// than the harness's fixed list, and runs the mock agent as a
    /// Codex-named agent, which loads `AGENTS.md`.
    #[allow(dead_code)]
    pub fn with_real_instruction_files(mut self) -> Self {
        let mock = self
            .binary
            .with_file_name(format!("mock_acp_agent{}", std::env::consts::EXE_SUFFIX));
        let command = serde_json::to_string(&mock.to_string_lossy()).unwrap();
        let config = self.nori_home.path().join("config.toml");
        let mut toml = std::fs::read_to_string(&config).unwrap();
        toml = toml.replacen("agent = \"mock-model\"", "agent = \"codex-mock\"", 1);
        toml.push_str(&format!(
            "[[agents]]\nname = \"Codex Mock\"\nslug = \"codex-mock\"\n[agents.distribution.local]\ncommand = {command}\n"
        ));
        std::fs::write(config, toml).unwrap();
        self.real_instruction_files = true;
        self.agent = "codex-mock";
        self
    }

    pub fn project(&self) -> PathBuf {
        self.nori_home.path().join("project")
    }

    fn spawn(
        &self,
        cwd: &Path,
        agent: &str,
        args: &[&str],
        nori_home: &Path,
        home: &Path,
    ) -> TuiSession {
        let mut config = SessionConfig::new()
            .with_command_args(
                ["--agent", agent]
                    .into_iter()
                    .chain(args.iter().copied())
                    .map(str::to_owned),
            )
            .with_agent_env("NORI_HOME", nori_home.to_string_lossy())
            .with_agent_env("CODEX_HOME", nori_home.to_string_lossy())
            .with_agent_env("HOME", home.to_string_lossy())
            .with_agent_env("NORI_NO_ANALYTICS", "1")
            .with_agent_env("MOCK_AGENT_RESPONSE", ANSWER)
            .with_extra_path(self.binary.parent().unwrap().to_path_buf());
        for (key, value) in &self.agent_env {
            config = config.with_agent_env(key.clone(), value.clone());
        }
        if self.real_instruction_files {
            config = config.with_real_instruction_files();
        }
        config.cwd = Some(cwd.to_path_buf());
        TuiSession::spawn_with_config(40, 120, config).unwrap()
    }

    pub fn start(&self) {
        let mut terminal = self.spawn(
            &self.project(),
            self.agent,
            &["host", "--port", "0"],
            self.nori_home.path(),
            self.home.path(),
        );
        terminal
            .wait_for(|screen| screen.to_lowercase().contains("started"), TIMEOUT)
            .expect("nori host reports a ready background host");
        assert!(terminal.wait_for_process_exit(TIMEOUT), "host must detach");
    }

    /// Attach with `--host` from a terminal with its own homes and folder,
    /// and wait for the host's session list.
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
            "mock-model",
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

    /// Attach and start a session in the host's project folder.
    pub fn attach_new(&self) -> Attached {
        let mut attached = self.attach();
        let terminal = &mut attached.terminal;
        terminal.send_key(Key::Escape).unwrap();
        terminal
            .wait_for(
                |screen| screen.contains("No session selected. Use"),
                TIMEOUT,
            )
            .unwrap();
        let project = self.project();
        terminal
            .submit_input(&format!("/new {}", project.display()))
            .unwrap();
        terminal.wait_for_text("ready", TIMEOUT).unwrap();
        attached
    }

    /// Attach and open the first session the host lists.
    pub fn attach_first(&self) -> Attached {
        let mut attached = self.attach();
        let terminal = &mut attached.terminal;
        // The picker opens before the host's first list lands; Enter on an
        // empty picker opens nothing, so wait for a listed session first.
        terminal
            .wait_for(
                |screen| screen.contains("enter open") && screen.contains(" ago"),
                TIMEOUT,
            )
            .unwrap();
        terminal.send_key(Key::Enter).unwrap();
        // "›" also marks the picker's selected row; the session view is the
        // one whose hint row offers to detach.
        terminal
            .wait_for(
                |screen| screen.contains("ctrl+d detach") && !screen.contains("enter open"),
                TIMEOUT,
            )
            .unwrap();
        attached
    }

    pub fn stop(&self) {
        let mut child = Command::new(&self.binary)
            .args(["--agent", self.agent, "host", "stop"])
            .current_dir(self.project())
            .env("NORI_HOME", self.nori_home.path())
            .env("CODEX_HOME", self.nori_home.path())
            .env("HOME", self.home.path())
            .env("NORI_NO_ANALYTICS", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(35);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                let _ = child.kill();
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for JoinedHost {
    fn drop(&mut self) {
        self.stop();
    }
}
