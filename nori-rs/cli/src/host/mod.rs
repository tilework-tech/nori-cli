//! Background session host commands and local discovery.

mod acp;
mod local_stdio;
mod oneshot;
mod recovery_control;
mod recovery_history;
mod service;
mod session_lookup;
mod state;
mod stdio;
mod table;
mod transcripts;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use clap::Parser;
use nori_config::NoriConfig;
use nori_config::NoriConfigOverrides;
use nori_harness::managed_launch::HostSessionIdentity;
use nori_harness::remote_agent::ManagedClient;
use nori_harness::remote_agent::ManagedRemoteHost;
use nori_harness::remote_agent::RemoteAcpServer;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionManagerConfig;
use serde::Deserialize;
use serde_json::json;

pub(crate) use self::local_stdio::serve as serve_stdio;
pub(crate) use self::oneshot::Request as OneShot;
pub(crate) use self::oneshot::run as run_one_shot;
use self::service::HostService;
use self::state::HostFiles;
use self::state::HostState;

#[derive(Debug, Parser)]
pub(crate) struct HostCommand {
    #[command(subcommand)]
    action: Option<HostAction>,
    /// Agent for this host or its new session.
    #[arg(long, short = 'a')]
    agent: Option<String>,
    /// Loopback port; zero chooses an available port.
    #[arg(long, default_value = "7420")]
    port: i32,
    #[arg(long, hide = true)]
    foreground: bool,
    /// Serve one client as ACP on stdin and stdout, in the foreground, until
    /// stdin closes. With --launch, serve that one managed launch instead;
    /// it needs `[host] managed` and `[host.dedicated_roots]`.
    #[arg(long, conflicts_with = "foreground")]
    stdio: bool,
    /// JSON launch specification for --stdio.
    #[arg(long, value_name = "FILE", requires = "stdio")]
    launch: Option<PathBuf>,
    /// Add this process's environment to the --stdio launch. Variables in the
    /// specification win.
    #[arg(long, requires = "launch")]
    inherit_env: bool,
}

#[derive(Debug, clap::Subcommand)]
enum HostAction {
    /// Stop the host and all of its sessions.
    Stop,
    /// List sessions in a flat table, plain when piped.
    Ls {
        /// Print session IDs and public credential reference labels as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Start a session on the running host and print its ID.
    New(NewCommand),
}

#[derive(Debug, Parser)]
struct NewCommand {
    /// Folder the session works in; relative paths start from the current
    /// directory, which is also the default for the local host. A host named
    /// with --host uses its own folder by default.
    #[arg(long)]
    cwd: Option<PathBuf>,
    /// Agent to run: claude-code, codex, gemini (Antigravity), grok-build, opencode, cursor, hermes, pi, or a configured agent.
    #[arg(long)]
    agent: Option<String>,
    /// First prompt to send once the session starts.
    #[arg(long)]
    prompt: Option<String>,
    /// Print `{"session_id", "cwd"}` instead of the bare ID.
    #[arg(long)]
    json: bool,
    /// Remote managed WebSocket URL; omit to use the local host.
    #[arg(long, requires = "token_file")]
    host: Option<String>,
    /// Private token file for a remote host.
    #[arg(long, requires = "host")]
    token_file: Option<PathBuf>,
}

impl HostCommand {
    pub(crate) fn select_agent(&mut self, root_agent: Option<String>) -> Option<String> {
        let agent = self.agent.take().or(root_agent);
        if let Some(HostAction::New(command)) = &mut self.action {
            command.agent = command.agent.take().or(agent);
            return None;
        }
        agent
    }

