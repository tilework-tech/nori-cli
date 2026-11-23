//! Behavior of the skillsets v2 client against a scripted v2 CLI and real Git
//! repositories.
#![cfg(unix)]
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use nori_skillsets::LayerChange;
use nori_skillsets::SkillsetEntry;
use nori_skillsets::SkillsetList;
use nori_skillsets::SkillsetsCli;
use nori_skillsets::SkillsetsError;
use pretty_assertions::assert_eq;
use serial_test::serial;
use tempfile::TempDir;

/// A stand-in for the v2 CLI: answers each subcommand with a canned JSON
/// envelope and exit code, and records every argv it receives.
struct FakeCli {
    dir: TempDir,
}

impl FakeCli {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("responses")).unwrap();
        write_executable(
            &dir.path().join("nori-skillsets"),
            r#"#!/bin/sh
d=$(dirname "$0")
printf '%s|' "$@" >> "$d/argv.log"; echo >> "$d/argv.log"
sub="$2"
if [ ! -f "$d/responses/$sub.json" ]; then
  echo "error: unknown command '$sub'" >&2
  exit 1
fi
cat "$d/responses/$sub.json"
exit "$(cat "$d/responses/$sub.code")"
"#,
        );
        Self { dir }
    }

    fn respond(&self, subcommand: &str, json: &str, exit: i32) {
        let responses = self.dir.path().join("responses");
        std::fs::write(responses.join(format!("{subcommand}.json")), json).unwrap();
        std::fs::write(
            responses.join(format!("{subcommand}.code")),
            exit.to_string(),
        )
        .unwrap();
    }

    fn bin_dir(&self) -> OsString {
        self.dir.path().as_os_str().to_owned()
    }

    fn cli(&self) -> SkillsetsCli {
        SkillsetsCli::resolve_from(None, Some(self.bin_dir())).unwrap()
    }

    /// Every argv the fake received, one `|`-terminated argument at a time.
    fn argv_log(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.path().join("argv.log"))
            .unwrap_or_default()
            .lines()
            .map(String::from)
            .collect()
    }
}

fn write_executable(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "init.defaultBranch=main", "-c", "tag.gpgSign=false"])
        .args(["-c", "commit.gpgSign=false"])
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// A Git repository with one commit and the given tags.
fn repo_with_tags(tags: &[&str]) -> TempDir {
    let repo = TempDir::new().unwrap();
    git(repo.path(), &["init", "--quiet"]);
    git(
        repo.path(),
        &["commit", "--quiet", "--allow-empty", "-m", "init"],
    );
    for tag in tags {
        match tag.strip_prefix("annotated:") {
            Some(tag) => git(repo.path(), &["tag", "-a", "-m", "release", tag]),
            None => git(repo.path(), &["tag", tag]),
        }
    }
    repo
}

fn status_json(repository: Option<&str>, layers: &[(&str, &str)]) -> String {
    let layers: Vec<serde_json::Value> = layers
        .iter()
        .map(|(name, version)| {
            serde_json::json!({"name": name, "version": version, "mode": "published"})
        })
        .collect();
    let mut data = serde_json::json!({
        "compiled": repository.is_some(),
        "layers": layers,
        "agents": ["claude"],
        "entries": [],
        "counts": {}
    });
    if let Some(repository) = repository {
        data["repository"] = serde_json::json!(repository);
    }
    serde_json::json!({"ok": true, "data": data}).to_string()
}

fn config_repository_json(repository: Option<&str>) -> String {
    let value: Vec<&str> = repository.into_iter().collect();
    serde_json::json!({
        "ok": true,
        "data": {"key": "repository", "value": value, "isSet": !value.is_empty()}
    })
    .to_string()
}

fn entry(name: &str, layered: bool, version: &str) -> SkillsetEntry {
    SkillsetEntry {
        name: name.to_string(),
        layered,
        version: Some(version.to_string()),
    }
}

