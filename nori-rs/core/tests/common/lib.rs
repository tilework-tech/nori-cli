#![expect(clippy::expect_used)]

use tempfile::TempDir;

use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::config::ConfigToml;
use regex_lite::Regex;

#[cfg(target_os = "linux")]
use assert_cmd::cargo::cargo_bin;

#[track_caller]
pub fn assert_regex_match<'s>(pattern: &str, actual: &'s str) -> regex_lite::Captures<'s> {
    let regex = Regex::new(pattern).unwrap_or_else(|err| {
        panic!("failed to compile regex {pattern:?}: {err}");
    });
    regex
        .captures(actual)
        .unwrap_or_else(|| panic!("regex {pattern:?} did not match {actual:?}"))
}

/// Returns a default `Config` whose on-disk state is confined to the provided
/// temporary directory. Using a per-test directory keeps tests hermetic and
/// avoids clobbering a developer's real `~/.codex`.
pub fn load_default_config_for_test(codex_home: &TempDir) -> Config {
    let mut config = Config::load_from_base_config_with_overrides(
        ConfigToml::default(),
        default_test_overrides(),
        codex_home.path().to_path_buf(),
    )
    .expect("defaults for test should always succeed");

    // Disable notifications by default in tests to prevent live desktop notifications.
    config.notify = Some(vec![]);

    config
}

#[cfg(target_os = "linux")]
fn default_test_overrides() -> ConfigOverrides {
    ConfigOverrides {
        codex_linux_sandbox_exe: Some(cargo_bin("codex-linux-sandbox")),
        ..ConfigOverrides::default()
    }
}

#[cfg(not(target_os = "linux"))]
fn default_test_overrides() -> ConfigOverrides {
    ConfigOverrides::default()
}

pub fn sandbox_env_var() -> &'static str {
    codex_core::spawn::CODEX_SANDBOX_ENV_VAR
}

pub fn sandbox_network_env_var() -> &'static str {
    codex_core::spawn::CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR
}

pub fn format_with_current_shell(command: &str) -> Vec<String> {
    codex_core::shell::default_user_shell().derive_exec_args(command, true)
}

pub fn format_with_current_shell_display(command: &str) -> String {
    let args = format_with_current_shell(command);
    shlex::try_join(args.iter().map(String::as_str)).expect("serialize current shell command")
}

pub mod fs_wait {
    use anyhow::Result;
    use anyhow::anyhow;
    use notify::RecursiveMode;
    use notify::Watcher;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;
    use std::time::Instant;
    use tokio::task;
    use walkdir::WalkDir;

    pub async fn wait_for_path_exists(
        path: impl Into<PathBuf>,
        timeout: Duration,
    ) -> Result<PathBuf> {
        let path = path.into();
        task::spawn_blocking(move || wait_for_path_exists_blocking(path, timeout)).await?
    }

    pub async fn wait_for_matching_file(
        root: impl Into<PathBuf>,
        timeout: Duration,
        predicate: impl FnMut(&Path) -> bool + Send + 'static,
    ) -> Result<PathBuf> {
        let root = root.into();
        task::spawn_blocking(move || {
            let mut predicate = predicate;
            blocking_find_matching_file(root, timeout, &mut predicate)
        })
        .await?
    }

    fn wait_for_path_exists_blocking(path: PathBuf, timeout: Duration) -> Result<PathBuf> {
        if path.exists() {
            return Ok(path);
        }

        let watch_root = nearest_existing_ancestor(&path);
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })?;
        watcher.watch(&watch_root, RecursiveMode::Recursive)?;

        let deadline = Instant::now() + timeout;
        loop {
            if path.exists() {
                return Ok(path.clone());
            }
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let remaining = deadline.saturating_duration_since(now);
            match rx.recv_timeout(remaining) {
                Ok(Ok(_event)) => {
                    if path.exists() {
                        return Ok(path.clone());
                    }
                }
                Ok(Err(err)) => return Err(err.into()),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        if path.exists() {
            Ok(path)
        } else {
            Err(anyhow!("timed out waiting for {path:?}"))
        }
    }

    fn blocking_find_matching_file(
        root: PathBuf,
        timeout: Duration,
        predicate: &mut impl FnMut(&Path) -> bool,
    ) -> Result<PathBuf> {
        let root = wait_for_path_exists_blocking(root, timeout)?;

        if let Some(found) = scan_for_match(&root, predicate) {
            return Ok(found);
        }

        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })?;
        watcher.watch(&root, RecursiveMode::Recursive)?;

        let deadline = Instant::now() + timeout;

        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remaining) {
                Ok(Ok(_event)) => {
                    if let Some(found) = scan_for_match(&root, predicate) {
                        return Ok(found);
                    }
                }
                Ok(Err(err)) => return Err(err.into()),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        if let Some(found) = scan_for_match(&root, predicate) {
            Ok(found)
        } else {
            Err(anyhow!("timed out waiting for matching file in {root:?}"))
        }
    }

    fn scan_for_match(root: &Path, predicate: &mut impl FnMut(&Path) -> bool) -> Option<PathBuf> {
        for entry in WalkDir::new(root).into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if !entry.file_type().is_file() {
                continue;
            }
            if predicate(path) {
                return Some(path.to_path_buf());
            }
        }
        None
    }

    fn nearest_existing_ancestor(path: &Path) -> PathBuf {
        let mut current = path;
        loop {
            if current.exists() {
                return current.to_path_buf();
            }
            match current.parent() {
                Some(parent) => current = parent,
                None => return PathBuf::from("."),
            }
        }
    }
}

#[macro_export]
macro_rules! skip_if_sandbox {
    () => {{
        if ::std::env::var($crate::sandbox_env_var())
            == ::core::result::Result::Ok("seatbelt".to_string())
        {
            eprintln!(
                "{} is set to 'seatbelt', skipping test.",
                $crate::sandbox_env_var()
            );
            return;
        }
    }};
    ($return_value:expr $(,)?) => {{
        if ::std::env::var($crate::sandbox_env_var())
            == ::core::result::Result::Ok("seatbelt".to_string())
        {
            eprintln!(
                "{} is set to 'seatbelt', skipping test.",
                $crate::sandbox_env_var()
            );
            return $return_value;
        }
    }};
}

#[macro_export]
macro_rules! skip_if_no_network {
    () => {{
        if ::std::env::var($crate::sandbox_network_env_var()).is_ok() {
            println!(
                "Skipping test because it cannot execute when network is disabled in a Codex sandbox."
            );
            return;
        }
    }};
    ($return_value:expr $(,)?) => {{
        if ::std::env::var($crate::sandbox_network_env_var()).is_ok() {
            println!(
                "Skipping test because it cannot execute when network is disabled in a Codex sandbox."
            );
            return $return_value;
        }
    }};
}
