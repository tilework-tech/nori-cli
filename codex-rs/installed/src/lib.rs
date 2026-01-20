//! Install tracking for nori-cli
//!
//! This crate tracks CLI lifecycle events (first install, version updates, sessions)
//! by maintaining a state file at `$NORI_HOME/.nori-install.json`.
//!
//! The tracker runs non-blocking on every CLI launch and can emit analytics events
//! to help understand CLI usage patterns.
//!
//! # Usage
//!
//! Call `track_launch()` early in the CLI startup:
//!
//! ```ignore
//! use nori_installed::track_launch;
//! use std::path::Path;
//!
//! // Non-blocking, fire-and-forget
//! track_launch(Path::new("/home/user/.nori/cli"));
//! ```

mod analytics;
mod detection;
mod state;

pub use analytics::ANALYTICS_OPT_OUT_ENV;
pub use analytics::AnalyticsEventType;
pub use analytics::TrackEventRequest;
pub use analytics::create_event;
pub use analytics::send_event;
pub use detection::detect_install_source;
pub use detection::generate_client_id;
pub use state::InstallSource;
pub use state::InstallState;
pub use state::read_install_state;
pub use state::write_install_state;

use chrono::Duration;
use chrono::Utc;
use semver::Version;
use std::path::Path;
use tracing::debug;
use uuid::Uuid;

/// The current CLI version from Cargo.toml
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Result of tracking a launch
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchEvent {
    /// First time installation
    AppInstall,
    /// Version was upgraded
    AppUpdate { previous_version: String },
    /// Start of a CLI session
    SessionStart,
    /// User returned after churn
    UserResurrected,
}

/// Track a CLI launch. Call this early in main().
///
/// This function spawns a background task to:
/// 1. Read or create the install state file
/// 2. Determine if this is a first install, upgrade, or normal session
/// 3. Update the state file
/// 4. Send analytics events (fire-and-forget)
///
/// The function returns immediately and never blocks startup.
/// All errors are silently logged at debug level.
pub fn track_launch(nori_home: &Path) {
    let nori_home = nori_home.to_path_buf();
    tokio::spawn(async move {
        if let Err(e) = track_launch_inner(&nori_home).await {
            debug!("Install tracking failed: {e}");
        }
    });
}

/// Internal implementation of launch tracking
async fn track_launch_inner(nori_home: &Path) -> anyhow::Result<Vec<LaunchEvent>> {
    let now = Utc::now();
    let current_version = CLI_VERSION;
    let install_source = detect_install_source();
    let client_id = generate_client_id();
    let session_id = Uuid::new_v4().to_string();

    // Read existing state or treat missing/corrupt file as first install
    let existing_state = read_install_state(nori_home);

    let (events, new_state) = match existing_state {
        None => {
            // First install
            debug!("First install detected, creating install state");
            let state = InstallState::new_first_install(
                client_id,
                current_version.to_string(),
                install_source,
                now,
            );
            (
                vec![LaunchEvent::AppInstall, LaunchEvent::SessionStart],
                state,
            )
        }
        Some(mut state) => {
            if !is_valid_uuid(&state.client_id) {
                state.client_id = generate_client_id();
            }

            let resurrected = is_resurrected(&state, now);
            let mut events = Vec::new();

            if is_semver_upgrade(current_version, &state.installed_version) {
                // Version upgrade
                let previous = state.installed_version.clone();
                debug!(
                    "Version upgrade detected: {} -> {}",
                    previous, current_version
                );
                state.record_upgrade(current_version.to_string(), install_source, now);
                events.push(LaunchEvent::AppUpdate {
                    previous_version: previous,
                });
            } else {
                // Normal session
                state.record_session(now);
            }

            if resurrected {
                events.push(LaunchEvent::UserResurrected);
            }

            events.push(LaunchEvent::SessionStart);

            (events, state)
        }
    };

    // Write updated state
    write_install_state(nori_home, &new_state).await?;

    // Send analytics events (fire-and-forget)
    if !should_skip_analytics(&new_state) {
        let days_since_install = new_state.days_since_install(now);
        for event in &events {
            let (event_type, is_first_install, previous_version) = match event {
                LaunchEvent::AppInstall => (AnalyticsEventType::InstallCompleted, true, None),
                LaunchEvent::AppUpdate { previous_version } => (
                    AnalyticsEventType::InstallCompleted,
                    false,
                    Some(previous_version.clone()),
                ),
                LaunchEvent::SessionStart => (AnalyticsEventType::SessionStart, false, None),
                LaunchEvent::UserResurrected => (AnalyticsEventType::UserResurrected, false, None),
            };

            let analytics_event = create_event(
                event_type,
                &new_state,
                &session_id,
                now,
                days_since_install,
                is_first_install,
                previous_version,
            );
            send_event(&analytics_event).await;
        }
    }

    debug!("Install tracking complete: {events:?}");

    Ok(events)
}

/// Synchronous version of launch tracking for testing
///
/// This is mainly useful for unit tests where we want to verify
/// the tracking behavior without async runtime complexity.
#[doc(hidden)]
pub fn track_launch_sync(nori_home: &Path) -> anyhow::Result<LaunchEvent> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let events = rt.block_on(track_launch_inner(nori_home))?;
    Ok(events.last().cloned().unwrap_or(LaunchEvent::SessionStart))
}

fn should_skip_analytics(state: &InstallState) -> bool {
    std::env::var(ANALYTICS_OPT_OUT_ENV).as_deref() == Ok("1") || state.opt_out
}

fn is_valid_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok()
}

fn is_semver_upgrade(current_version: &str, installed_version: &str) -> bool {
    let Ok(current) = Version::parse(current_version) else {
        return current_version != installed_version;
    };
    let Ok(installed) = Version::parse(installed_version) else {
        return current_version != installed_version;
    };
    current > installed
}