    pub(crate) fn needs_config(&self) -> bool {
        self.action.is_none()
    }
}

/// Start a session as `command` describes and print it. The folder is
/// resolved here, against the caller's directory, and checked by the host.
async fn new_session(command: NewCommand, files: &HostFiles) -> anyhow::Result<()> {
    // A named host may be another machine, where this directory need not
    // exist, so only a folder given explicitly is sent to it.
    let remote = command.host.is_some();
    let (mut client, initialized) = match (command.host, command.token_file) {
        (Some(endpoint), Some(path)) => {
            connect_endpoint(&endpoint, &state::read_token(&path)?).await?
        }
        _ => {
            let (host, token) = files.read()?;
            connect_endpoint(&host.endpoint, &token).await?
        }
    };
    anyhow::ensure!(
        initialized["capabilities"]["nori_sessions"]["session_cwd"] == true,
        "this host is too old for `nori host new`; restart it with this nori"
    );
    let mut params = json!({});
    let cwd = command
        .cwd
        .or_else(|| (!remote).then(|| PathBuf::from(".")));
    if let Some(cwd) = cwd {
        let cwd = std::path::absolute(cwd)?;
        params["cwd"] = json!(cwd.to_str().context("folder is not valid UTF-8")?);
    }
    if let Some(agent) = command.agent {
        params["agent"] = json!(agent);
    }
    if let Some(prompt) = command.prompt {
        params["prompt"] = json!(prompt);
    }
    let started = client.request("_nori/session/new", params).await?;
    let id = started["session_id"]
        .as_str()
        .context("host did not return the new session ID")?;
    if command.json {
        println!("{started}");
    } else {
        println!("{id}");
    }
    Ok(())
}

#[derive(Debug, Parser)]
pub(crate) struct AttachCommand {
    /// Session to open; omit to open the session list.
    session_id: Option<String>,
    /// Remote managed WebSocket URL; omit to discover the local host.
    #[arg(long, requires = "token_file")]
    host: Option<String>,
    /// Private token file for a remote host.
    #[arg(long, requires = "host")]
    token_file: Option<PathBuf>,
}

pub(crate) async fn attach(command: AttachCommand, config: NoriConfig) -> anyhow::Result<()> {
    anyhow::ensure!(
        config.multi_session,
        "nori attach requires [features] multi_session = true"
    );
    let mut refresh_local_token = None;
    let (endpoint, token) = if let Some(endpoint) = command.host {
        let path = command.token_file.context("--token-file is required")?;
        (endpoint, state::read_token(&path)?)
    } else {
        let files = HostFiles::new(&config.nori_home)?;
        let (host, token) = files.read()?;
        let token_path = files.directory.join("token");
        refresh_local_token = Some(Box::new(move || state::read_token(&token_path))
            as Box<dyn Fn() -> anyhow::Result<String> + Send + Sync>);
        (host.endpoint, token)
    };
    // A session can be named by the start of its ID; the host knows only
    // full IDs.
    let session_id = match command.session_id {
        // An ID in the host's own spelling needs no lookup.
        Some(full)
            if uuid::Uuid::parse_str(&full)
                .is_ok_and(|parsed| parsed.hyphenated().to_string() == full) =>
        {
            Some(full)
        }
        Some(query) => {
            let (mut client, _) = connect_endpoint(&endpoint, &token).await?;
            let listing = tokio::time::timeout(
                Duration::from_secs(3),
                client.request("_nori/session/list", json!({})),
            )
            .await
            .context("host did not list its sessions")??;
            Some(session_lookup::resolve_session(&listing, &query)?)
        }
        None => None,
    };
    nori_tui::run_attach(
        config,
        nori_tui::AttachOptions {
            endpoint,
            token,
            session_id,
            refresh_local_token,
        },
    )
    .await
}

pub(crate) async fn run(
    command: HostCommand,
    config: Option<(NoriConfig, NoriConfigOverrides)>,
) -> anyhow::Result<()> {
    let home = match &config {
        Some((config, _)) => config.nori_home.clone(),
        None => nori_config::find_nori_home()?,
    };
    let files = HostFiles::new(&home)?;
    match command.action {
        Some(HostAction::New(new)) => new_session(new, &files).await,
        Some(HostAction::Ls { json }) => {
            let (mut client, _) = connect(&files).await?;
            let listing = client.request("_nori/session/list", json!({})).await?;
            if json {
                let sessions = Vec::<HostSessionIdentity>::deserialize(&listing["sessions"])
                    .context("host returned an invalid session list")?;
                println!("{}", json!({"sessions":sessions}));
                return Ok(());
            }
            let tty = std::io::stdout().is_terminal();
            let width = tty
                .then(|| crossterm::terminal::size().map_or(80, |(width, _)| usize::from(width)));
            // Reading the terminal's background answers on stdin.
            let shade = if tty && std::io::stdin().is_terminal() {
                nori_tui::session_list_shade()
            } else {
                None
            };
            print!("{}", table::format_table(&listing, width, tty, shade));
            Ok(())
        }
        Some(HostAction::Stop) => {
            if !files.running()? {
                println!("No host running.");
                return Ok(());
            }
            let (mut client, _) = connect(&files).await?;
            // Shutdown may close the socket before its reply. Ownership is
            // released only after session cleanup, so wait for the lock below.
            client
                .send(json!({"jsonrpc":"2.0","id":2,"method":"_nori/host/stop","params":{}}))
                .await?;
            let deadline = Instant::now() + Duration::from_secs(30);
            while files.running()? {
                anyhow::ensure!(
                    Instant::now() < deadline,
                    "host is still shutting down; check {}",
                    files.directory.join("host.log").display()
                );
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            println!("Host stopped.");
            Ok(())
        }
        None if command.stdio && command.launch.is_none() => {
            local_stdio::serve(config.context("host configuration is required")?).await
        }
        None if command.stdio => {
            let (config, overrides) = config.context("host configuration is required")?;
            let binding = config
                .host
                .dedicated_roots
                .as_ref()
                .filter(|_| config.host.managed)
                .context("--stdio needs [host] managed = true and [host.dedicated_roots]")?
                .binding_id
                .clone();
            let launch = command.launch.context("--stdio needs --launch")?;
            let served = serve(0, (config, overrides), &files);
            tokio::pin!(served);
            let relayed = tokio::select! {
                ended = &mut served => {
                    ended?;
                    anyhow::bail!("the host stopped before the ACP stream ended");
                }
                relayed = stdio::relay(&files, &binding, &launch, command.inherit_env) => relayed,
            };
            tokio::time::timeout(Duration::from_secs(35), served)
                .await
                .context("the host did not stop")??;
            relayed
        }
        None if command.foreground => {
            serve(
                command.port,
                config.context("host configuration is required")?,
                &files,
            )
            .await
        }
        None => start(&files).await,
    }
}

async fn connect(files: &HostFiles) -> anyhow::Result<(ManagedClient, HostState)> {
    let (host, token) = files.read()?;
    let (client, _) = connect_endpoint(&host.endpoint, &token).await?;
    Ok((client, host))
}

/// A watching client of the host at `endpoint`, with the host's
/// `initialize` reply.
async fn connect_endpoint(
    endpoint: &str,
    token: &str,
) -> anyhow::Result<(ManagedClient, serde_json::Value)> {
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut client = ManagedClient::connect(endpoint, token).await?;
        let initialized = client
            .request("initialize", json!({"observe":true}))
            .await?;
        anyhow::Ok((client, initialized))
    })
    .await
    .context("host did not answer")?
}

