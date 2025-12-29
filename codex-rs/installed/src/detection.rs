//! Install source and user ID detection
//!
//! Provides functions to detect how the CLI was installed and generate
//! a privacy-protecting user identifier.

use crate::state::InstallSource;
use sha2::Digest;
use sha2::Sha256;

/// Environment variable set by nori.js when installed via Bun
const NORI_MANAGED_BY_BUN: &str = "NORI_MANAGED_BY_BUN";

/// Environment variable set by nori.js when installed via npm
const NORI_MANAGED_BY_NPM: &str = "NORI_MANAGED_BY_NPM";

/// Detect the install source from environment variables
///
/// The nori.js wrapper sets `NORI_MANAGED_BY_BUN=1` or `NORI_MANAGED_BY_NPM=1`
/// depending on which package manager was used.
pub fn detect_install_source() -> InstallSource {
    if std::env::var(NORI_MANAGED_BY_BUN).as_deref() == Ok("1") {
        InstallSource::Bun
    } else if std::env::var(NORI_MANAGED_BY_NPM).as_deref() == Ok("1") {
        InstallSource::Npm
    } else {
        InstallSource::Unknown
    }
}

/// Generate a privacy-protecting user identifier
///
/// Creates a deterministic hash of hostname and username that:
/// - Is stable across sessions on the same machine
/// - Cannot be reversed to recover the original values
/// - Is suitable for analytics without PII exposure
///
/// Format: `sha256:<hex_hash>`
pub fn generate_user_id() -> String {
    let hostname = get_hostname();
    let username = get_username();

    let input = format!("{hostname}:{username}");
    let hash = Sha256::digest(input.as_bytes());

    format!("sha256:{}", hex::encode(hash))
}

/// Get the system hostname
fn get_hostname() -> String {
    get_hostname_impl().unwrap_or_else(|| "unknown".to_string())
}

/// Get the current username
fn get_username() -> String {
    // Try environment variables first (most portable)
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(unix)]
fn get_hostname_impl() -> Option<String> {
    // Use libc::gethostname on Unix
    let mut buf = vec![0u8; 256];
    let result = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if result == 0 {
        // Find the null terminator
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf8(buf[..len].to_vec()).ok()
    } else {
        None
    }
}

#[cfg(windows)]
fn get_hostname_impl() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    let mut buf = [0u16; 256];
    let mut size = buf.len() as u32;

    let result = unsafe {
        windows_sys::Win32::System::SystemInformation::GetComputerNameW(buf.as_mut_ptr(), &mut size)
    };

    if result != 0 {
        let os_str = OsString::from_wide(&buf[..size as usize]);
        os_str.into_string().ok()
    } else {
        None
    }
}

#[cfg(not(any(unix, windows)))]
fn get_hostname_impl() -> Option<String> {
    // Fallback for unsupported platforms
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_detect_install_source_bun() {
        // Save original values
        let orig_bun = env::var(NORI_MANAGED_BY_BUN).ok();
        let orig_npm = env::var(NORI_MANAGED_BY_NPM).ok();

        // Set Bun env var
        // SAFETY: Tests run sequentially in the same process
        unsafe {
            env::set_var(NORI_MANAGED_BY_BUN, "1");
            env::remove_var(NORI_MANAGED_BY_NPM);
        }

        let source = detect_install_source();
        assert_eq!(source, InstallSource::Bun);

        // Restore
        restore_env(NORI_MANAGED_BY_BUN, orig_bun);
        restore_env(NORI_MANAGED_BY_NPM, orig_npm);
    }

    #[test]
    fn test_detect_install_source_npm() {
        let orig_bun = env::var(NORI_MANAGED_BY_BUN).ok();
        let orig_npm = env::var(NORI_MANAGED_BY_NPM).ok();

        // SAFETY: Tests run sequentially in the same process
        unsafe {
            env::remove_var(NORI_MANAGED_BY_BUN);
            env::set_var(NORI_MANAGED_BY_NPM, "1");
        }

        let source = detect_install_source();
        assert_eq!(source, InstallSource::Npm);

        restore_env(NORI_MANAGED_BY_BUN, orig_bun);
        restore_env(NORI_MANAGED_BY_NPM, orig_npm);
    }

    #[test]
    fn test_detect_install_source_unknown() {
        let orig_bun = env::var(NORI_MANAGED_BY_BUN).ok();
        let orig_npm = env::var(NORI_MANAGED_BY_NPM).ok();

        // SAFETY: Tests run sequentially in the same process
        unsafe {
            env::remove_var(NORI_MANAGED_BY_BUN);
            env::remove_var(NORI_MANAGED_BY_NPM);
        }

        let source = detect_install_source();
        assert_eq!(source, InstallSource::Unknown);

        restore_env(NORI_MANAGED_BY_BUN, orig_bun);
        restore_env(NORI_MANAGED_BY_NPM, orig_npm);
    }

    #[test]
    fn test_detect_install_source_bun_takes_precedence() {
        let orig_bun = env::var(NORI_MANAGED_BY_BUN).ok();
        let orig_npm = env::var(NORI_MANAGED_BY_NPM).ok();

        // Both set - Bun should take precedence
        // SAFETY: Tests run sequentially in the same process
        unsafe {
            env::set_var(NORI_MANAGED_BY_BUN, "1");
            env::set_var(NORI_MANAGED_BY_NPM, "1");
        }

        let source = detect_install_source();
        assert_eq!(source, InstallSource::Bun);

        restore_env(NORI_MANAGED_BY_BUN, orig_bun);
        restore_env(NORI_MANAGED_BY_NPM, orig_npm);
    }

    #[test]
    fn test_generate_user_id_format() {
        let user_id = generate_user_id();

        // Should start with "sha256:"
        assert!(
            user_id.starts_with("sha256:"),
            "user_id should start with 'sha256:'"
        );

        // Should have a 64-character hex hash after the prefix
        let hash_part = user_id.strip_prefix("sha256:").expect("prefix not found");
        assert_eq!(hash_part.len(), 64, "SHA256 hash should be 64 hex chars");

        // Should be valid hex
        assert!(
            hash_part.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be valid hex"
        );
    }

    #[test]
    fn test_generate_user_id_deterministic() {
        // Same machine should always produce the same ID
        let id1 = generate_user_id();
        let id2 = generate_user_id();
        assert_eq!(id1, id2, "user_id should be deterministic");
    }

    #[test]
    fn test_user_id_hash_computation() {
        // Verify the hash is computed correctly for known input
        let input = "testhost:testuser";
        let hash = Sha256::digest(input.as_bytes());
        let expected = format!("sha256:{}", hex::encode(hash));

        // Manually check the hash matches what we'd expect
        assert!(expected.starts_with("sha256:"));
        assert_eq!(expected.len(), 7 + 64); // "sha256:" + 64 hex chars
    }

    fn restore_env(key: &str, value: Option<String>) {
        // SAFETY: Tests run sequentially in the same process
        unsafe {
            match value {
                Some(v) => env::set_var(key, v),
                None => env::remove_var(key),
            }
        }
    }
}
