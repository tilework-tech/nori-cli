//! Inline tool output is full unless compact is chosen for a project or set globally,
//! and a line cap keeps only the head and tail when one is set.
#![cfg(target_os = "linux")]

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;
use tui_pty_e2e::has_composer_row;

const TRUST_PROMPT: &str = "Work without asking";
const COMPACT_QUESTION: &str = "Tool output";
const REPLY: &str = "All three widget tests pass.";
/// Only the full view shows output lines before the last one.
const EARLY_OUTPUT_LINE: &str = "test widgets::wraps_long_text ... ok";
const LAST_OUTPUT_LINE: &str = "test result: ok. 3 passed; 0 failed";

struct Workspace {
    _root: tempfile::TempDir,
    home: PathBuf,
}

impl Workspace {
    fn new(config: &str) -> Self {
        let root = tempfile::tempdir().expect("create workspace");
        let home = root.path().join("home");
        std::fs::create_dir(&home).expect("create NORI_HOME");
        std::fs::write(home.join("config.toml"), config).expect("write config");
        Self { _root: root, home }
    }

    fn project(&self, name: &str) -> PathBuf {
        let dir = self.home.parent().expect("workspace root").join(name);
        std::fs::create_dir(&dir).expect("create project");
        std::fs::write(dir.join("README.md"), "synthetic project\n").expect("write file");
        std::fs::canonicalize(dir).expect("canonical project path")
    }

    fn git_project(&self, name: &str) -> PathBuf {
        let dir = self.project(name);
        let status = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&dir)
            .status()
            .expect("run git init");
        assert!(status.success());
        std::fs::create_dir_all(dir.join("src/widgets")).expect("create subdirectory");
        dir
    }

    fn launch(&self, cwd: &Path) -> TuiSession {
        self.launch_with_override(cwd, None)
    }

    fn launch_with_override(&self, cwd: &Path, override_arg: Option<&str>) -> TuiSession {
        let mut config = SessionConfig::new()
            .with_nori_home(&self.home)
            .with_skip_trust_directory(false)
            .with_agent_env("MOCK_AGENT_SEND_COMMAND_OUTPUT", "1")
            .with_mock_response(REPLY)
            .with_cwd(cwd);
        if let Some(arg) = override_arg {
            config = config.with_arg("-c").with_arg(arg);
        }
        TuiSession::spawn_with_config(40, 100, config).expect("spawn nori")
    }

    fn config(&self) -> String {
        std::fs::read_to_string(self.home.join("config.toml")).expect("read config")
    }
}

/// Waits for the trust form to finish unrolling (keys pressed while it
/// unrolls only reveal it), then picks "Work without asking".
fn open_trust_form(session: &mut TuiSession) -> String {
    session
        .wait_for(
            |screen| screen.contains(TRUST_PROMPT) && screen.contains("Enter start"),
            TIMEOUT,
        )
        .expect("trust form");
    session.settle(Duration::from_millis(300));
    let screen = session.screen_contents();
    session.send_key(Key::Char('y')).expect("trust directory");
    screen
}

/// Picks "Work without asking" on a trust form that also asks about tool
/// output; the caller answers that and submits.
fn trust(session: &mut TuiSession) {
    let screen = open_trust_form(session);
    assert!(screen.contains(COMPACT_QUESTION), "{screen}");
}

fn submit(session: &mut TuiSession) {
    session.send_key(Key::Enter).expect("submit trust form");
}

fn run_command(session: &mut TuiSession) -> String {
    session
        .wait_for(
            |screen| {
                has_composer_row(screen)
                    && !screen.contains(TRUST_PROMPT)
                    && !screen.contains(COMPACT_QUESTION)
            },
            TIMEOUT,
        )
        .expect("composer after onboarding");
    std::thread::sleep(TIMEOUT_INPUT);
    session
        .submit_input("run the widget tests")
        .expect("submit prompt");
    session.wait_for_text(REPLY, TIMEOUT).expect("agent reply");
    session.settle(Duration::from_millis(300));
    session.history_contents()
}

fn assert_full(screen: &str) {
    assert!(
        screen.contains(EARLY_OUTPUT_LINE) && screen.contains("--package demo-widgets"),
        "expected full tool output:\n{screen}"
    );
}

fn assert_compact(screen: &str) {
    assert!(
        screen.contains(LAST_OUTPUT_LINE) && !screen.contains(EARLY_OUTPUT_LINE),
        "expected compact tool output:\n{screen}"
    );
}

#[test]
fn leaving_tool_output_untouched_keeps_full_output_and_writes_no_preference() {
    let workspace = Workspace::new("agent = \"mock-model\"\n");
    let project = workspace.project("alpha");

    let mut session = workspace.launch(&project);
    trust(&mut session);
    submit(&mut session);
    assert_full(&run_command(&mut session));

    let config = workspace.config();
    assert!(config.contains("trust_level = \"trusted\""), "{config}");
    assert!(!config.contains("tool_output"), "{config}");
}

#[test]
fn compact_opt_in_survives_relaunch_and_stays_in_its_project() {
    let workspace = Workspace::new("agent = \"mock-model\"\n");
    let compact_project = workspace.git_project("alpha");
    let other_project = workspace.project("beta");

    let mut first = workspace.launch(&compact_project.join("src/widgets"));
    trust(&mut first);
    first.send_key(Key::Char('c')).expect("choose compact");
    submit(&mut first);
    assert_compact(&run_command(&mut first));
    assert!(first.terminate_and_wait(TIMEOUT));

    let mut relaunched = workspace.launch(&compact_project);
    assert_compact(&run_command(&mut relaunched));
    assert!(relaunched.terminate_and_wait(TIMEOUT));

    let mut other = workspace.launch(&other_project);
    trust(&mut other);
    submit(&mut other);
    assert_full(&run_command(&mut other));
}