fn is_resurrected(state: &InstallState, now: chrono::DateTime<Utc>) -> bool {
    let diff = now - state.last_launched_at;
    diff > Duration::days(30)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_temp_home() -> TempDir {
        tempfile::tempdir().expect("failed to create temp dir")
    }

    fn track_launch_events(nori_home: &Path) -> Vec<LaunchEvent> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime build failed");
        rt.block_on(track_launch_inner(nori_home))
            .expect("tracking failed")
    }

    #[test]
    fn test_first_install() {
        let temp_home = setup_temp_home();

        let events = track_launch_events(temp_home.path());

        assert_eq!(
            events,
            vec![LaunchEvent::AppInstall, LaunchEvent::SessionStart]
        );

        // Verify state file was created
        let state = read_install_state(temp_home.path()).expect("state should exist");
        assert_eq!(state.installed_version, CLI_VERSION);
        assert!(Uuid::parse_str(&state.client_id).is_ok());
        assert!(!state.opt_out);
    }

    #[test]
    fn test_normal_session() {
        let temp_home = setup_temp_home();

        // First launch
        let event1 = track_launch_events(temp_home.path());
        assert_eq!(
            event1,
            vec![LaunchEvent::AppInstall, LaunchEvent::SessionStart]
        );

        // Second launch - should be a normal session
        let event2 = track_launch_events(temp_home.path());

        assert_eq!(event2, vec![LaunchEvent::SessionStart]);
    }

    #[test]
    fn test_version_upgrade() {
        let temp_home = setup_temp_home();

        // Create a state file with an older version
        let now = Utc::now();
        let old_version = match Version::parse(CLI_VERSION) {
            Ok(mut current) => {
                if current.major > 0 {
                    current.major -= 1;
                    current.minor = 0;
                    current.patch = 0;
                    current.to_string()
                } else if current.minor > 0 {
                    current.minor -= 1;
                    current.patch = 0;
                    current.to_string()
                } else if current.patch > 0 {
                    current.patch -= 1;
                    current.to_string()
                } else {
                    "0.0.0-alpha".to_string()
                }
            }
            Err(_) => "0.0.0".to_string(),
        };
        let old_state = InstallState::new_first_install(
            generate_client_id(),
            old_version.clone(),
            InstallSource::Npm,
            now,
        );

        // Write the old state
        let state_path = temp_home.path().join(".nori-install.json");
        let json = serde_json::to_string_pretty(&old_state).expect("serialize failed");
        fs::write(&state_path, format!("{json}\n")).expect("write failed");

        // Track launch with current version
        let events = track_launch_events(temp_home.path());

        assert_eq!(
            events,
            vec![
                LaunchEvent::AppUpdate {
                    previous_version: old_version,
                },
                LaunchEvent::SessionStart
            ]
        );

        // Verify state was updated
        let state = read_install_state(temp_home.path()).expect("state should exist");
        assert_eq!(state.installed_version, CLI_VERSION);
    }

    #[test]
    fn test_corrupt_file_treated_as_first_install() {
        let temp_home = setup_temp_home();

        // Write corrupt JSON
        let state_path = temp_home.path().join(".nori-install.json");
        fs::write(&state_path, "not valid json {{{").expect("write failed");

        // Should treat as first install
        let events = track_launch_events(temp_home.path());
        assert_eq!(
            events,
            vec![LaunchEvent::AppInstall, LaunchEvent::SessionStart]
        );

        // Verify state was recreated
        let state = read_install_state(temp_home.path()).expect("state should exist");
        assert_eq!(state.installed_version, CLI_VERSION);
    }

    #[test]
    fn test_client_id_persisted() {
        let temp_home = setup_temp_home();

        // First launch
        track_launch_events(temp_home.path());
        let state1 = read_install_state(temp_home.path()).expect("state should exist");

        // Second launch
        track_launch_events(temp_home.path());
        let state2 = read_install_state(temp_home.path()).expect("state should exist");

        // Client ID should remain the same
        assert_eq!(state1.client_id, state2.client_id);
    }

    #[test]
    fn test_first_installed_at_immutable() {
        let temp_home = setup_temp_home();

        // First launch
        track_launch_events(temp_home.path());
        let state1 = read_install_state(temp_home.path()).expect("state should exist");
        let first_installed = state1.first_installed_at;

        // Second launch
        track_launch_events(temp_home.path());
        let state2 = read_install_state(temp_home.path()).expect("state should exist");

        // first_installed_at should not change
        assert_eq!(state2.first_installed_at, first_installed);
    }

    #[test]
    fn test_last_launched_at_updated() {
        let temp_home = setup_temp_home();

        // First launch
        track_launch_events(temp_home.path());
        let state1 = read_install_state(temp_home.path()).expect("state should exist");

        // Small delay to ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Second launch
        track_launch_events(temp_home.path());
        let state2 = read_install_state(temp_home.path()).expect("state should exist");

        // last_launched_at should be updated
        assert!(state2.last_launched_at >= state1.last_launched_at);
    }

    #[test]
    fn test_creates_directory_if_missing() {
        let temp_home = setup_temp_home();
        let nested_home = temp_home.path().join("nested").join("dir");

        // Directory doesn't exist yet
        assert!(!nested_home.exists());

        // Track launch should create it
        let events = track_launch_events(&nested_home);
        assert_eq!(
            events,
            vec![LaunchEvent::AppInstall, LaunchEvent::SessionStart]
        );

        // Verify directory and file were created
        assert!(nested_home.exists());
        assert!(read_install_state(&nested_home).is_some());
    }
}