#[test]
#[serial]
fn missing_cli_is_reported_as_not_installed() {
    let empty = TempDir::new().unwrap();
    let error =
        SkillsetsCli::resolve_from(None, Some(empty.path().as_os_str().to_owned())).unwrap_err();

    assert!(matches!(error, SkillsetsError::NotInstalled), "{error:?}");
    let message = error.to_string();
    assert!(message.contains("NORI_SKILLSETS_COMMAND"), "{message}");
}

#[test]
#[serial]
fn command_override_launches_through_a_runner() {
    let fake = FakeCli::new();
    fake.respond("status", &status_json(None, &[]), 0);
    let runner_dir = TempDir::new().unwrap();
    let runner = runner_dir.path().join("bun");
    write_executable(
        &runner,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" > \"{}/runner.log\"\nshift 2\nexec \"{}/nori-skillsets\" \"$@\"\n",
            runner_dir.path().display(),
            fake.dir.path().display()
        ),
    );
    let project = TempDir::new().unwrap();

    let cli = SkillsetsCli::resolve_from(
        Some(&format!("{} x nori-skillsets@0.2.0", runner.display())),
        Some(OsString::new()),
    )
    .unwrap();
    let layered = cli.layered(project.path()).unwrap();

    assert_eq!(layered, Vec::<String>::new());
    let runner_argv = std::fs::read_to_string(runner_dir.path().join("runner.log")).unwrap();
    assert_eq!(
        runner_argv.trim(),
        format!(
            "x nori-skillsets@0.2.0 -n status --project {} --json",
            project.path().display()
        )
    );
}

#[test]
#[serial]
fn empty_or_unparseable_command_override_is_rejected() {
    for raw in ["  ", "\"unterminated"] {
        let error = SkillsetsCli::resolve_from(Some(raw), None).unwrap_err();
        assert!(
            matches!(error, SkillsetsError::InvalidOverride),
            "{raw}: {error:?}"
        );
    }
}

#[test]
#[serial]
fn v1_cli_is_reported_as_not_v2() {
    // v1 has no `status` command, so the fake answers like commander does.
    let fake = FakeCli::new();
    let project = TempDir::new().unwrap();

    let error = fake.cli().layered(project.path()).unwrap_err();

    assert!(matches!(error, SkillsetsError::NotV2 { .. }), "{error:?}");
}

#[test]
#[serial]
fn non_json_output_is_reported_as_not_v2() {
    let fake = FakeCli::new();
    fake.respond("status", "Active skillset: personal/foo\n", 0);
    let project = TempDir::new().unwrap();

    let error = fake.cli().layered(project.path()).unwrap_err();

    assert!(matches!(error, SkillsetsError::NotV2 { .. }), "{error:?}");
}

#[test]
#[serial]
fn layered_returns_stack_order() {
    let fake = FakeCli::new();
    fake.respond(
        "status",
        &status_json(Some("/repo"), &[("team", "1.0.0"), ("personal", "2.0.0")]),
        0,
    );
    let project = TempDir::new().unwrap();

    let layered = fake.cli().layered(project.path()).unwrap();

    assert_eq!(layered, vec!["team".to_string(), "personal".to_string()]);
}

