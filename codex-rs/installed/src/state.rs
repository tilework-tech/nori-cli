//! Install state persistence
//!
//! Manages the `.nori-install.json` file that tracks CLI lifecycle.

use chrono::DateTime;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;
use std::path::Path;
use std::path::PathBuf;

/// The filename for the install state file
pub const INSTALL_STATE_FILENAME: &str = ".nori-install.json";

/// Current schema version
pub const SCHEMA_VERSION: u32 = 1;

/// Client ID for nori-cli
pub const CLIENT_ID: &str = "nori-cli";

/// Install source detection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallSource {
    Npm,
    Bun,
    Unknown,
}

impl Default for InstallSource {
    fn default() -> Self {
        Self::Unknown
    }
}

/// The install state stored in `.nori-install.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallState {
    /// Schema version for forward compatibility
    pub schema_version: u32,

    /// Client identifier (always "nori-cli")
    pub client_id: String,

    /// Privacy-protecting user identifier: sha256(hostname:username)
    pub user_id: String,

    /// Timestamp of first launch (immutable after creation)
    pub first_installed_at: DateTime<Utc>,

    /// Timestamp when installed_version last changed
    pub last_updated_at: DateTime<Utc>,

    /// Timestamp of most recent launch
    pub last_launched_at: DateTime<Utc>,

    /// Current CLI version
    pub installed_version: String,

    /// How the CLI was installed
    pub install_source: InstallSource,
}

impl InstallState {
    /// Create a new install state for first-time installation
    pub fn new_first_install(
        user_id: String,
        version: String,
        source: InstallSource,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            client_id: CLIENT_ID.to_string(),
            user_id,
            first_installed_at: now,
            last_updated_at: now,
            last_launched_at: now,
            installed_version: version,
            install_source: source,
        }
    }

    /// Update state for a version upgrade
    pub fn record_upgrade(
        &mut self,
        new_version: String,
        source: InstallSource,
        now: DateTime<Utc>,
    ) {
        self.installed_version = new_version;
        self.install_source = source;
        self.last_updated_at = now;
        self.last_launched_at = now;
    }

    /// Update state for a normal session launch
    pub fn record_session(&mut self, now: DateTime<Utc>) {
        self.last_launched_at = now;
    }

    /// Calculate days since first install
    pub fn days_since_install(&self, now: DateTime<Utc>) -> i64 {
        (now - self.first_installed_at).num_days()
    }
}

/// Get the path to the install state file
pub fn install_state_path(nori_home: &Path) -> PathBuf {
    nori_home.join(INSTALL_STATE_FILENAME)
}

/// Read the install state from disk
///
/// Returns `None` if the file doesn't exist or can't be parsed.
pub fn read_install_state(nori_home: &Path) -> Option<InstallState> {
    let path = install_state_path(nori_home);
    let contents = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&contents).ok()
}

