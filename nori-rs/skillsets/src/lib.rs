//! Client for the skillsets v2 CLI (`nori-skillsets`).
//!
//! Exposes the three operations Nori supports for skillsets v2: list, layer
//! and unlayer. Every call shells out to the v2 CLI with `--json` and parses
//! its `{"ok": ..., "data" | "error": ...}` envelope. Calls are blocking; async
//! callers should run them on a blocking thread.
//!
//! Skillsets v2 has no command that lists the skillsets a repository
//! publishes, so [`SkillsetsCli::list`] reads the repository's
//! `refs/tags/skillsets/<name>@<version>` release tags with `git ls-remote`.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use semver::Version;

use serde::Deserialize;
use serde::Serialize;

/// Environment variable that overrides how the v2 CLI is launched.
///
/// The value is a command line split with shell rules, so a launcher can
/// supply a runner and its arguments, e.g.
/// `/Applications/Nori.app/.../bun x nori-skillsets@0.2.0`.
pub const COMMAND_ENV: &str = "NORI_SKILLSETS_COMMAND";

/// Binary name looked up on `PATH` when [`COMMAND_ENV`] is unset.
pub const DEFAULT_PROGRAM: &str = "nori-skillsets";

/// A resolved way to launch the skillsets v2 CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsetsCli {
    program: PathBuf,
    prefix_args: Vec<String>,
}

/// One skillset as the picker shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsetEntry {
    pub name: String,
    /// Whether the skillset is layered into the project.
    pub layered: bool,
    /// Layered version, or the latest published version when not layered:
    /// the highest stable semantic version, else the highest prerelease (the
    /// release `layer` would pick).
    pub version: Option<String>,
}

/// The skillsets available to, or layered into, one project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsetList {
    pub project: PathBuf,
    /// Repository the project reads releases from; `None` when none is
    /// configured for the project or the machine.
    pub repository: Option<String>,
    /// Layered skillsets first, in stack order, then the rest by name.
    pub skillsets: Vec<SkillsetEntry>,
    /// Why published releases could not be read, in which case `skillsets`
    /// holds only the layered entries.
    pub warning: Option<String>,
}

/// Result of a successful `layer` or `unlayer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LayerChange {
    /// Layer names in stack order after the change.
    pub layers: Vec<String>,
    /// Human-readable notes, including paths the CLI refused to overwrite.
    pub notes: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SkillsetsError {
    #[error(
        "The skillsets v2 CLI (`{DEFAULT_PROGRAM}`) was not found on PATH. Install it with \
         `curl -fsSL https://cli.noriskillsets.dev/install.sh | sh`, or set {COMMAND_ENV} to the \
         command that runs it."
    )]
    NotInstalled,
    #[error("{COMMAND_ENV} is set but empty or not a valid command line")]
    InvalidOverride,
    #[error(
        "`{program}` is not the skillsets v2 CLI ({detail}). Nori needs skillsets v2; install it \
         with `curl -fsSL https://cli.noriskillsets.dev/install.sh | sh`, or set {COMMAND_ENV}."
    )]
    NotV2 { program: String, detail: String },
    #[error("`{name}` is not a valid skillset name")]
    InvalidName { name: String },
    #[error("{message}")]
    Command { code: String, message: String },
    #[error("`{program}` returned an unexpected response: {message}")]
    UnexpectedResponse { program: String, message: String },
    #[error("git could not list releases in {repository}: {message}")]
    Git { repository: String, message: String },
    #[error("failed to run `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
}

impl SkillsetsCli {
    /// Resolve the CLI from [`COMMAND_ENV`], then `PATH`.
    pub fn resolve() -> Result<Self, SkillsetsError> {
        Self::resolve_from(
            std::env::var(COMMAND_ENV).ok().as_deref(),
            std::env::var_os("PATH"),
        )
    }

    /// Resolve the CLI from an explicit override and search path.
    pub fn resolve_from(
        command_override: Option<&str>,
        search_path: Option<OsString>,
    ) -> Result<Self, SkillsetsError> {
        if let Some(raw) = command_override {
            let mut parts = shlex::split(raw)
                .filter(|parts| !parts.is_empty())
                .ok_or(SkillsetsError::InvalidOverride)?;
            let program = PathBuf::from(parts.remove(0));
            return Ok(Self {
                program,
                prefix_args: parts,
            });
        }
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        which::which_in(DEFAULT_PROGRAM, search_path, cwd)
            .map(|program| Self {
                program,
                prefix_args: Vec::new(),
            })
            .map_err(|_| SkillsetsError::NotInstalled)
    }

