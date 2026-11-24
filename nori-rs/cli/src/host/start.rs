//! Startup election, detached child handoff and authenticated readiness.

use std::fs::File;
use std::io::Seek;
use std::io::Write;
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use nori_harness::remote_agent::ManagedClient;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::state;
use super::state::HostFiles;
use super::state::HostState;

const START_TIMEOUT: Duration = Duration::from_secs(15);

/// A separate operation lock lets the host keep the existing lifetime lock
/// and lets concurrent CLI processes sleep in the kernel during a cold start.
pub(super) async fn startup_lock(files: &HostFiles) -> anyhow::Result<File> {
    let lock = state::private_open(&files.directory.join("start.lock"), true)?;
    match lock.try_lock() {
        Ok(()) => return Ok(lock),
        Err(std::fs::TryLockError::WouldBlock) => {}
        Err(error) => return Err(error.into()),
    }
    let (send, receive) = oneshot::channel();
    // A stuck owner must not keep the CLI's Tokio runtime alive past its deadline.
    std::thread::Builder::new()
        .name("host-start-lock".into())
        .spawn(move || {
            let result = lock.lock().map(|()| lock);
            let _ = send.send(result);
        })?;
    Ok(tokio::time::timeout(START_TIMEOUT, receive)
        .await
        .context("timed out waiting for another host operation")???)
}

/// Observers briefly take this lock to distinguish a dead host from stale files.
/// Only the elected starter calls this. A contender never spawns a waiting child.
pub(super) async fn lifetime_lock(files: &HostFiles) -> anyhow::Result<File> {
    let lock = files.lock()?;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(150);
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(std::fs::TryLockError::WouldBlock) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            Err(error) => return Err(error).context("another host is already starting or running"),
        }
    }
}

pub(super) enum Startup {
    /// A direct foreground host may notify an in-process managed stdio relay.
    Foreground(Option<oneshot::Sender<()>>),
    /// The launcher holds the operation lock until it verifies this child's instance.
    Detached(File),
}

impl Startup {
    pub(super) fn detached() -> anyhow::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::fd::AsFd;
            // The child receives the one-use notification pipe on stdout; restore
            // stdout to the log before serving. The duplicate is close-on-exec.
            let pipe = std::io::stdout().as_fd().try_clone_to_owned()?;
            if unsafe { libc::dup2(libc::STDERR_FILENO, libc::STDOUT_FILENO) } < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            Ok(Self::Detached(pipe.into()))
        }
        #[cfg(not(unix))]
        anyhow::bail!("detached host startup requires Unix")
    }

    pub(super) fn ready(self, instance: &str) -> anyhow::Result<()> {
        match self {
            Self::Detached(mut pipe) => pipe.write_all(Uuid::parse_str(instance)?.as_bytes())?,
            Self::Foreground(Some(relay)) => {
                relay.send(()).map_err(|()| {
                    anyhow::anyhow!("the managed stdio relay exited during startup")
                })?;
            }
            Self::Foreground(None) => {}
        }
        Ok(())
    }
}

/// Probe without taking the startup operation lock on the warm path.
async fn probe(files: &HostFiles) -> anyhow::Result<(HostState, Option<Uuid>)> {
    let (host, token) = files.read()?;
    let instance = tokio::time::timeout(Duration::from_secs(3), async {
        let mut client = ManagedClient::connect_lifecycle(&host.endpoint, &token).await?;
        client
            .request("initialize", json!({"observe":true}))
            .await?;
        anyhow::Ok(client.host_instance())
    })
    .await
    .context("host did not answer")??;

    // A managed host has no host.json. Preserve discovery for older compatible
    // hosts, including those which predate the instance header on upgrades.
    if files.directory.join("host.json").try_exists()? {
        let discovery = files.read_discovery();
        if !discovery.as_ref().is_some_and(|discovery| {
            nori_protocol::host_serves_contract(&discovery.contract, nori_protocol::HOST_CONTRACT)
        }) {
            let (version, contract) = discovery
                .map(|d| (d.version, d.contract))
                .unwrap_or_else(|| ("unknown".to_owned(), "unknown".to_owned()));
            anyhow::bail!(
                "the running host (pid {}, nori {version}, contract {contract}) does not serve this nori's contract {}; stop it with `nori host stop`, or run this nori's host beside it in another home: NORI_HOME=<dir> nori host",
                host.pid,
                nori_protocol::HOST_CONTRACT
            );
        }
        let discovery = discovery.context("missing host discovery")?;
        anyhow::ensure!(
            discovery.pid == host.pid
                && discovery.endpoint == host.endpoint
                && instance
                    .is_none_or(|id| Some(id) == Uuid::parse_str(&discovery.instance_id).ok()),
            "host discovery changed during startup; retry"
        );
    }
    Ok((host, instance))
}

fn already_running(host: HostState) {
    println!("Host already running (pid {})  {}", host.pid, host.endpoint);
}

pub(super) async fn start(files: &HostFiles) -> anyhow::Result<()> {
    if files.directory.join("state.json").try_exists()?
        && let Ok((host, _)) = probe(files).await
    {
        already_running(host);
        return Ok(());
    }
    let _election = startup_lock(files).await?;
    if files.running()? {
        match probe(files).await {
            Ok((host, _)) => {
                already_running(host);
                return Ok(());
            }
            Err(error) => {
                // A discovery reader may have been the only lock holder. Recheck
                // while still elected; a persistent unresponsive owner is an error.
                if lifetime_lock(files).await.is_err() {
                    return Err(error)
                        .context("a host owns this home but did not confirm readiness");
                }
            }
        }
    }
    let log_path = files.directory.join("host.log");
    let mut log = state::private_open(&log_path, true)?;
    log.seek(std::io::SeekFrom::End(0))?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(std::env::args_os().skip(1))
        .args(["--foreground", "--notify-launcher"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(log);
    #[cfg(unix)]
    // SAFETY: the only operation between fork and exec is the async-signal-safe setsid.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = command.spawn().context("could not start host process")?;
    let readiness = async {
        let mut bytes = [0; 16];
        child
            .stdout
            .take()
            .context("missing host startup pipe")?
            .read_exact(&mut bytes)
            .await
            .context("host exited before publishing readiness")?;
        let (host, instance) = probe(files).await?;
        anyhow::ensure!(
            Some(host.pid) == child.id().map(i64::from)
                && instance == Some(Uuid::from_bytes(bytes)),
            "the ready endpoint does not belong to the host just started"
        );
        anyhow::Ok(host)
    };
    let result = tokio::time::timeout(START_TIMEOUT, readiness)
        .await
        .context("host startup timed out")
        .and_then(|ready| ready);
    let host = match result {
        Ok(host) => host,
        Err(error) => {
            // Only terminate our own unreaped child; never use a discovery PID here.
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(error)
                .with_context(|| format!("host startup failed; check {}", log_path.display()));
        }
    };
    println!("Host started (pid {})  {}", host.pid, host.endpoint);
    println!(
        "Token: {} (owner-only)",
        files.directory.join("token").display()
    );
    Ok(())
}