#[test]
#[serial]
fn list_merges_layered_and_published_skillsets() {
    let repo = repo_with_tags(&[
        "skillsets/alpha@1.0.0",
        "skillsets/alpha@1.10.0",
        "skillsets/alpha@2.0.0-rc.1",
        "skillsets/beta@0.1.0",
        "skillsets/beta@0.2.0",
        "skillsets/gamma@0.1.0-beta.1",
        "annotated:skillsets/delta@1.2.0",
        "skillsets/broken@latest",
        "skillsets/no-version",
        "skills/not-a-skillset@1.0.0",
    ]);
    let repository = repo.path().to_string_lossy().to_string();
    let fake = FakeCli::new();
    fake.respond(
        "status",
        &status_json(
            Some(&repository),
            &[("gamma", "0.1.0-beta.1"), ("beta", "0.1.0")],
        ),
        0,
    );
    let project = TempDir::new().unwrap();

    let list = fake.cli().list(project.path()).unwrap();

    assert_eq!(
        list,
        SkillsetList {
            project: project.path().to_path_buf(),
            repository: Some(repository),
            warning: None,
            skillsets: vec![
                entry("gamma", true, "0.1.0-beta.1"),
                entry("beta", true, "0.1.0"),
                entry("alpha", false, "1.10.0"),
                entry("delta", false, "1.2.0"),
            ],
        }
    );
    assert_eq!(
        fake.argv_log(),
        vec![format!(
            "-n|status|--project|{}|--json|",
            project.path().display()
        )]
    );
}

#[test]
#[serial]
fn list_reads_a_checkouts_remote_not_its_local_tags() {
    let upstream = repo_with_tags(&["skillsets/published@1.0.0"]);
    let parent = TempDir::new().unwrap();
    let checkout = parent.path().join("checkout");
    git(
        parent.path(),
        &[
            "clone",
            "--quiet",
            &upstream.path().to_string_lossy(),
            &checkout.to_string_lossy(),
        ],
    );
    git(&checkout, &["tag", "skillsets/unreleased@1.0.0"]);
    let repository = checkout.to_string_lossy().to_string();
    let fake = FakeCli::new();
    fake.respond("status", &status_json(Some(&repository), &[]), 0);
    let project = TempDir::new().unwrap();

    let list = fake.cli().list(project.path()).unwrap();

    assert_eq!(list.skillsets, vec![entry("published", false, "1.0.0")]);
}

#[test]
#[serial]
fn list_falls_back_to_the_machine_repository_for_new_projects() {
    let repo = repo_with_tags(&["skillsets/alpha@1.0.0"]);
    let repository = repo.path().to_string_lossy().to_string();
    let fake = FakeCli::new();
    fake.respond("status", &status_json(None, &[]), 0);
    fake.respond("config", &config_repository_json(Some(&repository)), 0);
    let project = TempDir::new().unwrap();

    let list = fake.cli().list(project.path()).unwrap();

    assert_eq!(
        list,
        SkillsetList {
            project: project.path().to_path_buf(),
            repository: Some(repository),
            warning: None,
            skillsets: vec![entry("alpha", false, "1.0.0")],
        }
    );
}

#[test]
#[serial]
fn list_without_a_repository_returns_no_skillsets() {
    let fake = FakeCli::new();
    fake.respond("status", &status_json(None, &[]), 0);
    fake.respond("config", &config_repository_json(None), 0);
    let project = TempDir::new().unwrap();

    let list = fake.cli().list(project.path()).unwrap();

    assert_eq!(
        list,
        SkillsetList {
            project: project.path().to_path_buf(),
            repository: None,
            warning: None,
            skillsets: vec![],
        }
    );
}

#[test]
#[serial]
fn list_keeps_layered_skillsets_when_releases_cannot_be_read() {
    let fake = FakeCli::new();
    fake.respond(
        "status",
        &status_json(Some("/nonexistent/repo"), &[("team", "1.0.0")]),
        0,
    );
    let project = TempDir::new().unwrap();

    let list = fake.cli().list(project.path()).unwrap();

    assert_eq!(list.skillsets, vec![entry("team", true, "1.0.0")]);
    let warning = list
        .warning
        .expect("a warning about the unreadable repository");
    assert!(warning.contains("/nonexistent/repo"), "{warning}");
}

