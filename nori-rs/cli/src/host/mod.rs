//! Background session host commands and local discovery.

mod service;
mod state;

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
use nori_harness::remote_agent::ManagedClient;
use nori_harness::remote_agent::ManagedRemoteHost;
use nori_harness::remote_agent::RemoteAcpServer;
use nori_harness::session_manager::SessionManager;
use nori_harness::session_manager::SessionManagerConfig;
use owo_colors::OwoColorize;
use serde_json::Value;
use serde_json::json;

use self::service::HostService;
use self::state::HostFiles;
use self::state::HostState;

#[derive(Debug, Parser)]
pub(crate) struct HostCommand {
    #[command(subcommand)]
    action: Option<HostAction>,
    /// Loopback port; zero chooses an available port.
    #[arg(long, default_value = "7420")]
    port: i32,
    #[arg(long, hide = true)]
    foreground: bool,
}

#[derive(Debug, clap::Subcommand)]
enum HostAction {
    /// Stop the host and all of its sessions.
    Stop,
    /// List sessions in a flat table, plain when piped.
    Ls,
}

impl HostCommand {
    pub(crate) fn needs_config(&self) -> bool {
        self.action.is_none()
    }
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
    let (endpoint, token) = if let Some(endpoint) = command.host {
        let path = command.token_file.context("--token-file is required")?;
        (endpoint, state::read_token(&path)?)
    } else {
        let (host, token) = HostFiles::new(&config.nori_home)?.read()?;
        (host.endpoint, token)
    };
    nori_tui::run_attach(
        config,
        nori_tui::AttachOptions {
            endpoint,
            token,
            session_id: command.session_id,
        },
    )
    .await
}

pub(crate) async fn run(command: HostCommand, config: Option<NoriConfig>) -> anyhow::Result<()> {
    let home = match &config {
        Some(config) => config.nori_home.clone(),
        None => nori_config::find_nori_home()?,
    };
    let files = HostFiles::new(&home)?;
    match command.action {
        Some(HostAction::Ls) => {
            let (mut client, _) = connect(&files).await?;
            let listing = client.request("_nori/session/list", json!({})).await?;
            print_table(&listing);
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
    let client = tokio::time::timeout(Duration::from_secs(3), async {
        let mut client = ManagedClient::connect(&host.endpoint, &token).await?;
        client
            .request("initialize", json!({"observe":true}))
            .await?;
        anyhow::Ok(client)
    })
    .await
    .context("host did not answer")??;
    Ok((client, host))
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

async fn serve(port: i32, config: NoriConfig, files: &HostFiles) -> anyhow::Result<()> {
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
    nori_harness::initialize_registry(config.agents.clone())?;
    let manager = SessionManager::new(SessionManagerConfig {
        max_sessions: config.max_sessions,
        ..Default::default()
    });
    let host = Arc::new(HostService {
        managed: ManagedRemoteHost::new(manager.clone()),
        manager,
        config: Arc::new(config),
        stop: tokio::sync::Notify::new(),
    });
    let server =
        RemoteAcpServer::bind_host(format!("127.0.0.1:{port}").parse()?, host.clone(), token)
            .await?;
    files.publish(&HostState {
        endpoint: format!("ws://{}/sessions", server.local_addr()),
        pid: i64::from(std::process::id()),
    })?;
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
    let report = host.manager.shutdown(Duration::ZERO).await;
    std::fs::remove_file(files.directory.join("state.json"))?;
    anyhow::ensure!(
        report.timed_out.is_empty() && report.failed.is_empty(),
        "some sessions did not stop cleanly: {report:?}"
    );
    drop(lock);
    Ok(())
}

fn print_table(listing: &Value) {
    let color = std::io::stdout().is_terminal();
    println!(
        "{:<36}  {:<16}  {:<16}  {:>6}  TITLE",
        "ID", "AGENT", "STATUS", "AGE"
    );
    for row in listing["sessions"].as_array().into_iter().flatten() {
        let clean = |key: &str| {
            row[key]
                .as_str()
                .unwrap_or("-")
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect::<String>()
        };
        let id = clean("session_id");
        let agent = clean("agent");
        let status = clean("status").replace('_', " ");
        let title = clean("title");
        let seconds = row["age_seconds"].as_i64().unwrap_or(0).max(0);
        let age = if seconds < 60 {
            format!("{seconds}s")
        } else if seconds < 3600 {
            format!("{}m", seconds / 60)
        } else {
            format!("{}h", seconds / 3600)
        };
        let status = format!("{status:<16}");
        let status = if color {
            match row["status"].as_str().unwrap_or_default() {
                "failed" => status.red().to_string(),
                "waiting_on_you" | "working" | "starting" => status.yellow().to_string(),
                "ready" => status.green().to_string(),
                _ => status.dimmed().to_string(),
            }
        } else {
            status
        };
        println!("{id:<36}  {agent:<16}  {status}  {age:>6}  {title}");
    }
}
