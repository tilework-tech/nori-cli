use std::path::Path;
use std::path::PathBuf;

use super::LaunchSpec;
use tokio::process::Command;

const NAMESPACE_ARGS: [&str; 17] = [
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
];

/// The machine's Bubblewrap launcher and whether it can create the read-only
/// launch on this machine.
pub(super) struct Sandbox {
    pub(super) launcher: PathBuf,
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(super) usable: bool,
}

impl Sandbox {
    /// Prefer the system install and fall back to the host's `PATH`. Probe
    /// with the namespaces and mounts every read-only launch requests. A
    /// launcher that has not finished within five seconds is unusable.
    pub(super) fn locate() -> Self {
        let system = PathBuf::from("/usr/bin/bwrap");
        let launcher = if system.is_file() {
            system
        } else {
            which::which("bwrap").unwrap_or(system)
        };
        let usable = cfg!(target_os = "linux")
            && std::process::Command::new(&launcher)
                .env_clear()
                .args(NAMESPACE_ARGS)
                .arg("--")
                .arg(&launcher)
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .is_ok_and(|mut probe| {
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                    loop {
                        match probe.try_wait() {
                            Ok(Some(status)) => return status.success(),
                            Ok(None) if std::time::Instant::now() < deadline => {
                                std::thread::sleep(std::time::Duration::from_millis(5));
                            }
                            Ok(None) | Err(_) => {
                                let _ = probe.kill();
                                let _ = probe.wait();
                                return false;
                            }
                        }
                    }
                });
        Self { launcher, usable }
    }
}

#[cfg(not(target_os = "linux"))]
pub(super) fn command(
    _: &LaunchSpec,
    _: &Path,
    _: &Path,
    _: Option<&Path>,
    _: Option<[std::fs::File; 2]>,
    _: &Path,
) -> anyhow::Result<Command> {
    anyhow::bail!("read-only workspace enforcement unavailable on this platform")
}

#[cfg(target_os = "linux")]
pub(super) fn command(
    spec: &LaunchSpec,
    cwd: &Path,
    home: &Path,
    runtime_dir: Option<&Path>,
    side_fds: Option<[std::fs::File; 2]>,
    launcher: &Path,
) -> anyhow::Result<Command> {
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
    let private_codex = (side_fds.is_some() && spec.uses_private_codex_home())
        .then(|| Path::new("/dev/shm").join(format!("codex-{}", uuid::Uuid::new_v4())));
    for (key, value) in &spec.env {
        if private_codex.is_none() || super::codex_home::SIDE_ENV.contains(&key.as_str()) {
            set_env(key, OsStr::new(value))?;
        }
    }
    if let Some(dir) = runtime_dir {
        ensure!(
            private_codex.is_none(),
            "prepared runtimes are unsupported for a private Codex side"
        );
        set_env(super::RUNTIME_ENV, dir.as_os_str())?;
    }
    set_env("HOME", home.as_os_str())?;
    if let Some(private) = &private_codex {
        super::codex_home::validate_launch(spec)?;
        set_env("CODEX_HOME", private.as_os_str())?;
        set_env("CODEX_SQLITE_HOME", private.as_os_str())?;
    } else {
        set_env("CODEX_HOME", home.join(".codex").as_os_str())?;
    }
    set_env("NORI_HOME", home.join(".nori/cli").as_os_str())?;
    set_env("TMPDIR", OsStr::new("/dev/shm"))?;
    settings.rewind()?;

    let mut command = Command::new(launcher);
    command.env_clear().args(NAMESPACE_ARGS);
    let mut descriptors = match private_codex {
        Some(private) => super::codex_home::mount_inputs(
            &mut command,
            home,
            &private,
            spec.has_subscription_file(),
        )?,
        None => Vec::new(),
    };
    if let Some([info, gate]) = side_fds {
        command
            .arg("--info-fd")
            .arg(info.as_raw_fd().to_string())
            .arg("--block-fd")
            .arg(gate.as_raw_fd().to_string());
        descriptors.extend([info, gate]);
    }
    ensure!(
        launcher.is_file(),
        "read-only workspace enforcement requires {}",
        launcher.display()
    );
    command
        .arg("--chdir")
        .arg(cwd)
        .arg("--args")
        .arg(settings.as_raw_fd().to_string())
        .arg("--")
        .arg(&spec.adapter.path)
        .args(&spec.adapter.args)
        .current_dir(cwd)
        .stderr(Stdio::null());
    descriptors.push(settings);
    // SAFETY: fcntl is async-signal-safe. The closure owns the open descriptors
    // until spawn returns; Bubblewrap consumes them before executing the adapter.
    unsafe {
        command.pre_exec(move || {
            for file in &descriptors {
                if libc::fcntl(file.as_raw_fd(), libc::F_SETFD, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    Ok(command)
}