    /// List the skillsets layered into `project` plus those published in its
    /// repository.
    pub fn list(&self, project: &Path) -> Result<SkillsetList, SkillsetsError> {
        let status = self.status(project)?;
        let repository = match status.repository {
            Some(repository) => Some(repository),
            None => self.machine_repository()?,
        };
        let (mut published, warning) = match repository.as_deref().map(published_releases) {
            Some(Ok(published)) => (published, None),
            Some(Err(error)) => (BTreeMap::new(), Some(error.to_string())),
            None => (BTreeMap::new(), None),
        };

        let mut skillsets: Vec<SkillsetEntry> = status
            .layers
            .into_iter()
            .map(|layer| {
                published.remove(&layer.name);
                SkillsetEntry {
                    name: layer.name,
                    layered: true,
                    version: layer.version,
                }
            })
            .collect();
        skillsets.extend(published.into_iter().map(|(name, version)| SkillsetEntry {
            name,
            layered: false,
            version: Some(version.to_string()),
        }));

        Ok(SkillsetList {
            project: project.to_path_buf(),
            repository,
            skillsets,
            warning,
        })
    }

    /// Names of the skillsets layered into `project`, in stack order. Never
    /// touches the network.
    pub fn layered(&self, project: &Path) -> Result<Vec<String>, SkillsetsError> {
        Ok(self
            .status(project)?
            .layers
            .into_iter()
            .map(|layer| layer.name)
            .collect())
    }

    /// Add `name` to the end of `project`'s layer stack and recompile.
    pub fn layer(&self, project: &Path, name: &str) -> Result<LayerChange, SkillsetsError> {
        self.change("layer", project, name)
    }

    /// Remove `name` from `project`'s layer stack and recompile.
    pub fn unlayer(&self, project: &Path, name: &str) -> Result<LayerChange, SkillsetsError> {
        self.change("unlayer", project, name)
    }

    fn change(
        &self,
        verb: &str,
        project: &Path,
        name: &str,
    ) -> Result<LayerChange, SkillsetsError> {
        if name.is_empty() || name.starts_with('-') {
            return Err(SkillsetsError::InvalidName {
                name: name.to_string(),
            });
        }
        let data: ChangeData = self.run(&[
            verb.as_ref(),
            name.as_ref(),
            "--project".as_ref(),
            project.as_os_str(),
        ])?;
        let mut notes = data.notes;
        notes.extend(
            data.refusals
                .into_iter()
                .map(|refusal| format!("refused {}: {}", refusal.path, refusal.message)),
        );
        notes.extend(
            data.kept
                .into_iter()
                .map(|kept| format!("kept {}: {}", kept.path, kept.message)),
        );
        Ok(LayerChange {
            layers: data.layers,
            notes,
        })
    }

    fn status(&self, project: &Path) -> Result<StatusData, SkillsetsError> {
        self.run(&["status".as_ref(), "--project".as_ref(), project.as_os_str()])
    }

    fn machine_repository(&self) -> Result<Option<String>, SkillsetsError> {
        let data: ConfigValue =
            self.run(&["config".as_ref(), "get".as_ref(), "repository".as_ref()])?;
        Ok(data.value.into_iter().next())
    }