#[test]
#[serial]
fn unexpected_response_shape_is_not_mistaken_for_v1() {
    let fake = FakeCli::new();
    fake.respond("status", r#"{"ok":true,"data":{"layers":"not-a-list"}}"#, 0);
    let project = TempDir::new().unwrap();

    let error = fake.cli().layered(project.path()).unwrap_err();

    assert!(
        matches!(error, SkillsetsError::UnexpectedResponse { .. }),
        "{error:?}"
    );
}

#[test]
#[serial]
fn layer_runs_v2_layer_and_reports_the_new_stack() {
    let fake = FakeCli::new();
    fake.respond(
        "layer",
        r#"{"ok":true,"data":{"layers":["team","personal"],"agents":["claude"],"written":["CLAUDE.md"],"unchanged":[],"removed":[],"kept":[],"refusals":[{"path":"AGENTS.md","message":"edited since last compile"}],"notes":["collision on skill x"]}}"#,
        0,
    );
    let project = TempDir::new().unwrap();

    let change = fake.cli().layer(project.path(), "personal").unwrap();

    assert_eq!(
        change,
        LayerChange {
            layers: vec!["team".to_string(), "personal".to_string()],
            notes: vec![
                "collision on skill x".to_string(),
                "refused AGENTS.md: edited since last compile".to_string(),
            ],
        }
    );
    assert_eq!(
        fake.argv_log(),
        vec![format!(
            "-n|layer|personal|--project|{}|--json|",
            project.path().display()
        )]
    );
}

#[test]
#[serial]
fn layer_failure_surfaces_the_v2_error() {
    let fake = FakeCli::new();
    fake.respond(
        "layer",
        r#"{"ok":false,"error":{"code":"SOURCE_INVALID","message":"No published release tags for skillsets/ghost","suggestions":[]}}"#,
        1,
    );
    let project = TempDir::new().unwrap();

    let error = fake.cli().layer(project.path(), "ghost").unwrap_err();

    let SkillsetsError::Command { code, message } = error else {
        panic!("expected a command error, got {error:?}");
    };
    assert_eq!(
        (code.as_str(), message.as_str()),
        (
            "SOURCE_INVALID",
            "No published release tags for skillsets/ghost"
        )
    );
}

#[test]
#[serial]
fn unlayer_runs_v2_unlayer() {
    let fake = FakeCli::new();
    fake.respond(
        "unlayer",
        r#"{"ok":true,"data":{"layers":[],"agents":["claude"],"written":[],"unchanged":[],"removed":["CLAUDE.md"],"kept":[],"refusals":[],"notes":[]}}"#,
        0,
    );
    let project = TempDir::new().unwrap();

    let change = fake.cli().unlayer(project.path(), "team").unwrap();

    assert_eq!(
        change,
        LayerChange {
            layers: vec![],
            notes: vec![],
        }
    );
    assert_eq!(
        fake.argv_log(),
        vec![format!(
            "-n|unlayer|team|--project|{}|--json|",
            project.path().display()
        )]
    );
}

#[test]
#[serial]
fn status_errors_surface_the_v2_error() {
    let fake = FakeCli::new();
    fake.respond(
        "status",
        r#"{"ok":false,"error":{"code":"MANIFEST_INVALID","message":"manifest is corrupt"}}"#,
        1,
    );
    let project = TempDir::new().unwrap();

    let error = fake.cli().list(project.path()).unwrap_err();

    let SkillsetsError::Command { code, message } = error else {
        panic!("expected a command error, got {error:?}");
    };
    assert_eq!(
        (code.as_str(), message.as_str()),
        ("MANIFEST_INVALID", "manifest is corrupt")
    );
}

#[test]
#[serial]
fn names_that_look_like_flags_are_rejected_without_running_the_cli() {
    let fake = FakeCli::new();
    let project = TempDir::new().unwrap();

    let error = fake.cli().layer(project.path(), "--all").unwrap_err();

    assert!(
        matches!(error, SkillsetsError::InvalidName { .. }),
        "{error:?}"
    );
    assert_eq!(fake.argv_log(), Vec::<String>::new());
}
