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

#[cfg(target_os = "linux")]
mod codex_home;
#[cfg(target_os = "linux")]
mod namespace_exit;
mod side_sandbox;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnsureLaunch {
    command_id: String,
    conversation_id: ConversationId,
    binding_id: String,
    generation: i64,
    spec: LaunchSpec,
}

impl EnsureLaunch {
    pub fn validate(&self) -> anyhow::Result<()> {
        ensure!(
            !self.command_id.trim().is_empty() && self.generation > 0,
            LaunchRefusal::InvalidParams(anyhow::anyhow!(
                "commandId and a positive generation are required"
            ))
        );
        validate_binding_id(&self.binding_id).map_err(LaunchRefusal::InvalidParams)?;
        Ok(())
    }
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub workspace_read_only: bool,
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

#[derive(Debug, thiserror::Error)]
pub enum LaunchRefusal {
    #[error("{0}")]
    InvalidParams(#[source] anyhow::Error),
    #[error("managed host at capacity")]
    AtCapacity,
}

fn validate_binding_id(id: &str) -> anyhow::Result<()> {
    ensure!(
        !id.is_empty()
            && id != "."
            && id != ".."
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.')),
        "invalid bindingId"
    );
    Ok(())
}

fn validate_roots(cwd: &Path, home: &Path) -> anyhow::Result<()> {
    ensure!(cwd != home, "cwd and home must be distinct");
    for name in [".codex", ".nori", ".nori/cli"] {
        let path = home.join(name);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => ensure!(
                meta.is_dir() && session_folder(&path)?.starts_with(home),
                "configuration directories must stay inside the binding home"
            ),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

#[derive(Clone)]
enum Layout {
    Shared(PathBuf),
    Dedicated(nori_config::DedicatedRoots),
}

impl Layout {
    fn validate(&self, binding: &str, cwd: &Path, home: &Path) -> anyhow::Result<()> {
        match self {
            Self::Dedicated(roots) => ensure!(
                binding == roots.binding_id && cwd == roots.cwd && home == roots.home,
                "binding does not match dedicated roots"
            ),
            Self::Shared(root) => {
                let scope = root.join(binding);
                ensure!(
                    session_folder(&scope)? == scope,
                    "binding root must not be redirected"
                );
                ensure!(
                    [cwd, home]
                        .iter()
                        .all(|p| p.starts_with(&scope) && *p != scope),
                    "cwd and home must be inside the binding root"
                );
            }
        }
        validate_roots(cwd, home)
    }
}

impl LaunchSpec {
    pub(crate) fn uses_private_codex_home(&self) -> bool {
        use nori_acp_host::registry::AgentKind;
        AgentKind::from_slug(&self.agent) == Some(AgentKind::Codex)
            && self.adapter.version == "2.7.0"
    }

    pub(crate) fn has_subscription_file(&self) -> bool {
        self.credential_refs == ["sessions:codex-subscription-file:v1"]
    }

    fn command(
        &self,
        cwd: &Path,
        home: &Path,
        side_fds: Option<[std::fs::File; 2]>,
        sandbox: &side_sandbox::Sandbox,
    ) -> anyhow::Result<Command> {
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
        ensure!(
            self.adapter
                .args
                .iter()
                .chain(self.env.values())
                .all(|s| !s.contains('\0')),
            "adapter arguments and environment values cannot contain NUL"
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
        if side_fds.is_some() || self.workspace_read_only {
            return side_sandbox::command(self, cwd, home, side_fds, &sandbox.launcher);
        }
        let mut command = Command::new(&self.adapter.path);
        command
            .args(&self.adapter.args)
            .env_clear()
            .envs(&self.env)
            .env("HOME", home)
            .env("CODEX_HOME", home.join(".codex"))
            .env("NORI_HOME", home.join(".nori/cli"))
            .current_dir(cwd)
            .stderr(Stdio::null());
        #[cfg(windows)]
        command.env("USERPROFILE", home);
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
    pub cwd: PathBuf,
    pub home: PathBuf,
    pub conversation_id: ConversationId,
    pub spec: Arc<LaunchSpec>,
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
}

struct Launch {
    conversation_id: ConversationId,
    generation: i64,
    digest: String,
    process: Result<Arc<ChildHandle>, String>,
    spec: Arc<LaunchSpec>,
    cwd: PathBuf,
    home: PathBuf,
    pipes: Option<(ChildStdin, ChildStdout)>,
    sides: HashMap<String, Arc<SideProcess>>,
    releasing: bool,
}

pub(crate) struct SideProcess {
    supervisor: Arc<ChildHandle>,
    /// Whether the OS read-only launch holds this process.
    pub(crate) confined: bool,
    drained: tokio::sync::watch::Receiver<Option<Result<(), String>>>,
}

impl SideProcess {
    fn has_exited(&self) -> bool {
        matches!(*self.drained.borrow(), Some(Ok(())))
    }

    pub(crate) async fn stop(&self) -> anyhow::Result<()> {
        self.supervisor.request_kill();
        let mut drained = self.drained.clone();
        let done = drained
            .wait_for(Option::is_some)
            .await
            .context("side exit monitor disconnected")?;
        if let Some(Err(error)) = &*done {
            anyhow::bail!("side process exit unconfirmed: {error}");
        }
        Ok(())
    }
}

impl Launch {
    fn connection(&self, stdin: ChildStdin, stdout: ChildStdout) -> LaunchConnection {
        LaunchConnection {
            cwd: self.cwd.clone(),
            home: self.home.clone(),
            conversation_id: self.conversation_id,
            spec: self.spec.clone(),
            stdin,
            stdout,
        }
    }

    fn stop(&mut self) -> impl Future<Output = anyhow::Result<()>> + use<> {
        self.releasing = true;
        self.pipes = None;
        let parent = self.process.as_ref().ok().cloned();
        let sides: Vec<_> = self.sides.values().cloned().collect();
        async move {
            let parent = async {
                if let Some(parent) = parent {
                    parent.request_kill();
                    parent.wait_for_exit().await;
                }
            };
            let sides = futures::future::join_all(sides.iter().map(|side| side.stop()));
            for result in tokio::join!(parent, sides).1 {
                result?;
            }
            Ok(())
        }
    }

    fn live_slots(&mut self) -> usize {
        let parent_live = self.process.as_ref().is_ok_and(|p| !p.has_exited());
        if !parent_live {
            self.pipes = None;
        }
        self.sides.retain(|_, child| !child.has_exited());
        usize::from(parent_live) + self.sides.len()
    }

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
    standard_acp: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    rebuilt_side_sessions: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fork_side_sessions: Option<i32>,
}

#[derive(Clone)]
pub struct ManagedLaunches {
    layout: Layout,
    limit: usize,
    side_sessions: bool,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    side_confinement: nori_config::SideConfinement,
    sandbox: Arc<side_sandbox::Sandbox>,
    launches: Arc<Mutex<Option<HashMap<String, Launch>>>>,
}

enum PreparedLaunch {
    Existing(LaunchReceipt),
    Spawn {
        command: Box<Command>,
        digest: String,
        cwd: PathBuf,
        home: PathBuf,
    },
}

impl ManagedLaunches {
    pub fn new(root: &Path, max_sessions: i32) -> anyhow::Result<Self> {
        ensure!(max_sessions > 0, "host.max_sessions must be positive");
        Ok(Self {
            layout: Layout::Shared(session_folder(root)?),
            limit: max_sessions.try_into()?,
            side_sessions: false,
            side_confinement: nori_config::SideConfinement::default(),
            sandbox: Arc::new(side_sandbox::Sandbox::locate()),
            launches: Arc::new(Mutex::new(Some(HashMap::new()))),
        })
    }

    pub fn from_config(config: &nori_config::HostConfig, nori_home: &Path) -> anyhow::Result<Self> {
        if let Some(roots) = &config.dedicated_roots {
            ensure!(
                config.bindings_root.is_none(),
                "dedicated roots cannot be combined with bindings_root"
            );
            validate_binding_id(&roots.binding_id)?;
            let cwd = session_folder(&roots.cwd)?;
            let home = session_folder(&roots.home)?;
            validate_roots(&cwd, &home)?;
            ensure!(
                config.max_sessions > 0,
                "host.max_sessions must be positive"
            );
            return Ok(Self {
                layout: Layout::Dedicated(nori_config::DedicatedRoots {
                    binding_id: roots.binding_id.clone(),
                    cwd,
                    home,
                }),
                limit: config.max_sessions.try_into()?,
                side_sessions: config.side_sessions,
                side_confinement: config.side_confinement,
                sandbox: Arc::new(side_sandbox::Sandbox::locate()),
                launches: Arc::new(Mutex::new(Some(HashMap::new()))),
            });
        }
        let root = config
            .bindings_root
            .clone()
            .unwrap_or_else(|| nori_home.join("hosted-sessions"));
        let mut launches = Self::new(&root, config.max_sessions)?;
        launches.side_sessions = config.side_sessions;
        launches.side_confinement = config.side_confinement;
        Ok(launches)
    }

    pub fn capabilities(&self) -> ManagedHostCapabilities {
        ManagedHostCapabilities {
            version: 1,
            max_sessions: self.limit,
            standard_acp: 1,
            rebuilt_side_sessions: self.side_sessions.then_some(1),
            fork_side_sessions: self.side_sessions.then_some(1),
        }
    }

    pub fn is_resident(&self, binding: &str) -> anyhow::Result<bool> {
        let guard = self
            .launches
            .lock()
            .map_err(|_| anyhow::anyhow!("launch registry poisoned"))?;
        Ok(guard
            .as_ref()
            .context("host is stopping")?
            .contains_key(binding))
    }

    pub fn validate_destination(&self, request: &EnsureLaunch) -> anyhow::Result<()> {
        let mut guard = self
            .launches
            .lock()
            .map_err(|_| anyhow::anyhow!("launch registry poisoned"))?;
        let launches = guard.as_mut().context("host is stopping")?;
        self.prepare(launches, request).map(|_| ())
    }

    fn prepare(
        &self,
        launches: &mut HashMap<String, Launch>,
        request: &EnsureLaunch,
    ) -> anyhow::Result<PreparedLaunch> {
        request.validate()?;
        let digest = hex::encode(Sha256::digest(serde_json::to_vec(&request.spec)?));
        if let Some(existing) = launches.get(&request.binding_id) {
            ensure!(
                existing.conversation_id == request.conversation_id
                    && existing.generation == request.generation
                    && existing.digest == digest,
                "managed launch conflict"
            );
            ensure!(!existing.releasing, "binding is releasing");
            return Ok(PreparedLaunch::Existing(
                existing.acknowledge(&request.binding_id)?,
            ));
        }
        ensure!(
            !launches
                .values()
                .any(|l| l.conversation_id == request.conversation_id),
            "managed conversation ID conflict"
        );
        let cwd = session_folder(&request.spec.cwd).map_err(LaunchRefusal::InvalidParams)?;
        let home = session_folder(&request.spec.home).map_err(LaunchRefusal::InvalidParams)?;
        self.layout
            .validate(&request.binding_id, &cwd, &home)
            .map_err(LaunchRefusal::InvalidParams)?;
        let command = request
            .spec
            .command(&cwd, &home, None, &self.sandbox)
            .map_err(LaunchRefusal::InvalidParams)?;
        let live: usize = launches.values_mut().map(Launch::live_slots).sum();
        ensure!(live < self.limit, LaunchRefusal::AtCapacity);
        Ok(PreparedLaunch::Spawn {
            command: Box::new(command),
            digest,
            cwd,
            home,
        })
    }

    pub fn ensure(&self, request: EnsureLaunch) -> anyhow::Result<LaunchReceipt> {
        let mut guard = self
            .launches
            .lock()
            .map_err(|_| anyhow::anyhow!("launch registry poisoned"))?;
        let launches = guard.as_mut().context("host is stopping")?;
        let (command, digest, cwd, home) = match self.prepare(launches, &request)? {
            PreparedLaunch::Existing(receipt) => return Ok(receipt),
            PreparedLaunch::Spawn {
                command,
                digest,
                cwd,
                home,
            } => (*command, digest, cwd, home),
        };
        let EnsureLaunch {
            binding_id,
            generation,
            conversation_id,
            spec,
            ..
        } = request;
        let (process, pipes) = match AgentProcess::spawn(command) {
            Ok(child) => (
                Ok(Arc::new(child.handle)),
                Some((child.stdin, child.stdout)),
            ),
            Err(error) => (Err(error.to_string()), None),
        };
        launches
            .entry(binding_id.clone())
            .or_insert(Launch {
                conversation_id,
                generation,
                digest,
                process,
                cwd,
                home,
                spec: Arc::new(spec),
                pipes,
                sides: HashMap::new(),
                releasing: false,
            })
            .acknowledge(&binding_id)
    }

    pub fn status(&self, binding: &str, generation: i64) -> anyhow::Result<LaunchReceipt> {
        let guard = self
            .launches
            .lock()
            .map_err(|_| anyhow::anyhow!("launch registry poisoned"))?;
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
        let mut guard = self
            .launches
            .lock()
            .map_err(|_| anyhow::anyhow!("launch registry poisoned"))?;
        let launch = guard
            .as_mut()
            .and_then(|l| l.get_mut(binding))
            .filter(|l| l.generation == generation)
            .context("unknown binding or generation")?;
        ensure!(
            launch.process.as_ref().is_ok_and(|p| !p.has_exited()),
            "adapter is not running"
        );
        let (stdin, stdout) = launch
            .pipes
            .take()
            .context("no available adapter connection")?;
        Ok(launch.connection(stdin, stdout))
    }

    pub fn side_sessions_enabled(&self) -> bool {
        self.side_sessions
    }

    pub fn spawn_side(
        &self,
        binding: &str,
        generation: i64,
        side_id: &str,
    ) -> anyhow::Result<LaunchConnection> {
        self.spawn_owned_side(binding, generation, side_id)
            .map(|(connection, _)| connection)
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn spawn_owned_side(
        &self,
        _: &str,
        _: i64,
        _: &str,
    ) -> anyhow::Result<(LaunchConnection, Arc<SideProcess>)> {
        anyhow::bail!("read-only workspace enforcement unavailable on this platform")
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn spawn_owned_side(
        &self,
        binding: &str,
        generation: i64,
        side_id: &str,
    ) -> anyhow::Result<(LaunchConnection, Arc<SideProcess>)> {
        ensure!(self.side_sessions, "side sessions disabled");
        let mut guard = self
            .launches
            .lock()
            .map_err(|_| anyhow::anyhow!("launch registry poisoned"))?;
        let launches = guard.as_mut().context("host is stopping")?;
        let live: usize = launches.values_mut().map(Launch::live_slots).sum();
        let launch = launches
            .get_mut(binding)
            .filter(|l| l.generation == generation)
            .context("unknown binding or generation")?;
        ensure!(!launch.releasing, "binding is releasing");
        let parent_exit = launch
            .process
            .as_ref()
            .ok()
            .filter(|p| !p.has_exited())
            .context("parent adapter is not running")?
            .wait_for_exit();
        ensure!(
            !launch.sides.contains_key(side_id),
            "side ID already in use"
        );
        ensure!(live < self.limit, LaunchRefusal::AtCapacity);
        self.layout.validate(binding, &launch.cwd, &launch.home)?;
        ensure!(
            session_folder(&launch.cwd)? == launch.cwd
                && session_folder(&launch.home)? == launch.home,
            "pinned launch roots changed"
        );
        let confined =
            self.side_confinement == nori_config::SideConfinement::Required || self.sandbox.usable;
        let (namespace, command) = if confined {
            let (namespace, fds) = namespace_exit::NamespaceExit::prepare()?;
            let command =
                launch
                    .spec
                    .command(&launch.cwd, &launch.home, Some(fds), &self.sandbox)?;
            (Some(namespace), command)
        } else {
            // This machine cannot create the read-only launch. The side runs as
            // an ordinary adapter process and the caller learns that it did.
            let command = launch
                .spec
                .command(&launch.cwd, &launch.home, None, &self.sandbox)?;
            (None, command)
        };
        let child = AgentProcess::spawn(command).context("side adapter launch failed")?;
        let supervisor = Arc::new(child.handle);
        let outer_exit = supervisor.wait_for_exit();
        let side = Arc::downgrade(&supervisor);
        let (done, drained) = tokio::sync::watch::channel(None);
        let process = Arc::new(SideProcess {
            supervisor,
            confined,
            drained,
        });
        launch.sides.insert(side_id.to_owned(), process.clone());
        tokio::spawn(async move {
            let drain = async {
                match namespace {
                    Some(namespace) => namespace.monitor(side.clone(), outer_exit).await,
                    None => {
                        outer_exit.await;
                        Ok(())
                    }
                }
            };
            tokio::pin!(drain);
            let result = tokio::select! {
                () = parent_exit => {
                    if let Some(side) = side.upgrade() {
                        side.request_kill();
                    }
                    drain.await
                }
                result = &mut drain => result,
            };
            done.send_replace(Some(result.map_err(|error| format!("{error:#}"))));
        });
        Ok((launch.connection(child.stdin, child.stdout), process))
    }

    pub async fn stop_side(
        &self,
        binding: &str,
        generation: i64,
        side_id: &str,
    ) -> anyhow::Result<()> {
        let handle = self
            .launches
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(|launches| launches.get(binding))
            .filter(|launch| launch.generation == generation)
            .and_then(|launch| launch.sides.get(side_id))
            .cloned();
        if let Some(handle) = handle {
            handle.stop().await?;
        }
        Ok(())
    }

    pub async fn release_one(&self, binding: &str, generation: i64) -> anyhow::Result<()> {
        let stopping = {
            let mut guard = self
                .launches
                .lock()
                .map_err(|_| anyhow::anyhow!("launch registry poisoned"))?;
            let launches = guard.as_mut().context("host is stopping")?;
            let Some(existing) = launches.get_mut(binding) else {
                return Ok(());
            };
            ensure!(
                existing.generation == generation,
                "binding generation mismatch"
            );
            existing.stop()
        };
        stopping.await?;
        let mut guard = self
            .launches
            .lock()
            .map_err(|_| anyhow::anyhow!("launch registry poisoned"))?;
        if let Some(launches) = guard.as_mut()
            && launches
                .get(binding)
                .is_some_and(|launch| launch.generation == generation && launch.releasing)
        {
            launches.remove(binding);
        }
        Ok(())
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        let launches = self
            .launches
            .lock()
            .map_err(|_| anyhow::anyhow!("launch registry poisoned"))?
            .take()
            .unwrap_or_default();
        let stopping = launches.into_values().map(|mut launch| launch.stop());
        let results = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            futures::future::join_all(stopping),
        )
        .await
        .context("managed adapters did not stop")?;
        for result in results {
            result?;
        }
        Ok(())
    }
}
