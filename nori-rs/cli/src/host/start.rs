//! Starting the detached background host.

use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;

use super::connect;
use super::state;
use super::state::HostFiles;

pub(super) async fn start(files: &HostFiles) -> anyhow::Result<()> {
    match connect(files).await {
        Ok((_, host)) => {
            if let Some(discovery) = files.read_discovery()
                && !nori_protocol::host_serves_contract(
                    &discovery.contract,
                    nori_protocol::HOST_CONTRACT,
                )
            {
                anyhow::bail!(
                    "the running host (pid {}, nori {}, contract {}) does not serve this nori's contract {}; \
                     stop it with `nori host stop`, or run this nori's host beside it in another home: \
                     NORI_HOME=<dir> nori host",
                    host.pid,
                    discovery.version,
                    discovery.contract,
                    nori_protocol::HOST_CONTRACT,
                );
            }
            println!("Host already running (pid {})  {}", host.pid, host.endpoint);
            return Ok(());
        }
        // A host at its client limit refuses the probe but holds the lock.
        Err(error) if files.running()? => {
            println!("Host already running; it refused this connection: {error:#}");
            return Ok(());
        }
        Err(_) => {}
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
