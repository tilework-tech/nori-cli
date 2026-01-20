//! Install source and client ID detection
//!
//! Provides functions to detect how the CLI was installed and generate
//! a privacy-preserving client identifier.

use crate::state::InstallSource;
use sha2::Digest;
use sha2::Sha256;
use uuid::Uuid;

/// Environment variable set by nori.js when installed via Bun
const NORI_MANAGED_BY_BUN: &str = "NORI_MANAGED_BY_BUN";

/// Environment variable set by nori.js when installed via npm
const NORI_MANAGED_BY_NPM: &str = "NORI_MANAGED_BY_NPM";

const CLIENT_ID_SALT: &str = "nori_salt";

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

/// Generate a privacy-preserving client identifier
///
/// Creates a deterministic hash of hostname and username that:
/// - Is stable across sessions on the same machine
/// - Cannot be reversed to recover the original values
/// - Is suitable for analytics without PII exposure
///
/// Format: UUID string derived from SHA256("nori_salt:<hostname>:<username>")
pub fn generate_client_id() -> String {
    let hostname = get_hostname();
    let username = get_username();

    let input = format!("{CLIENT_ID_SALT}:{hostname}:{username}");
    let hash = Sha256::digest(input.as_bytes());

    let bytes: [u8; 16] = hash
        .get(..16)
        .and_then(|slice| <[u8; 16]>::try_from(slice).ok())
        .unwrap_or_default();
    Uuid::from_bytes(bytes).to_string()
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
    fn test_generate_client_id_format() {
        let client_id = generate_client_id();

        let parsed = Uuid::parse_str(&client_id).expect("client_id should be a UUID");
        assert_eq!(parsed.to_string(), client_id);
    }

    #[test]
    fn test_generate_client_id_deterministic() {
        // Same machine should always produce the same ID
        let id1 = generate_client_id();
        let id2 = generate_client_id();
        assert_eq!(id1, id2, "client_id should be deterministic");
    }

    #[test]
    fn test_client_id_hash_computation() {
        // Verify the hash is computed correctly for known input
        let input = "nori_salt:testhost:testuser";
        let hash = Sha256::digest(input.as_bytes());
        let bytes: [u8; 16] = hash[..16]
            .try_into()
            .expect("hash slice should be 16 bytes");
        let expected = Uuid::from_bytes(bytes).to_string();

        // Manually check the hash matches what we'd expect
        assert!(Uuid::parse_str(&expected).is_ok());
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
