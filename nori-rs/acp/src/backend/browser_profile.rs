//! Chrome profile selection for the `/browser` command.
//!
//! [`BrowserProfileMode`](crate::config::BrowserProfileMode) chooses *which*
//! on-disk Chrome profile a browser session launches against. This module turns
//! that choice into a concrete `--user-data-dir` and owns the directory's
//! lifetime so `browser_session` only has to launch Chrome at the resolved path.

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context as _;
use anyhow::Result;

use crate::config::BrowserProfileMode;

/// Subdirectory of the nori home that holds the persistent browser profile.
const PERSISTENT_PROFILE_DIR: &str = "browser-profile";

/// A resolved Chrome `--user-data-dir` together with its cleanup policy.
///
/// `Throwaway` owns a [`tempfile::TempDir`] that is deleted when the session
/// drops (the secure default). `Keep` is a path we deliberately leave on disk
/// so logins survive across launches (persistent and system tiers).
pub enum ProfileDir {
    Throwaway(tempfile::TempDir),
    Keep(PathBuf),
}

impl ProfileDir {
    /// The directory to hand Chrome via `--user-data-dir`.
    pub fn path(&self) -> &Path {
        match self {
            Self::Throwaway(dir) => dir.path(),
            Self::Keep(path) => path,
        }
    }
}

/// Resolve the Chrome profile directory for `mode`.
///
/// `chrome` is the resolved browser binary; it selects the correct default
/// profile location for `System` (Chrome vs Chromium). `nori_home` is the
/// directory the persistent profile lives under, passed in so the caller owns
/// environment resolution.
pub fn resolve_profile_dir(
    mode: BrowserProfileMode,
    chrome: &Path,
    nori_home: &Path,
) -> Result<ProfileDir> {
    match mode {
        BrowserProfileMode::Throwaway => {
            let dir =
                tempfile::tempdir().context("failed to create temporary Chrome profile dir")?;
            Ok(ProfileDir::Throwaway(dir))
        }
        BrowserProfileMode::Persistent => {
            let dir = nori_home.join(PERSISTENT_PROFILE_DIR);
            std::fs::create_dir_all(&dir).with_context(|| {
                format!(
                    "failed to create persistent Chrome profile dir: {}",
                    dir.display()
                )
            })?;
            Ok(ProfileDir::Keep(dir))
        }
        BrowserProfileMode::System => {
            let home = dirs::home_dir().context("could not determine home directory")?;
            Ok(ProfileDir::Keep(system_chrome_profile_dir(chrome, &home)))
        }
    }
}

/// The user's real default Chrome (or Chromium) profile directory.
///
/// We never create this directory: it is the user's own profile and Chrome owns
/// its lifecycle. The binary name distinguishes Chromium from Chrome, and the
/// target OS selects the platform's standard profile location.
fn system_chrome_profile_dir(chrome: &Path, home: &Path) -> PathBuf {
    let is_chromium = chrome
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains("chromium"));

    if cfg!(target_os = "macos") {
        let base = home.join("Library").join("Application Support");
        if is_chromium {
            base.join("Chromium")
        } else {
            base.join("Google").join("Chrome")
        }
    } else {
        let base = home.join(".config");
        if is_chromium {
            base.join("chromium")
        } else {
            base.join("google-chrome")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn throwaway_creates_an_existing_temp_dir() {
        let nori_home = tempfile::tempdir().unwrap();
        let profile = resolve_profile_dir(
            BrowserProfileMode::Throwaway,
            Path::new("/usr/bin/google-chrome"),
            nori_home.path(),
        )
        .unwrap();

        assert!(matches!(profile, ProfileDir::Throwaway(_)));
        assert!(
            profile.path().exists(),
            "throwaway profile dir should exist"
        );
    }

    #[test]
    fn persistent_creates_dir_under_nori_home() {
        let nori_home = tempfile::tempdir().unwrap();
        let profile = resolve_profile_dir(
            BrowserProfileMode::Persistent,
            Path::new("/usr/bin/google-chrome"),
            nori_home.path(),
        )
        .unwrap();

        let expected = nori_home.path().join(PERSISTENT_PROFILE_DIR);
        match profile {
            ProfileDir::Keep(path) => assert_eq!(path, expected),
            ProfileDir::Throwaway(_) => panic!("persistent mode must keep its dir"),
        }
        assert!(
            expected.exists(),
            "persistent profile dir should be created"
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn system_dir_distinguishes_chrome_from_chromium_on_linux() {
        let home = Path::new("/home/tester");
        assert_eq!(
            system_chrome_profile_dir(Path::new("/usr/bin/google-chrome"), home),
            home.join(".config").join("google-chrome")
        );
        assert_eq!(
            system_chrome_profile_dir(Path::new("/usr/bin/chromium"), home),
            home.join(".config").join("chromium")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn system_dir_distinguishes_chrome_from_chromium_on_macos() {
        let home = Path::new("/Users/tester");
        assert_eq!(
            system_chrome_profile_dir(Path::new("/usr/bin/google-chrome"), home),
            home.join("Library")
                .join("Application Support")
                .join("Google")
                .join("Chrome")
        );
        assert_eq!(
            system_chrome_profile_dir(Path::new("/usr/bin/chromium"), home),
            home.join("Library")
                .join("Application Support")
                .join("Chromium")
        );
    }
}
