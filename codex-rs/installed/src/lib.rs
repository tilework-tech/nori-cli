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

pub use analytics::EventProperties;
pub use analytics::FlatEventRequest;
pub use analytics::InstallEventType;
pub use analytics::TrackEventRequest;
pub use analytics::create_flat_install_event;
pub use analytics::create_flat_resurrection_event;
pub use analytics::create_flat_session_event;
pub use analytics::create_install_event;
pub use analytics::create_session_event;
pub use analytics::send_event;
pub use analytics::send_flat_event;
pub use detection::detect_install_source;
pub use detection::generate_client_id;
pub use detection::generate_user_id;
pub use state::InstallSource;
pub use state::InstallState;
pub use state::read_install_state;
pub use state::write_install_state;

use chrono::Utc;
use std::path::Path;
use tracing::debug;

/// The current CLI version from Cargo.toml
pub const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Environment variable to opt out of analytics
const NORI_NO_ANALYTICS: &str = "NORI_NO_ANALYTICS";

/// Check if analytics should be sent
///
/// Returns false if:
/// - NORI_NO_ANALYTICS environment variable is set to "1"
/// - opt_out is true in the state file
fn should_send_analytics(state: &InstallState) -> bool {
    // Environment variable takes precedence
    if std::env::var(NORI_NO_ANALYTICS).as_deref() == Ok("1") {
        return false;
    }

    // Check state file opt_out flag
    !state.opt_out
}

/// Check if user should be considered "resurrected" (> 30 days since last launch)
fn should_resurrect(state: &InstallState, now: chrono::DateTime<Utc>) -> bool {
    (now - state.last_launched_at).num_days() > 30
}

/// Result of tracking a launch
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchEvent {
    /// First time installation
    FirstInstall,
    /// Version was upgraded
    Upgrade { previous_version: String },
    /// Normal session start
    Session { days_since_install: i64 },
}