#[test]
fn previously_trusted_project_shows_full_output_without_asking() {
    let root = tempfile::tempdir().expect("create project parent");
    let project = std::fs::canonicalize(root.path()).expect("canonical path");
    let workspace = Workspace::new(&format!(
        "agent = \"mock-model\"\n\n[projects.{:?}]\ntrust_level = \"trusted\"\n",
        project.display().to_string()
    ));

    let mut session = workspace.launch(&project);
    let screen = run_command(&mut session);
    assert!(!screen.contains(COMPACT_QUESTION), "{screen}");
    assert_full(&screen);
}

#[test]
fn global_compact_setting_applies_without_asking_again() {
    let workspace = Workspace::new("agent = \"mock-model\"\n\n[tui]\ntool_output = \"compact\"\n");
    let project = workspace.project("alpha");

    let mut session = workspace.launch(&project);
    let form = open_trust_form(&mut session);
    assert!(!form.contains(COMPACT_QUESTION), "{form}");
    submit(&mut session);
    let screen = run_command(&mut session);
    assert_compact(&screen);
}

#[test]
fn choosing_full_keeps_full_after_a_one_off_override_ends() {
    let workspace = Workspace::new("agent = \"mock-model\"\n\n[tui]\ntool_output = \"compact\"\n");
    let project = workspace.project("alpha");

    let mut first = workspace.launch_with_override(&project, Some("tui.tool_output=full"));
    trust(&mut first);
    first.send_key(Key::Char('f')).expect("choose full");
    submit(&mut first);
    assert_full(&run_command(&mut first));
    assert!(first.terminate_and_wait(TIMEOUT));

    let mut relaunched = workspace.launch(&project);
    assert_full(&run_command(&mut relaunched));
}

#[test]
fn untouched_tool_output_keeps_inheriting_the_global_setting() {
    let workspace = Workspace::new("agent = \"mock-model\"\n");
    let project = workspace.project("alpha");

    let mut first = workspace.launch(&project);
    trust(&mut first);
    submit(&mut first);
    run_command(&mut first);
    assert!(first.terminate_and_wait(TIMEOUT));

    std::fs::write(
        workspace.home.join("config.toml"),
        format!("{}\n[tui]\ntool_output = \"compact\"\n", workspace.config()),
    )
    .expect("set global compact");
    let mut relaunched = workspace.launch(&project);
    assert_compact(&run_command(&mut relaunched));
}

#[test]
fn one_off_compact_override_still_asks_and_is_not_saved() {
    let workspace = Workspace::new("agent = \"mock-model\"\n");
    let project = workspace.project("alpha");

    let mut first = workspace.launch_with_override(&project, Some("tui.tool_output=compact"));
    trust(&mut first);
    submit(&mut first);
    assert_compact(&run_command(&mut first));
    assert!(first.terminate_and_wait(TIMEOUT));
    assert!(
        !workspace.config().contains("tool_output"),
        "{}",
        workspace.config()
    );

    let mut relaunched = workspace.launch(&project);
    assert_full(&run_command(&mut relaunched));
}

#[test]
fn unrelated_tui_override_keeps_the_project_compact_choice() {
    let workspace = Workspace::new("agent = \"mock-model\"\n");
    let project = workspace.project("alpha");

    let mut first = workspace.launch(&project);
    trust(&mut first);
    first.send_key(Key::Char('c')).expect("choose compact");
    submit(&mut first);
    run_command(&mut first);
    assert!(first.terminate_and_wait(TIMEOUT));

    let mut overridden =
        workspace.launch_with_override(&project, Some("tui={pinned_plan_drawer=false}"));
    assert_compact(&run_command(&mut overridden));
}

fn capped_project(cap: &str) -> (tempfile::TempDir, PathBuf, Workspace) {
    let root = tempfile::tempdir().expect("create project parent");
    let project = std::fs::canonicalize(root.path()).expect("canonical path");
    let workspace = Workspace::new(&format!(
        "agent = \"mock-model\"\n\n[tui]\ntool_output_max_lines = 40\n\n[projects.{:?}]\ntrust_level = \"trusted\"\n{cap}\n",
        project.display().to_string()
    ));
    (root, project, workspace)
}

#[test]
fn project_line_cap_keeps_head_and_tail_of_full_output() {
    let (_root, project, workspace) = capped_project("tool_output_max_lines = 3");

    let mut session = workspace.launch(&project);
    let screen = run_command(&mut session);
    assert!(
        screen.contains("running 3 tests")
            && screen.contains("┄ 3 lines hidden")
            && screen.contains(LAST_OUTPUT_LINE)
            && !screen.contains(EARLY_OUTPUT_LINE),
        "expected capped tool output:\n{screen}"
    );
}

#[test]
fn command_line_zero_lifts_a_project_line_cap() {
    let (_root, project, workspace) = capped_project("tool_output_max_lines = 3");

    let mut session = workspace.launch_with_override(&project, Some("tui.tool_output_max_lines=0"));
    let screen = run_command(&mut session);
    assert_full(&screen);
    assert!(!screen.contains("lines hidden"), "{screen}");
}
