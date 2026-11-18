//! Privileged managed launch admission for a single host incarnation.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Context;
use anyhow::ensure;
use nori_acp_host::connection::AgentProcess;
use nori_acp_host::connection::ChildHandle;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use tokio::process::ChildStdin;
use tokio::process::ChildStdout;
use tokio::process::Command;

use crate::ConversationId;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnsureLaunch {
    command_id: String,
    conversation_id: ConversationId,
    binding_id: String,
    generation: i64,
    spec: LaunchSpec,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchSpec {
    pub project_id: String,
    pub run_id: String,
    pub agent: String,
    pub model: Option<String>,
    pub adapter: PinnedAdapter,
    pub cwd: PathBuf,
    pub home: PathBuf,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub credential_refs: Vec<String>,
    #[serde(default)]
    pub resume_artifacts: Vec<String>,
    pub additions: HostAdditions,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PinnedAdapter {
    pub path: PathBuf,
    pub version: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HostAdditions {
    pub prompt_echo: bool,
    pub status_events: bool,
    pub recording: bool,
    pub client_mcp: bool,
}

pub fn session_folder(path: &Path) -> anyhow::Result<PathBuf> {
    let requested = path.display();
    ensure!(
        path.is_absolute(),
        "session folder must be an absolute path: {requested}"
    );
    let folder = dunce::canonicalize(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => {
            anyhow::anyhow!("session folder does not exist on the host: {requested}")
        }
        _ => anyhow::anyhow!("cannot open session folder {requested}: {error}"),
    })?;
    ensure!(
        folder.is_dir(),
        "session folder is not a directory: {requested}"
    );
    Ok(folder)
}

impl LaunchSpec {
    fn command(&self, root: &Path, binding: &str) -> anyhow::Result<Command> {
        ensure!(
            self.additions == HostAdditions::default(),
            "requested host addition is unsupported"
        );
        ensure!(
            [
                &self.agent,
                &self.project_id,
                &self.run_id,
                &self.adapter.version
            ]
            .iter()
            .all(|s| !s.trim().is_empty()),
            "missing launch identity or pin"
        );
        ensure!(
            self.adapter.path.is_absolute() && self.adapter.path.is_file(),
            "adapter path must name an installed absolute executable"
        );
        let binding_root = root.join(binding);
        ensure!(
            session_folder(&binding_root)? == binding_root,
            "binding root must not be redirected"
        );
        let cwd = session_folder(&self.cwd)?;
        let home = session_folder(&self.home)?;
        ensure!(
            cwd != home
                && [&cwd, &home]
                    .iter()
                    .all(|p| p.starts_with(&binding_root) && **p != binding_root),
            "cwd and home must be distinct directories inside the binding root"
        );
        for key in self.env.keys() {
            ensure!(
                !key.is_empty()
                    && !key.contains(['=', '\0'])
                    && !["HOME", "USERPROFILE", "CODEX_HOME", "NORI_HOME"]
                        .iter()
                        .any(|reserved| key.eq_ignore_ascii_case(reserved)),
                "invalid or reserved environment key"
            );
        }
        let mut command = Command::new(&self.adapter.path);
        command
            .args(&self.adapter.args)
            .env_clear()
            .envs(&self.env)
            .env("HOME", &home)
            .env("CODEX_HOME", home.join(".codex"))
            .env("NORI_HOME", home.join(".nori/cli"))
            .current_dir(cwd)
            .stderr(Stdio::null());
        #[cfg(windows)]
        command.env("USERPROFILE", &home);
        Ok(command)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchReceipt {
    binding_id: String,
    generation: i64,
    conversation_id: ConversationId,
    spec_digest: String,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
}

pub struct LaunchConnection {
    pub conversation_id: ConversationId,
    pub spec: Arc<LaunchSpec>,
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
}

struct Launch {
    conversation_id: ConversationId,
    generation: i64,
    spec: Arc<LaunchSpec>,
    digest: String,
    process: Result<ChildHandle, String>,
    pipes: Option<(ChildStdin, ChildStdout)>,
}

impl Launch {
    fn receipt(&self, binding: &str, status: bool) -> LaunchReceipt {
        let (state, exit_code) = match &self.process {
            Err(_) => ("spawnFailed", None),
            Ok(process) if status && process.has_exited() => ("exited", process.exit_code()),
            Ok(_) => ("spawned", None),
        };
        LaunchReceipt {
            binding_id: binding.into(),
            generation: self.generation,
            conversation_id: self.conversation_id,
            spec_digest: self.digest.clone(),
            state,
            exit_code,
        }
    }

    fn acknowledge(&self, binding: &str) -> anyhow::Result<LaunchReceipt> {
        if let Err(error) = &self.process {
            anyhow::bail!("adapter spawn failed: {error}");
        }
        Ok(self.receipt(binding, false))
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedHostCapabilities {
    version: i32,
    max_sessions: usize,
}

pub struct ManagedLaunches {
    root: PathBuf,
    limit: usize,
    launches: Mutex<Option<HashMap<String, Launch>>>,
}

impl ManagedLaunches {
    pub fn new(root: &Path, max_sessions: i32) -> anyhow::Result<Self> {
        ensure!(max_sessions > 0, "host.max_sessions must be positive");
        Ok(Self {
            root: session_folder(root)?,
            limit: max_sessions.try_into()?,
            launches: Mutex::new(Some(HashMap::new())),
        })
    }

    pub fn capabilities(&self) -> ManagedHostCapabilities {
        ManagedHostCapabilities {
            version: 1,
            max_sessions: self.limit,
        }
    }

    pub fn ensure(&self, request: EnsureLaunch) -> anyhow::Result<LaunchReceipt> {
        let EnsureLaunch {
            binding_id,
            generation,
            conversation_id,
            command_id,
            spec,
        } = request;
        ensure!(
            !command_id.trim().is_empty() && generation > 0,
            "commandId and a positive generation are required"
        );
        ensure!(
            !binding_id.is_empty()
                && binding_id != "."
                && binding_id != ".."
                && binding_id
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.')),
            "invalid bindingId"
        );
        let mut guard = self.launches.lock().expect("launch registry poisoned");
        let launches = guard.as_mut().context("host is stopping")?;
        if let Some(existing) = launches.get(&binding_id) {
            ensure!(
                existing.conversation_id == conversation_id
                    && existing.generation == generation
                    && *existing.spec == spec,
                "managed launch conflict"
            );
            return existing.acknowledge(&binding_id);
        }
        ensure!(
            !launches
                .values()
                .any(|l| l.conversation_id == conversation_id),
            "managed conversation ID conflict"
        );
        let command = spec.command(&self.root, &binding_id)?;
        ensure!(
            launches
                .values()
                .filter(|l| l.process.as_ref().is_ok_and(|p| !p.has_exited()))
                .count()
                < self.limit,
            "managed host at capacity"
        );
        let digest = hex::encode(Sha256::digest(serde_json::to_vec(&spec)?));
        let (process, pipes) = match AgentProcess::spawn(command) {
            Ok(child) => (Ok(child.handle), Some((child.stdin, child.stdout))),
            Err(error) => (Err(error.to_string()), None),
        };
        launches
            .entry(binding_id.clone())
            .or_insert(Launch {
                conversation_id,
                generation,
                spec: Arc::new(spec),
                digest,
                process,
                pipes,
            })
            .acknowledge(&binding_id)
    }

    pub fn status(&self, binding: &str, generation: i64) -> anyhow::Result<LaunchReceipt> {
        let guard = self.launches.lock().expect("launch registry poisoned");
        let launch = guard
            .as_ref()
            .and_then(|l| l.get(binding))
            .filter(|l| l.generation == generation)
            .context("unknown binding or generation")?;
        Ok(launch.receipt(binding, true))
    }

    pub fn take_connection(
        &self,
        binding: &str,
        generation: i64,
    ) -> anyhow::Result<LaunchConnection> {
        let mut guard = self.launches.lock().expect("launch registry poisoned");
        let launch = guard
            .as_mut()
            .and_then(|l| l.get_mut(binding))
            .filter(|l| l.generation == generation)
            .context("unknown binding or generation")?;
        let (stdin, stdout) = launch
            .pipes
            .take()
            .context("no available adapter connection")?;
        Ok(LaunchConnection {
            conversation_id: launch.conversation_id,
            spec: launch.spec.clone(),
            stdin,
            stdout,
        })
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        let launches = self
            .launches
            .lock()
            .expect("launch registry poisoned")
            .take()
            .unwrap_or_default();
        let stopping = launches
            .into_values()
            .filter_map(|launch| launch.process.ok())
            .map(ChildHandle::stop);
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            futures::future::join_all(stopping),
        )
        .await
        .context("managed adapters did not stop")?;
        Ok(())
    }
}