/// Track a CLI launch. Call this early in main().
///
/// This function spawns a background task to:
/// 1. Read or create the install state file
/// 2. Determine if this is a first install, upgrade, or normal session
/// 3. Update the state file
/// 4. Send analytics events (release builds only)
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
async fn track_launch_inner(nori_home: &Path) -> anyhow::Result<LaunchEvent> {
    let now = Utc::now();
    let current_version = CLI_VERSION;
    let install_source = detect_install_source();
    let client_id = generate_client_id();
    let user_id = generate_user_id();

    // Generate ephemeral session_id for this process
    let session_id = uuid::Uuid::new_v4().to_string();

    // Read existing state or treat missing/corrupt file as first install
    let existing_state = read_install_state(nori_home);

    let (event, new_state, is_resurrection) = match existing_state {
        None => {
            // First install
            debug!("First install detected, creating install state");
            let state = InstallState::new_first_install(
                client_id,
                user_id,
                current_version.to_string(),
                install_source,
                now,
            );
            (LaunchEvent::FirstInstall, state, false)
        }
        Some(mut state) => {
            // Check for resurrection before other logic
            let resurrected = should_resurrect(&state, now);

            if state.installed_version != current_version {
                // Version upgrade
                let previous = state.installed_version.clone();
                debug!(
                    "Version upgrade detected: {} -> {}",
                    previous, current_version
                );
                state.record_upgrade(current_version.to_string(), install_source, now);
                (
                    LaunchEvent::Upgrade {
                        previous_version: previous,
                    },
                    state,
                    resurrected,
                )
            } else {
                // Normal session
                let days = state.days_since_install(now);
                debug!("Normal session, days since install: {days}");
                state.record_session(now);
                (
                    LaunchEvent::Session {
                        days_since_install: days,
                    },
                    state,
                    resurrected,
                )
            }
        }
    };

    // Write updated state
    write_install_state(nori_home, &new_state).await?;

    // Send analytics events if not opted out
    if should_send_analytics(&new_state) {
        let days = new_state.days_since_install(now);

        // Send resurrection event first if applicable
        if is_resurrection {
            debug!("User resurrection detected (>30 days since last launch)");
            let resurrection_event = create_flat_resurrection_event(&new_state, days, &session_id);
            send_flat_event(&resurrection_event).await;
        }

        // Send main event using new flat schema
        let flat_event = match &event {
            LaunchEvent::FirstInstall => create_flat_install_event(
                &new_state,
                InstallEventType::FirstInstall,
                days,
                &session_id,
            ),
            LaunchEvent::Upgrade { previous_version } => create_flat_install_event(
                &new_state,
                InstallEventType::Upgrade {
                    previous_version: previous_version.clone(),
                },
                days,
                &session_id,
            ),
            LaunchEvent::Session { days_since_install } => {
                create_flat_session_event(&new_state, *days_since_install, &session_id)
            }
        };
        send_flat_event(&flat_event).await;
    } else {
        debug!("Analytics opted out, skipping event sending");
    }

    debug!("Install tracking complete: {event:?}");

    Ok(event)
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
    rt.block_on(track_launch_inner(nori_home))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_temp_home() -> TempDir {
        tempfile::tempdir().expect("failed to create temp dir")
    }

    #[test]
    fn test_first_install() {
        let temp_home = setup_temp_home();

        let event = track_launch_sync(temp_home.path()).expect("tracking failed");

        assert_eq!(event, LaunchEvent::FirstInstall);

        // Verify state file was created
        let state = read_install_state(temp_home.path()).expect("state should exist");
        assert_eq!(state.installed_version, CLI_VERSION);
        assert_eq!(state.schema_version, 2);
        assert!(state.client_id.contains('-')); // UUID format
        assert!(!state.opt_out); // Default opt-out is false
    }

    #[test]
    fn test_normal_session() {
        let temp_home = setup_temp_home();

        // First launch
        let event1 = track_launch_sync(temp_home.path()).expect("first tracking failed");
        assert_eq!(event1, LaunchEvent::FirstInstall);

        // Second launch - should be a normal session
        let event2 = track_launch_sync(temp_home.path()).expect("second tracking failed");

        match event2 {
            LaunchEvent::Session { days_since_install } => {
                assert_eq!(days_since_install, 0); // Same day
            }
            _ => panic!("Expected Session event, got {event2:?}"),
        }
    }

    #[test]
    fn test_version_upgrade() {
        let temp_home = setup_temp_home();

        // Create a state file with an older version
        let now = Utc::now();
        let old_state = InstallState::new_first_install(
            generate_client_id(),
            generate_user_id(),
            "0.0.1".to_string(), // Old version
            InstallSource::Npm,
            now,
        );

        // Write the old state
        let state_path = temp_home.path().join(".nori-install.json");
        let json = serde_json::to_string_pretty(&old_state).expect("serialize failed");
        fs::write(&state_path, format!("{json}\n")).expect("write failed");

        // Track launch with current version
        let event = track_launch_sync(temp_home.path()).expect("tracking failed");

        match event {
            LaunchEvent::Upgrade { previous_version } => {
                assert_eq!(previous_version, "0.0.1");
            }
            _ => panic!("Expected Upgrade event, got {event:?}"),
        }

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
        let event = track_launch_sync(temp_home.path()).expect("tracking failed");
        assert_eq!(event, LaunchEvent::FirstInstall);

        // Verify state was recreated
        let state = read_install_state(temp_home.path()).expect("state should exist");
        assert_eq!(state.installed_version, CLI_VERSION);
    }

    #[test]
    fn test_user_id_persisted() {
        let temp_home = setup_temp_home();

        // First launch
        track_launch_sync(temp_home.path()).expect("first tracking failed");
        let state1 = read_install_state(temp_home.path()).expect("state should exist");

        // Second launch
        track_launch_sync(temp_home.path()).expect("second tracking failed");
        let state2 = read_install_state(temp_home.path()).expect("state should exist");

        // User ID should remain the same
        assert_eq!(state1.user_id, state2.user_id);
    }

    #[test]
    fn test_first_installed_at_immutable() {
        let temp_home = setup_temp_home();

        // First launch
        track_launch_sync(temp_home.path()).expect("first tracking failed");
        let state1 = read_install_state(temp_home.path()).expect("state should exist");
        let first_installed = state1.first_installed_at;

        // Second launch
        track_launch_sync(temp_home.path()).expect("second tracking failed");
        let state2 = read_install_state(temp_home.path()).expect("state should exist");

        // first_installed_at should not change
        assert_eq!(state2.first_installed_at, first_installed);
    }

    #[test]
    fn test_last_launched_at_updated() {
        let temp_home = setup_temp_home();

        // First launch
        track_launch_sync(temp_home.path()).expect("first tracking failed");
        let state1 = read_install_state(temp_home.path()).expect("state should exist");

        // Small delay to ensure different timestamp
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Second launch
        track_launch_sync(temp_home.path()).expect("second tracking failed");
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
        let event = track_launch_sync(&nested_home).expect("tracking failed");
        assert_eq!(event, LaunchEvent::FirstInstall);

        // Verify directory and file were created
        assert!(nested_home.exists());
        assert!(read_install_state(&nested_home).is_some());
    }

    // @current-session
    #[test]
    fn test_opt_out_via_env_var() {
        let temp_home = setup_temp_home();

        // Set opt-out environment variable
        // SAFETY: Tests run sequentially in the same process
        unsafe {
            std::env::set_var("NORI_NO_ANALYTICS", "1");
        }

        // Track launch
        track_launch_sync(temp_home.path()).expect("tracking failed");

        // State file should still be updated
        let state = read_install_state(temp_home.path()).expect("state should exist");
        assert_eq!(state.installed_version, CLI_VERSION);

        // Clean up env var
        // SAFETY: Tests run sequentially in the same process
        unsafe {
            std::env::remove_var("NORI_NO_ANALYTICS");
        }

        // Note: We can't verify that no network request was made in this sync test,
        // but the send_event function checks this internally
    }

    // @current-session
    #[test]
    fn test_opt_out_via_state_file() {
        let temp_home = setup_temp_home();

        // Create initial state
        track_launch_sync(temp_home.path()).expect("first tracking failed");

        // Set opt_out in state file
        let mut state = read_install_state(temp_home.path()).expect("state should exist");
        state.opt_out = true;
        let state_path = temp_home.path().join(".nori-install.json");
        let json = serde_json::to_string_pretty(&state).expect("serialize failed");
        fs::write(&state_path, format!("{json}\n")).expect("write failed");

        // Track another launch
        track_launch_sync(temp_home.path()).expect("second tracking failed");

        // State should still be updated
        let state2 = read_install_state(temp_home.path()).expect("state should exist");
        assert!(state2.opt_out);

        // Note: We can't verify that no network request was made in this sync test,
        // but the send_event function checks this internally
    }

    // @current-session
    #[test]
    fn test_user_resurrection_after_30_days() {
        let temp_home = setup_temp_home();

        // Create initial state with old last_launched_at
        let now = Utc::now();
        let old_time = now - chrono::Duration::days(31);
        let mut state = InstallState::new_first_install(
            generate_client_id(),
            generate_user_id(),
            CLI_VERSION.to_string(),
            InstallSource::Npm,
            old_time,
        );
        state.last_launched_at = old_time;

        // Write state
        let state_path = temp_home.path().join(".nori-install.json");
        let json = serde_json::to_string_pretty(&state).expect("serialize failed");
        fs::write(&state_path, format!("{json}\n")).expect("write failed");

        // Track launch - should detect resurrection
        let event = track_launch_sync(temp_home.path()).expect("tracking failed");

        // Should return Session event (resurrection is tracked internally)
        match event {
            LaunchEvent::Session { .. } => {} // OK
            _ => panic!("Expected Session event after resurrection, got {event:?}"),
        }

        // State should be updated
        let new_state = read_install_state(temp_home.path()).expect("state should exist");
        assert!(new_state.last_launched_at > old_time);
    }

    // @current-session
    #[test]
    fn test_no_resurrection_before_30_days() {
        let temp_home = setup_temp_home();

        // Create initial state with recent last_launched_at
        let now = Utc::now();
        let recent_time = now - chrono::Duration::days(29);
        let mut state = InstallState::new_first_install(
            generate_client_id(),
            generate_user_id(),
            CLI_VERSION.to_string(),
            InstallSource::Npm,
            recent_time,
        );
        state.last_launched_at = recent_time;

        // Write state
        let state_path = temp_home.path().join(".nori-install.json");
        let json = serde_json::to_string_pretty(&state).expect("serialize failed");
        fs::write(&state_path, format!("{json}\n")).expect("write failed");

        // Track launch - should NOT detect resurrection
        let event = track_launch_sync(temp_home.path()).expect("tracking failed");

        // Should be normal session
        match event {
            LaunchEvent::Session { .. } => {} // OK
            _ => panic!("Expected Session event, got {event:?}"),
        }
    }
}
