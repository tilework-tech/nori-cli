use std::path::Path;

use super::LaunchSpec;
use tokio::process::Command;

#[cfg(not(target_os = "linux"))]
pub(super) fn command(_: &LaunchSpec, _: &Path, _: &Path) -> anyhow::Result<Command> {
    anyhow::bail!("read-only workspace enforcement unavailable on this platform")
}

#[cfg(target_os = "linux")]
pub(super) fn command(spec: &LaunchSpec, cwd: &Path, home: &Path) -> anyhow::Result<Command> {
    use anyhow::Context;
    use anyhow::ensure;
    use std::ffi::OsStr;
    use std::fs::File;
    use std::io::Seek;
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::fd::FromRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::process::Stdio;

    const BWRAP: &str = "/usr/bin/bwrap";
    ensure!(
        Path::new(BWRAP).is_file(),
        "read-only workspace enforcement requires {BWRAP}"
    );
    for mount in [Path::new("/dev"), Path::new("/proc")] {
        ensure!(
            ![cwd, home]
                .iter()
                .any(|root| root.starts_with(mount) || mount.starts_with(root)),
            "pinned roots overlap a private sandbox mount"
        );
    }
    // The launcher runs with an empty environment. Adapter settings (including
    // credentials and dynamic loader settings) are passed only to the payload.
    // SAFETY: the kernel copies the static NUL-terminated name. A successful
    // call returns a new descriptor owned only by this File.
    let mut settings = unsafe {
        let fd = libc::memfd_create(c"nori-side-settings".as_ptr(), libc::MFD_CLOEXEC);
        if fd < 0 {
            return Err(std::io::Error::last_os_error())
                .context("creating sandbox settings descriptor");
        }
        File::from_raw_fd(fd)
    };
    let mut set_env = |key: &str, value: &OsStr| -> std::io::Result<()> {
        settings.write_all(b"--setenv\0")?;
        settings.write_all(key.as_bytes())?;
        settings.write_all(&[0])?;
        settings.write_all(value.as_bytes())?;
        settings.write_all(&[0])
    };
    for (key, value) in &spec.env {
        set_env(key, OsStr::new(value))?;
    }
    set_env("HOME", home.as_os_str())?;
    set_env("CODEX_HOME", home.join(".codex").as_os_str())?;
    set_env("NORI_HOME", home.join(".nori/cli").as_os_str())?;
    set_env("TMPDIR", OsStr::new("/dev/shm"))?;
    settings.rewind()?;

    let mut command = Command::new(BWRAP);
    command
        .env_clear()
        .args([
            "--unshare-user",
            "--unshare-pid",
            "--unshare-ipc",
            "--disable-userns",
            "--cap-drop",
            "ALL",
            "--die-with-parent",
            "--new-session",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--tmpfs",
            "/dev/shm",
        ])
        .arg("--chdir")
        .arg(cwd)
        .arg("--args")
        .arg(settings.as_raw_fd().to_string())
        .arg("--")
        .arg(&spec.adapter.path)
        .args(&spec.adapter.args)
        .current_dir(cwd)
        .stderr(Stdio::null());
    // SAFETY: fcntl is async-signal-safe. The closure owns the open descriptor
    // until spawn returns; Bubblewrap consumes it before executing the adapter.
    unsafe {
        command.pre_exec(move || {
            if libc::fcntl(settings.as_raw_fd(), libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    Ok(command)
}
