//! Observe a rebuilt side's PID namespace through shutdown of its last process.

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::sync::Weak;
use std::time::Duration;

use anyhow::Context;
use anyhow::ensure;
use nori_acp_host::connection::ChildHandle;
use serde::Deserialize;
use tokio::io::AsyncReadExt;
use tokio::io::Interest;
use tokio::io::unix::AsyncFd;
use tokio::net::UnixStream;

pub(super) struct NamespaceExit {
    info: UnixStream,
    gate: UnixStream,
}

impl NamespaceExit {
    pub(super) fn prepare() -> anyhow::Result<(Self, [File; 2])> {
        let pair = || -> io::Result<(UnixStream, File)> {
            let (host, sandbox) = StdUnixStream::pair()?;
            host.set_nonblocking(true)?;
            Ok((
                UnixStream::from_std(host)?,
                File::from(OwnedFd::from(sandbox)),
            ))
        };
        let (info, writer) = pair()?;
        let (gate, reader) = pair()?;
        Ok((Self { info, gate }, [writer, reader]))
    }

    pub(super) async fn monitor(
        mut self,
        supervisor: Weak<ChildHandle>,
        outer_exit: impl Future<Output = ()>,
    ) -> anyhow::Result<()> {
        let init = tokio::time::timeout(Duration::from_secs(5), self.open_init(&supervisor))
            .await
            .context("timed out reading Bubblewrap namespace information")
            .and_then(|result| result);
        let init = match init {
            Ok(init) => init,
            Err(error) => {
                tracing::debug!("Cannot track side namespace: {error:#}");
                if let Some(supervisor) = supervisor.upgrade() {
                    supervisor.request_kill();
                }
                outer_exit.await;
                // Closing the gate would allow the payload to exec. Keep it
                // open until Bubblewrap has discarded every peer descriptor.
                let mut byte = [0];
                ensure!(
                    self.gate.read(&mut byte).await? == 0,
                    "unexpected data on side startup gate"
                );
                return Ok(());
            }
        };
        drop(self.gate);
        outer_exit.await;
        // PID 1 can leave the monitor's process group before arming its
        // parent-death signal. Address the verified pidfd across that window.
        // SAFETY: the descriptor stays owned and a null siginfo requests the
        // kernel's ordinary signal delivery.
        let result = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                init.get_ref().as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0u32,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error).context("stopping side namespace init");
            }
        }
        // PID 1 becomes readable after the kernel drains the namespace.
        let _ready = init
            .readable()
            .await
            .context("waiting for side namespace exit")?;
        Ok(())
    }

    async fn open_init(
        &mut self,
        supervisor: &Weak<ChildHandle>,
    ) -> anyhow::Result<AsyncFd<OwnedFd>> {
        #[derive(Deserialize)]
        struct Info {
            #[serde(rename = "child-pid")]
            pid: libc::pid_t,
        }

        let mut report = Vec::new();
        (&mut self.info).take(4097).read_to_end(&mut report).await?;
        ensure!(
            report.len() <= 4096,
            "oversized Bubblewrap namespace information"
        );
        let Info { pid } = serde_json::from_slice(&report)?;
        ensure!(pid > 0, "invalid Bubblewrap namespace init PID");
        let supervisor = supervisor.upgrade().context("side supervisor dropped")?;

        // SAFETY: pidfd_open takes a process ID and flags, and returns a new
        // owned descriptor on success. It does not signal the process.
        let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0u32) };
        if raw < 0 {
            return Err(io::Error::last_os_error()).context("opening side namespace pidfd");
        }
        let init = unsafe { OwnedFd::from_raw_fd(raw as libc::c_int) };
        // Bubblewrap's only direct child in this launch is the namespace init.
        // Validate after opening the pidfd, with the payload still gated.
        let status = std::fs::read_to_string(format!("/proc/{pid}/status"))?;
        let parent = status
            .lines()
            .find_map(|line| line.strip_prefix("PPid:"))
            .and_then(|s| s.trim().parse::<u32>().ok())
            .context("missing side init parent PID")?;
        ensure!(
            supervisor.unreaped_id() == Some(parent),
            "namespace init is no longer a child of the admitted Bubblewrap supervisor"
        );
        // SAFETY: AsyncFd takes sole ownership of the pidfd and keeps it open
        // and unchanged for the full registration lifetime.
        unsafe { AsyncFd::register_with_interest(init, Interest::READABLE) }
            .context("registering side namespace pidfd")
    }
}
