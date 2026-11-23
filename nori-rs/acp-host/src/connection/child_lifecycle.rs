//! Lifecycle handles for a locally spawned ACP agent subprocess.
//!
//! The `Child` itself is owned by an exit-watcher task in `spawn()`. On Unix,
//! the watcher observes an exit without reaping the process, sweeps the
//! child's process group, and only then reaps the child. Teardown observes
//! the final status via a watch channel and requests kills through a `Notify`,
//! so no `Child` handle is ever shared across tasks.

use tracing::debug;

/// How long shutdown waits for a child to exit after closing its stdin
/// before killing it. Generous so children with network cleanup (e.g.
/// `nori-handroll cloud-acp` releasing its broker session) can finish;
/// well-behaved agents exit in milliseconds.
pub(super) const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(25);

/// How many recent child stderr lines are kept for error reporting.
pub(super) const STDERR_TAIL_LINES: usize = 20;

/// Snapshot the bounded stderr tail as one newline-joined string.
pub(super) fn collect_stderr_tail(
    tail: &std::sync::Mutex<std::collections::VecDeque<String>>,
) -> String {
    match tail.lock() {
        Ok(tail) => tail.iter().cloned().collect::<Vec<_>>().join("\n"),
        Err(_) => String::new(),
    }
}

/// Teardown handles for a locally spawned agent.
///
/// The `Child` itself is owned by the exit-watcher task, which preserves the
/// Unix process-group leader until descendants are swept and then reaps it;
/// teardown observes the exit via `exit_rx` and requests a kill via `kill`,
/// instead of sharing the `Child`.
pub struct ChildHandle {
    /// OS pid of the direct child, used to signal its process group on Unix.
    pub(super) pid: u32,
    /// Becomes `Some(status)` once the child has exited and been reaped.
    pub(super) exit_rx: tokio::sync::watch::Receiver<Option<std::process::ExitStatus>>,
    /// Tells the watcher task to kill the child now.
    pub(super) kill: std::sync::Arc<tokio::sync::Notify>,
    /// Serializes synchronous drop-time signaling with the watcher's final
    /// group sweep, so no caller can signal a recycled process-group ID.
    pub(super) group_cleanup_complete: std::sync::Arc<std::sync::Mutex<bool>>,
}

pub struct AgentProcess {
    pub stdin: tokio::process::ChildStdin,
    pub stdout: tokio::process::ChildStdout,
    pub stderr: Option<tokio::process::ChildStderr>,
    pub handle: ChildHandle,
}

impl AgentProcess {
    pub fn spawn(cmd: tokio::process::Command) -> anyhow::Result<Self> {
        Self::spawn_with_resource(cmd, ())
    }

    /// Keep launch inputs alive until the supervisor finishes reaping the group.
    pub(super) fn spawn_with_resource(
        mut cmd: tokio::process::Command,
        resource: impl Send + 'static,
    ) -> anyhow::Result<Self> {
        use anyhow::Context;
        use std::process::Stdio;
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true);
        // Process group isolation and parent death signal.
        #[cfg(unix)]
        unsafe {
            #[cfg(target_os = "linux")]
            let parent_pid = libc::getpid();

            cmd.pre_exec(move || {
                if libc::setpgid(0, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }

                #[cfg(target_os = "linux")]
                {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::getppid() != parent_pid {
                        libc::raise(libc::SIGTERM);
                    }
                }

                Ok(())
            });
        }

        let mut child = cmd.spawn()?;
        let pid = child.id().context("missing child pid")?;
        let stdin = child.stdin.take().context("missing child stdin")?;
        let stdout = child.stdout.take().context("missing child stdout")?;
        let stderr = child.stderr.take();
        let (exit_tx, exit_rx) = tokio::sync::watch::channel(None);
        let handle = ChildHandle {
            pid,
            exit_rx,
            kill: Default::default(),
            group_cleanup_complete: Default::default(),
        };
        let kill = handle.kill.clone();
        let group = handle.group_cleanup_complete.clone();
        tokio::spawn(async move {
            let status = wait_for_child_exit(child, pid, kill, group).await;
            drop(resource);
            exit_tx.send_replace(status);
        });
        Ok(Self {
            stdin,
            stdout,
            stderr,
            handle,
        })
    }
}

impl ChildHandle {
    /// A PID for validating launcher observations made before this check.
    /// The watcher retires the process group before reaping its leader.
    #[cfg(target_os = "linux")]
    pub fn unreaped_id(&self) -> Option<u32> {
        let retired = self
            .group_cleanup_complete
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        (!*retired).then_some(self.pid)
    }

    pub fn has_exited(&self) -> bool {
        self.exit_rx.borrow().is_some() || self.exit_rx.has_changed().is_err()
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exit_rx.borrow().and_then(|status| status.code())
    }

    pub async fn stop(self) {
        self.request_kill();
        self.wait_for_exit().await;
    }