/// Write the install state to disk atomically
///
/// Uses a temp file + rename pattern to avoid partial writes.
pub async fn write_install_state(nori_home: &Path, state: &InstallState) -> anyhow::Result<()> {
    let path = install_state_path(nori_home);

    // Ensure the directory exists
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Serialize to JSON with trailing newline
    let json = serde_json::to_string_pretty(state)?;
    let content = format!("{json}\n");

    // Write to a temp file first, then rename for atomicity
    let temp_path = path.with_extension("json.tmp");
    tokio::fs::write(&temp_path, &content).await?;
    tokio::fs::rename(&temp_path, &path).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_install_state_serialization() {
        let now = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let state = InstallState::new_first_install(
            "sha256:abc123".to_string(),
            "1.0.0".to_string(),
            InstallSource::Bun,
            now,
        );

        let json = serde_json::to_string(&state).expect("serialization failed");
        assert!(json.contains("\"schema_version\":1"));
        assert!(json.contains("\"client_id\":\"nori-cli\""));
        assert!(json.contains("\"user_id\":\"sha256:abc123\""));
        assert!(json.contains("\"installed_version\":\"1.0.0\""));
        assert!(json.contains("\"install_source\":\"bun\""));
    }

    #[test]
    fn test_install_state_deserialization() {
        let json = r#"{
            "schema_version": 1,
            "client_id": "nori-cli",
            "user_id": "sha256:def456",
            "first_installed_at": "2025-01-15T10:30:00Z",
            "last_updated_at": "2025-01-20T14:22:00Z",
            "last_launched_at": "2025-01-21T09:00:00Z",
            "installed_version": "1.2.3",
            "install_source": "npm"
        }"#;

        let state: InstallState = serde_json::from_str(json).expect("deserialization failed");
        assert_eq!(state.schema_version, 1);
        assert_eq!(state.client_id, "nori-cli");
        assert_eq!(state.user_id, "sha256:def456");
        assert_eq!(state.installed_version, "1.2.3");
        assert_eq!(state.install_source, InstallSource::Npm);
    }

    #[test]
    fn test_install_source_variants() {
        assert_eq!(
            serde_json::to_string(&InstallSource::Npm).expect("failed"),
            "\"npm\""
        );
        assert_eq!(
            serde_json::to_string(&InstallSource::Bun).expect("failed"),
            "\"bun\""
        );
        assert_eq!(
            serde_json::to_string(&InstallSource::Unknown).expect("failed"),
            "\"unknown\""
        );
    }

    #[test]
    fn test_record_upgrade() {
        let initial = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let upgrade_time = Utc.with_ymd_and_hms(2025, 1, 15, 12, 0, 0).unwrap();

        let mut state = InstallState::new_first_install(
            "sha256:test".to_string(),
            "1.0.0".to_string(),
            InstallSource::Npm,
            initial,
        );

        state.record_upgrade("2.0.0".to_string(), InstallSource::Bun, upgrade_time);

        assert_eq!(state.installed_version, "2.0.0");
        assert_eq!(state.install_source, InstallSource::Bun);
        assert_eq!(state.last_updated_at, upgrade_time);
        assert_eq!(state.last_launched_at, upgrade_time);
        // first_installed_at should remain unchanged
        assert_eq!(state.first_installed_at, initial);
    }

    #[test]
    fn test_record_session() {
        let initial = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let session_time = Utc.with_ymd_and_hms(2025, 1, 10, 8, 0, 0).unwrap();

        let mut state = InstallState::new_first_install(
            "sha256:test".to_string(),
            "1.0.0".to_string(),
            InstallSource::Unknown,
            initial,
        );

        state.record_session(session_time);

        assert_eq!(state.last_launched_at, session_time);
        // Other fields should remain unchanged
        assert_eq!(state.first_installed_at, initial);
        assert_eq!(state.last_updated_at, initial);
        assert_eq!(state.installed_version, "1.0.0");
    }

    #[test]
    fn test_days_since_install() {
        let install_time = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2025, 1, 6, 0, 0, 0).unwrap();

        let state = InstallState::new_first_install(
            "sha256:test".to_string(),
            "1.0.0".to_string(),
            InstallSource::Bun,
            install_time,
        );

        assert_eq!(state.days_since_install(now), 5);
    }

    #[test]
    fn test_install_state_path() {
        let home = PathBuf::from("/home/user/.nori/cli");
        let path = install_state_path(&home);
        assert_eq!(
            path,
            PathBuf::from("/home/user/.nori/cli/.nori-install.json")
        );
    }

    #[tokio::test]
    async fn test_write_and_read_install_state() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let now = Utc::now();

        let state = InstallState::new_first_install(
            "sha256:testuser".to_string(),
            "1.0.0".to_string(),
            InstallSource::Npm,
            now,
        );

        // Write state
        write_install_state(temp_dir.path(), &state)
            .await
            .expect("write failed");

        // Read it back
        let loaded = read_install_state(temp_dir.path()).expect("read failed");

        assert_eq!(loaded.schema_version, state.schema_version);
        assert_eq!(loaded.client_id, state.client_id);
        assert_eq!(loaded.user_id, state.user_id);
        assert_eq!(loaded.installed_version, state.installed_version);
        assert_eq!(loaded.install_source, state.install_source);
    }

    #[test]
    fn test_read_nonexistent_file() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let result = read_install_state(temp_dir.path());
        assert!(result.is_none());
    }

    #[test]
    fn test_read_corrupted_file() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = install_state_path(temp_dir.path());

        // Write invalid JSON
        std::fs::write(&path, "not valid json").expect("write failed");

        let result = read_install_state(temp_dir.path());
        assert!(result.is_none());
    }
}
