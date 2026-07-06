//! Lifecycle handles for a locally spawned ACP agent subprocess.
//!
//! The `Child` itself is owned by an exit-watcher task in `spawn()` (which
//! `wait()`s and reaps it); teardown code observes the exit via a watch
//! channel and requests kills through a `Notify`, so no `Child` handle is
//! ever shared across tasks.

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
/// The `Child` itself is owned by the exit-watcher task (which `wait()`s and
/// reaps it); teardown observes the exit via `exit_rx` and requests a kill
/// via `kill`, instead of sharing the `Child`.
pub(super) struct ChildHandle {
    /// OS pid of the direct child, used to signal its process group on Unix.
    pub(super) pid: u32,
    /// Becomes `Some(status)` once the child has exited and been reaped.
    pub(super) exit_rx: tokio::sync::watch::Receiver<Option<std::process::ExitStatus>>,
    /// Tells the watcher task to kill the child now.
    pub(super) kill: std::sync::Arc<tokio::sync::Notify>,
}

impl ChildHandle {
    /// Kill the child (and, on Unix, its whole process group) without
    /// waiting. Safe to call repeatedly or after the child already exited.
    pub(super) fn request_kill(&self) {
        // Only signal the group while the child is unreaped; after reaping
        // the pid may be reused by an unrelated process.
        if self.exit_rx.borrow().is_none() {
            #[cfg(unix)]
            if let Err(e) = kill_process_group(self.pid) {
                debug!("Failed to kill agent process group: {e}");
            }
            self.kill.notify_one();
        }
    }
}

/// Kill the entire process group to ensure grandchildren are terminated.
#[cfg(unix)]
fn kill_process_group(pid: u32) -> std::io::Result<()> {
    use std::io::ErrorKind;

    let pid = pid as libc::pid_t;

    let pgid = unsafe { libc::getpgid(pid) };
    if pgid == -1 {
        let err = std::io::Error::last_os_error();
        if err.kind() != ErrorKind::NotFound {
            return Err(err);
        }
        return Ok(());
    }

    let result = unsafe { libc::killpg(pgid, libc::SIGKILL) };
    if result == -1 {
        let err = std::io::Error::last_os_error();
        if err.kind() != ErrorKind::NotFound {
            return Err(err);
        }
    }

    Ok(())
}