async fn start(files: &HostFiles) -> anyhow::Result<()> {
    if let Ok((_, host)) = connect(files).await {
        println!("Host already running (pid {})  {}", host.pid, host.endpoint);
        return Ok(());
    }
    let log_path = files.directory.join("host.log");
    let log = state::private_open(&log_path, true)?;
    // Append through cloned handles without exposing output pipes to the daemon.
    use std::io::Seek;
    let mut log = log;
    log.seek(std::io::SeekFrom::End(0))?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(std::env::args_os().skip(1))
        .arg("--foreground")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Only the async-signal-safe syscall runs between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = command.spawn().context("could not start host process")?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok((_, host)) = connect(files).await {
            println!("Host started (pid {})  {}", host.pid, host.endpoint);
            println!(
                "Token: {} (owner-only)",
                files.directory.join("token").display()
            );
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            anyhow::bail!(
                "host exited during startup ({status}); check {}",
                log_path.display()
            );
        }
        if Instant::now() >= deadline {
            // This is the child we just spawned, not a PID from stale state.
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!("host startup timed out; check {}", log_path.display());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn serve(
    port: i32,
    (config, overrides): (NoriConfig, NoriConfigOverrides),
    files: &HostFiles,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        (0..=65535).contains(&port),
        "port must be between 0 and 65535"
    );
    let lock = files.lock()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                // Discovery probes briefly acquire the same lock. Only a
                // persistent owner means another daemon is running.
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(error) => return Err(error).context("another host is already starting or running"),
        }
    }
    // A crash can leave discovery state, but cannot retain the OS lock.
    let _ = std::fs::remove_file(files.directory.join("state.json"));
    let token = files.new_token()?;
    let launches = if config.host.managed {
        Some(nori_harness::managed_launch::ManagedLaunches::from_config(
            &config.host,
            &config.nori_home,
        )?)
    } else {
        nori_harness::initialize_registry(config.agents.clone())?;
        None
    };
    let manager = SessionManager::new(SessionManagerConfig {
        max_sessions: config.max_sessions,
        ..Default::default()
    });
    let recovery = if launches.is_some() {
        Some(recovery_control::RecoveryControl::open(&files.directory)?)
    } else {
        None
    };
    let host = host_service(config, overrides, manager, launches, recovery);
    let server = RemoteAcpServer::bind_host(
        format!("127.0.0.1:{port}").parse()?,
        host.clone(),
        token.clone(),
    )
    .await?;
    let endpoint = format!("ws://{}/sessions", server.local_addr());
    let pid = i64::from(std::process::id());
    // `state.json` is what `nori host` waits for, so `host.json` must exist
    // before it does.
    if host.launches.is_none() {
        files.publish_discovery(&nori_protocol::HostDiscovery {
            endpoint: endpoint.clone(),
            token,
            pid,
            instance_id: host.instance.clone(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            contract: nori_protocol::HostContractVersions {
                acp: 1,
                nori: acp::handshake::CONTRACT,
            },
            started_by: std::env::var("NORI_HOST_STARTED_BY")
                .ok()
                .filter(|started_by| !started_by.trim().is_empty())
                .unwrap_or_else(|| "nori-cli".to_owned()),
        })?;
    }
    files.publish(&HostState { endpoint, pid })?;
    #[cfg(unix)]
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    #[cfg(unix)]
    tokio::select! {
        _ = host.stop.notified() => {},
        _ = tokio::signal::ctrl_c() => {},
        _ = terminate.recv() => {},
    }
    #[cfg(not(unix))]
    host.stop.notified().await;
    server.shutdown().await;
    if let Some(launches) = &host.launches {
        launches.shutdown().await?;
    }
    let report = host.manager.shutdown(Duration::ZERO).await;
    std::fs::remove_file(files.directory.join("state.json"))?;
    match std::fs::remove_file(files.directory.join("host.json")) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    anyhow::ensure!(
        report.timed_out.is_empty() && report.failed.is_empty(),
        "some sessions did not stop cleanly: {report:?}"
    );
    drop(lock);
    Ok(())
}