    /// Run `<cli> -n <args> --json` and decode the v2 JSON envelope.
    fn run<T: serde::de::DeserializeOwned>(&self, args: &[&OsStr]) -> Result<T, SkillsetsError> {
        let program = self.program.display().to_string();
        let output = Command::new(&self.program)
            .args(&self.prefix_args)
            .arg("-n")
            .args(args)
            .arg("--json")
            .envs(non_interactive_git_env())
            .stdin(Stdio::null())
            .output()
            .map_err(|source| SkillsetsError::Spawn {
                program: program.clone(),
                source,
            })?;
        let not_v2 = |detail: String| SkillsetsError::NotV2 {
            program: program.clone(),
            detail,
        };
        let envelope: Envelope<serde_json::Value> = serde_json::from_slice(&output.stdout)
            .map_err(|_| {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let stdout = String::from_utf8_lossy(&output.stdout);
                let first_line = stderr
                    .lines()
                    .chain(stdout.lines())
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .map(String::from);
                not_v2(first_line.unwrap_or_else(|| format!("exit status {}", output.status)))
            })?;
        match envelope {
            Envelope {
                data: Some(data),
                error: None,
                ok: true,
            } => serde_json::from_value(data).map_err(|error| SkillsetsError::UnexpectedResponse {
                program: program.clone(),
                message: error.to_string(),
            }),
            Envelope {
                error: Some(error), ..
            } if error.code == "UNKNOWN_COMMAND" => Err(not_v2(error.message)),
            Envelope {
                error: Some(error), ..
            } => Err(SkillsetsError::Command {
                code: error.code,
                message: error.message,
            }),
            Envelope { .. } => Err(not_v2("unexpected JSON response".to_string())),
        }
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    data: Option<T>,
    error: Option<ErrorBody>,
}

#[derive(Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

#[derive(Deserialize)]
struct StatusData {
    #[serde(default)]
    layers: Vec<StatusLayer>,
    repository: Option<String>,
}

#[derive(Deserialize)]
struct StatusLayer {
    name: String,
    version: Option<String>,
}

#[derive(Deserialize)]
struct ConfigValue {
    #[serde(default)]
    value: Vec<String>,
}

#[derive(Deserialize)]
struct ChangeData {
    #[serde(default)]
    layers: Vec<String>,
    #[serde(default)]
    notes: Vec<String>,
    #[serde(default)]
    refusals: Vec<PathMessage>,
    #[serde(default)]
    kept: Vec<PathMessage>,
}

#[derive(Deserialize)]
struct PathMessage {
    path: String,
    message: String,
}

const RELEASE_TAG_PREFIX: &str = "refs/tags/skillsets/";

/// Latest release of each skillset published in `repository`.
///
/// Mirrors how v2 picks a remote: a Git URL as given, or for a local checkout
/// its `origin` remote, else its first remote, else the checkout itself. A
/// checkout's remote is queried from inside the checkout so its own
/// credential and transport settings apply. The latest version is the highest
/// stable semantic version, else the highest prerelease.
fn published_releases(repository: &str) -> Result<BTreeMap<String, Version>, SkillsetsError> {
    let git_error = |message: String| SkillsetsError::Git {
        repository: repository.to_string(),
        message,
    };
    let checkout = Path::new(repository);
    let (dir, remote) = if checkout.is_dir() {
        let remotes = git_output(Some(checkout), &["remote"]).map_err(git_error)?;
        let remote = remotes
            .lines()
            .find(|name| *name == "origin")
            .or_else(|| remotes.lines().next())
            .unwrap_or(".")
            .to_string();
        (Some(checkout), remote)
    } else {
        (None, repository.to_string())
    };
    let pattern = format!("{RELEASE_TAG_PREFIX}*");
    let listed = git_output(
        dir,
        &["ls-remote", "--tags", "--refs", "--", &remote, &pattern],
    )
    .map_err(git_error)?;

    let mut stable: BTreeMap<String, Version> = BTreeMap::new();
    let mut prerelease: BTreeMap<String, Version> = BTreeMap::new();
    for reference in listed.lines().filter_map(|line| line.split('\t').nth(1)) {
        let Some((name, version)) = reference
            .strip_prefix(RELEASE_TAG_PREFIX)
            .and_then(|tag| tag.rsplit_once('@'))
        else {
            continue;
        };
        let Ok(version) = Version::parse(version) else {
            continue;
        };
        let latest = if version.pre.is_empty() {
            &mut stable
        } else {
            &mut prerelease
        };
        match latest.get(name) {
            Some(current) if *current >= version => {}
            Some(_) | None => {
                latest.insert(name.to_string(), version);
            }
        }
    }
    for (name, version) in prerelease {
        stable.entry(name).or_insert(version);
    }
    Ok(stable)
}

fn git_output(dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    if let Some(dir) = dir {
        command.arg("-C").arg(dir);
    }
    let output = command
        .args(args)
        .envs(non_interactive_git_env())
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Environment that stops Git and SSH from prompting on the terminal the TUI
/// owns: credential prompts are disabled, and SSH runs in batch mode unless the
/// user already chose an SSH command.
fn non_interactive_git_env() -> Vec<(&'static str, OsString)> {
    let mut env = vec![("GIT_TERMINAL_PROMPT", OsString::from("0"))];
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        env.push(("GIT_SSH_COMMAND", OsString::from("ssh -o BatchMode=yes")));
    }
    env
}