    /// Observe reaping without extending the lifetime of the owning process handle.
    pub fn wait_for_exit(&self) -> impl Future<Output = ()> + use<> {
        let mut exit = self.exit_rx.clone();
        async move {
            let _ = exit.wait_for(std::option::Option::is_some).await;
        }
    }

    /// Kill the child (and, on Unix, its whole process group) without
    /// waiting. Safe to call repeatedly or after the child already exited.
    pub fn request_kill(&self) {
        // Only signal the group while the child is unreaped; after reaping
        // the pid may be reused by an unrelated process.
        if self.exit_rx.borrow().is_none() {
            #[cfg(unix)]
            if let Err(e) = kill_process_group_once(self.pid, &self.group_cleanup_complete, false) {
                debug!("Failed to kill agent process group: {e}");
            }
            self.kill.notify_one();
        }
    }
}

impl Drop for ChildHandle {
    fn drop(&mut self) {
        // The handle is also the cancellation guard during `spawn_inner`:
        // once the child moves into the exit watcher, dropping the unfinished
        // spawn future must still signal that watcher to kill and reap it.
        self.request_kill();
    }
}

/// Own the direct child through exit, process-group cleanup, and reaping.
///
/// Unix process-group identity is anchored by the leader's PID. Reaping the
/// leader before killing surviving descendants makes a later `killpg` unsafe:
/// the PID could be reused by an unrelated process. `waitid(WNOWAIT)` lets us
/// observe the leader's exit while it remains waitable, sweep the still-pinned
/// group, and then reap it exactly once with Tokio's `Child::wait`.
pub(super) async fn wait_for_child_exit(
    mut child: tokio::process::Child,
    pid: u32,
    kill: std::sync::Arc<tokio::sync::Notify>,
    group_cleanup_complete: std::sync::Arc<std::sync::Mutex<bool>>,
) -> Option<std::process::ExitStatus> {
    #[cfg(unix)]
    {
        let observed_exit = tokio::task::spawn_blocking(move || observe_exit_without_reaping(pid));
        tokio::select! {
            observed = observed_exit => {
                match observed {
                    Ok(Ok(())) => {
                        if let Err(error) =
                            kill_process_group_once(pid, &group_cleanup_complete, true)
                        {
                            debug!("Failed to sweep exited agent process group: {error}");
                        }
                    }
                    Ok(Err(error)) => {
                        debug!("Failed to observe ACP agent exit without reaping: {error}");
                        mark_process_group_cleanup_complete(&group_cleanup_complete);
                    }
                    Err(error) => {
                        debug!("ACP agent exit observer task failed: {error}");
                        mark_process_group_cleanup_complete(&group_cleanup_complete);
                    }
                }
            }
            () = kill.notified() => {
                if let Err(error) =
                    kill_process_group_once(pid, &group_cleanup_complete, true)
                {
                    debug!("Failed to kill agent process group: {error}");
                }
                let _ = child.start_kill();
            }
        }
        child.wait().await.ok()
    }

    #[cfg(not(unix))]
    {
        let _ = group_cleanup_complete;
        tokio::select! {
            status = child.wait() => status.ok(),
            () = kill.notified() => {
                let _ = child.start_kill();
                child.wait().await.ok()
            }
        }
    }
}

/// Signal the group at most once while serializing against the final reap.
///
/// A non-final caller leaves the state retryable when signaling fails. The
/// watcher marks cleanup complete even on failure immediately before it reaps
/// the leader; after that point retrying by numeric PGID would be unsafe.
#[cfg(unix)]
fn kill_process_group_once(
    pid: u32,
    cleanup_complete: &std::sync::Mutex<bool>,
    final_attempt: bool,
) -> std::io::Result<()> {
    let mut cleanup_complete = cleanup_complete
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if *cleanup_complete {
        return Ok(());
    }

    let result = kill_process_group(pid);
    if result.is_ok() || final_attempt {
        *cleanup_complete = true;
    }
    result
}

/// Disable any later synchronous PGID signal when exit observation itself
/// failed and therefore cannot prove that the numeric group ID is still ours.
#[cfg(unix)]
fn mark_process_group_cleanup_complete(cleanup_complete: &std::sync::Mutex<bool>) {
    *cleanup_complete
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
}

/// Wait for `pid` to exit while deliberately leaving it waitable.
#[cfg(unix)]
fn observe_exit_without_reaping(pid: u32) -> std::io::Result<()> {
    loop {
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if result == 0 {
            return Ok(());
        }

        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// Kill the entire process group to ensure grandchildren are terminated.
#[cfg(unix)]
fn kill_process_group(pgid: u32) -> std::io::Result<()> {
    use std::io::ErrorKind;

    // `spawn_inner` establishes the child as its process-group leader before
    // exec, so its PID is the stable group ID even after the leader exits.
    let pgid = pgid as libc::pid_t;

    let result = unsafe { libc::killpg(pgid, libc::SIGKILL) };
    if result == -1 {
        let err = std::io::Error::last_os_error();
        if err.kind() != ErrorKind::NotFound {
            return Err(err);
        }
    }

    Ok(())
}