fn host_service(
    config: NoriConfig,
    overrides: NoriConfigOverrides,
    manager: SessionManager,
    launches: Option<nori_harness::managed_launch::ManagedLaunches>,
    recovery: Option<recovery_control::RecoveryControl>,
) -> Arc<HostService> {
    let settings = acp::config::SettingsStore::open(&config.nori_home);
    Arc::new(HostService {
        recovery,
        launches,
        managed: ManagedRemoteHost::authenticated_host(manager.clone()),
        manager,
        config: Arc::new(config),
        overrides,
        stop: tokio::sync::Notify::new(),
        acp: acp::AcpFace::default(),
        settings,
        instance: uuid::Uuid::new_v4().to_string(),
        deleted: Default::default(),
        restarts: tokio::sync::Mutex::new(()),
    })
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn new_session_uses_the_nearest_explicit_agent() {
        for (args, expected) in [
            (vec!["host", "new"], "codex"),
            (vec!["host", "--agent", "agy", "new"], "agy"),
            (
                vec!["host", "--agent", "agy", "new", "--agent", "grok"],
                "grok",
            ),
        ] {
            let mut host = HostCommand::try_parse_from(args).unwrap();
            host.select_agent(Some("codex".into()));
            let Some(HostAction::New(command)) = host.action else {
                panic!("expected new");
            };
            assert_eq!(command.agent.as_deref(), Some(expected));
        }
    }
}
