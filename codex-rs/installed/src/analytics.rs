//! Analytics event sending
//!
//! Provides fire-and-forget analytics event sending for install tracking.
//!
//! Analytics events are only sent in release builds to avoid noise from
//! development and E2E testing.

use crate::state::InstallSource;
use crate::state::InstallState;
use serde::Serialize;
use tracing::debug;

/// Event name for install/upgrade events
pub const EVENT_PLUGIN_INSTALL_COMPLETED: &str = "plugin_install_completed";

/// Event name for session start events
pub const EVENT_SESSION_STARTED: &str = "nori_session_started";

/// Event names for new flat schema
pub const EVENT_APP_INSTALL: &str = "app_install";
pub const EVENT_APP_UPDATE: &str = "app_update";
pub const EVENT_SESSION_START: &str = "session_start";
pub const EVENT_USER_RESURRECTED: &str = "user_resurrected";

/// Analytics event request payload (legacy nested schema)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackEventRequest {
    /// Client identifier (deterministic UUID)
    pub client_id: String,

    /// Privacy-protecting user identifier
    pub user_id: String,

    /// Name of the event
    pub event_name: String,

    /// Event-specific parameters
    pub event_params: serde_json::Value,
}

/// Analytics event request payload (new flat schema)
#[derive(Debug, Clone, Serialize)]
pub struct FlatEventRequest {
    /// Event name
    pub event: String,

    /// Client identifier (deterministic UUID)
    pub client_id: String,

    /// Session identifier (ephemeral, per-process)
    pub session_id: String,

    /// ISO-8601 timestamp
    pub timestamp: String,

    /// Event properties
    pub properties: EventProperties,
}

/// Event properties for flat schema
#[derive(Debug, Clone, Serialize)]
pub struct EventProperties {
    /// CLI version
    pub version: String,

    /// Operating system
    pub os: String,

    /// CPU architecture
    pub arch: String,

    /// Whether running in CI environment
    pub is_ci: bool,
}

/// Type of install event
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallEventType {
    /// First time installation
    FirstInstall,
    /// Version upgrade
    Upgrade { previous_version: String },
}

/// Create an install/upgrade event
pub fn create_install_event(
    state: &InstallState,
    event_type: InstallEventType,
    days_since_install: i64,
) -> TrackEventRequest {
    let (is_first_install, previous_version) = match &event_type {
        InstallEventType::FirstInstall => (true, None),
        InstallEventType::Upgrade { previous_version } => (false, Some(previous_version.clone())),
    };

    let mut params = serde_json::json!({
        "tilework_user_id": state.user_id,
        "tilework_cli_installed_version": state.installed_version,
        "tilework_cli_install_source": install_source_to_string(state.install_source),
        "tilework_cli_is_first_install": is_first_install,
        "tilework_cli_days_since_install": days_since_install,
    });

    if let Some(prev) = previous_version {
        params["tilework_cli_previous_version"] = serde_json::Value::String(prev);
    }

    TrackEventRequest {
        client_id: state.client_id.clone(),
        user_id: state.user_id.clone(),
        event_name: EVENT_PLUGIN_INSTALL_COMPLETED.to_string(),
        event_params: params,
    }
}

/// Create a session started event
pub fn create_session_event(state: &InstallState, days_since_install: i64) -> TrackEventRequest {
    let params = serde_json::json!({
        "tilework_user_id": state.user_id,
        "tilework_cli_installed_version": state.installed_version,
        "tilework_cli_install_source": install_source_to_string(state.install_source),
        "tilework_cli_days_since_install": days_since_install,
    });

    TrackEventRequest {
        client_id: state.client_id.clone(),
        user_id: state.user_id.clone(),
        event_name: EVENT_SESSION_STARTED.to_string(),
        event_params: params,
    }
}

fn install_source_to_string(source: InstallSource) -> &'static str {
    match source {
        InstallSource::Npm => "npm",
        InstallSource::Bun => "bun",
        InstallSource::Unknown => "unknown",
    }
}

/// Detect operating system
fn detect_os() -> String {
    std::env::consts::OS.to_string()
}

/// Detect CPU architecture
fn detect_arch() -> String {
    std::env::consts::ARCH.to_string()
}

/// Detect if running in CI environment
fn is_ci() -> bool {
    std::env::var("CI").as_deref() == Ok("true")
        || std::env::var("CI").as_deref() == Ok("1")
        || std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        || std::env::var("GITLAB_CI").as_deref() == Ok("true")
        || std::env::var("CIRCLECI").as_deref() == Ok("true")
        || std::env::var("TRAVIS").as_deref() == Ok("true")
}

/// Create event properties for flat schema
fn create_event_properties(state: &InstallState) -> EventProperties {
    EventProperties {
        version: state.installed_version.clone(),
        os: detect_os(),
        arch: detect_arch(),
        is_ci: is_ci(),
    }
}

/// Create a flat install/upgrade event
pub fn create_flat_install_event(
    state: &InstallState,
    event_type: InstallEventType,
    _days_since_install: i64,
    session_id: &str,
) -> FlatEventRequest {
    let event = match event_type {
        InstallEventType::FirstInstall => EVENT_APP_INSTALL,
        InstallEventType::Upgrade { .. } => EVENT_APP_UPDATE,
    };

    FlatEventRequest {
        event: event.to_string(),
        client_id: state.client_id.clone(),
        session_id: session_id.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        properties: create_event_properties(state),
    }
}

/// Create a flat session started event
pub fn create_flat_session_event(
    state: &InstallState,
    _days_since_install: i64,
    session_id: &str,
) -> FlatEventRequest {
    FlatEventRequest {
        event: EVENT_SESSION_START.to_string(),
        client_id: state.client_id.clone(),
        session_id: session_id.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        properties: create_event_properties(state),
    }
}

/// Create a flat user resurrected event
pub fn create_flat_resurrection_event(
    state: &InstallState,
    _days_since_install: i64,
    session_id: &str,
) -> FlatEventRequest {
    FlatEventRequest {
        event: EVENT_USER_RESURRECTED.to_string(),
        client_id: state.client_id.clone(),
        session_id: session_id.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        properties: create_event_properties(state),
    }
}

/// Send a flat analytics event to the backend (release builds only)
///
/// This function is a no-op in debug builds to avoid noise from
/// development and E2E testing.
///
/// In release builds, it sends the event via HTTP POST to the analytics
/// endpoint. Failures are silently ignored (fire-and-forget).
#[cfg(not(debug_assertions))]
pub async fn send_flat_event(event: &FlatEventRequest) {
    /// Default analytics endpoint URL
    const DEFAULT_ANALYTICS_URL: &str = "https://noriskillsets.dev/api/analytics/track";

    /// Environment variable to override the analytics URL
    const ANALYTICS_URL_ENV: &str = "NORI_ANALYTICS_URL";

    let url =
        std::env::var(ANALYTICS_URL_ENV).unwrap_or_else(|_| DEFAULT_ANALYTICS_URL.to_string());
    debug!("Sending analytics event to {}: {:?}", url, event.event);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(e) => {
            debug!("Failed to create HTTP client for analytics: {e}");
            return;
        }
    };

    match client.post(&url).json(event).send().await {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                debug!("Analytics event sent successfully: {}", event.event);
            } else {
                debug!("Analytics request failed with status {status}");
            }
        }
        Err(e) => {
            debug!("Failed to send analytics event: {e}");
        }
    }
}

/// No-op analytics sending for debug builds
#[cfg(debug_assertions)]
pub async fn send_flat_event(event: &FlatEventRequest) {
    debug!("Analytics event skipped (debug build): {}", event.event);
    let _ = event; // Suppress unused warning
}

/// Send an analytics event to the backend (release builds only)
///
/// This function is a no-op in debug builds to avoid noise from
/// development and E2E testing.
///
/// In release builds, it sends the event via HTTP POST to the analytics
/// endpoint. Failures are silently ignored (fire-and-forget).
#[cfg(not(debug_assertions))]
pub async fn send_event(event: &TrackEventRequest) {
    /// Default analytics endpoint URL
    const DEFAULT_ANALYTICS_URL: &str = "https://noriskillsets.dev/api/analytics/track";

    /// Environment variable to override the analytics URL
    const ANALYTICS_URL_ENV: &str = "NORI_ANALYTICS_URL";

    let url =
        std::env::var(ANALYTICS_URL_ENV).unwrap_or_else(|_| DEFAULT_ANALYTICS_URL.to_string());
    debug!("Sending analytics event to {}: {:?}", url, event.event_name);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(e) => {
            debug!("Failed to create HTTP client for analytics: {e}");
            return;
        }
    };

    match client.post(&url).json(event).send().await {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                debug!("Analytics event sent successfully: {}", event.event_name);
            } else {
                debug!("Analytics request failed with status {status}");
            }
        }
        Err(e) => {
            debug!("Failed to send analytics event: {e}");
        }
    }
}

/// No-op analytics sending for debug builds
#[cfg(debug_assertions)]
pub async fn send_event(event: &TrackEventRequest) {
    debug!(
        "Analytics event skipped (debug build): {}",
        event.event_name
    );
    let _ = event; // Suppress unused warning
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::Utc;

    fn create_test_state() -> InstallState {
        let now = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        InstallState::new_first_install(
            "550e8400-e29b-41d4-a716-446655440000".to_string(),
            "sha256:testhash".to_string(),
            "1.0.0".to_string(),
            InstallSource::Bun,
            now,
        )
    }

    #[test]
    fn test_create_first_install_event() {
        let state = create_test_state();
        let event = create_install_event(&state, InstallEventType::FirstInstall, 0);

        assert_eq!(event.client_id, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(event.user_id, "sha256:testhash");
        assert_eq!(event.event_name, EVENT_PLUGIN_INSTALL_COMPLETED);

        let params = &event.event_params;
        // Verify new tilework_cli_ prefixed fields
        assert_eq!(params["tilework_user_id"], "sha256:testhash");
        assert_eq!(params["tilework_cli_install_source"], "bun");
        assert_eq!(params["tilework_cli_installed_version"], "1.0.0");
        assert_eq!(params["tilework_cli_is_first_install"], true);
        assert_eq!(params["tilework_cli_days_since_install"], 0);
        assert!(params.get("tilework_cli_previous_version").is_none());

        // Verify removed fields are NOT present
        assert!(params.get("install_type").is_none());
        assert!(params.get("install_source").is_none());
        assert!(params.get("installed_version").is_none());
        assert!(params.get("is_first_install").is_none());
    }

    #[test]
    fn test_create_upgrade_event() {
        let mut state = create_test_state();
        state.installed_version = "2.0.0".to_string();
        state.install_source = InstallSource::Npm;

        let event = create_install_event(
            &state,
            InstallEventType::Upgrade {
                previous_version: "1.0.0".to_string(),
            },
            5, // 5 days since original install
        );

        assert_eq!(event.event_name, EVENT_PLUGIN_INSTALL_COMPLETED);

        let params = &event.event_params;
        // Verify new tilework_cli_ prefixed fields
        assert_eq!(params["tilework_user_id"], "sha256:testhash");
        assert_eq!(params["tilework_cli_install_source"], "npm");
        assert_eq!(params["tilework_cli_installed_version"], "2.0.0");
        assert_eq!(params["tilework_cli_is_first_install"], false);
        assert_eq!(params["tilework_cli_previous_version"], "1.0.0");
        assert_eq!(params["tilework_cli_days_since_install"], 5);

        // Verify removed fields are NOT present
        assert!(params.get("install_type").is_none());
        assert!(params.get("install_source").is_none());
        assert!(params.get("installed_version").is_none());
        assert!(params.get("is_first_install").is_none());
        assert!(params.get("previous_version").is_none());
    }

    #[test]
    fn test_create_session_event() {
        let state = create_test_state();
        let event = create_session_event(&state, 5);

        assert_eq!(event.client_id, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(event.user_id, "sha256:testhash");
        assert_eq!(event.event_name, EVENT_SESSION_STARTED);

        let params = &event.event_params;
        // Verify new tilework_cli_ prefixed fields
        assert_eq!(params["tilework_user_id"], "sha256:testhash");
        assert_eq!(params["tilework_cli_installed_version"], "1.0.0");
        assert_eq!(params["tilework_cli_install_source"], "bun");
        assert_eq!(params["tilework_cli_days_since_install"], 5);

        // Verify removed fields are NOT present
        assert!(params.get("install_type").is_none());
        assert!(params.get("installed_version").is_none());
        assert!(params.get("install_source").is_none());
        assert!(params.get("days_since_install").is_none());

        // Verify is_first_install is NOT in session events
        assert!(params.get("tilework_cli_is_first_install").is_none());
    }

    #[test]
    fn test_event_serialization() {
        let state = create_test_state();
        let event = create_session_event(&state, 10);

        let json = serde_json::to_string(&event).expect("serialization failed");

        // Verify camelCase field names
        assert!(json.contains("\"clientId\""));
        assert!(json.contains("\"userId\""));
        assert!(json.contains("\"eventName\""));
        assert!(json.contains("\"eventParams\""));
    }

    #[test]
    fn test_install_source_unknown() {
        let now = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let state = InstallState::new_first_install(
            "550e8400-e29b-41d4-a716-446655440000".to_string(),
            "sha256:test".to_string(),
            "1.0.0".to_string(),
            InstallSource::Unknown,
            now,
        );

        let event = create_session_event(&state, 0);
        assert_eq!(event.event_params["tilework_cli_install_source"], "unknown");
    }

    // @current-session
    #[test]
    fn test_flat_event_schema_app_install() {
        let state = create_test_state();
        let session_id = "abc123-session-id";
        let event =
            create_flat_install_event(&state, InstallEventType::FirstInstall, 0, session_id);

        // Top-level fields
        assert_eq!(event.event, "app_install");
        assert_eq!(event.client_id, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(event.session_id, session_id);
        assert!(!event.timestamp.is_empty());

        // Properties
        assert_eq!(event.properties.version, "1.0.0");
        assert!(!event.properties.os.is_empty());
        assert!(!event.properties.arch.is_empty());
        assert!(event.properties.is_ci == true || event.properties.is_ci == false);

        // Should NOT have nested event_params
        let json = serde_json::to_string(&event).expect("serialization failed");
        assert!(!json.contains("eventParams"));
        assert!(!json.contains("eventName"));
    }

    // @current-session
    #[test]
    fn test_flat_event_schema_session_start() {
        let state = create_test_state();
        let session_id = "xyz789-session-id";
        let event = create_flat_session_event(&state, 5, session_id);

        // Top-level fields
        assert_eq!(event.event, "session_start");
        assert_eq!(event.client_id, "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(event.session_id, session_id);

        // Properties
        assert_eq!(event.properties.version, "1.0.0");

        // Should NOT have nested event_params
        let json = serde_json::to_string(&event).expect("serialization failed");
        assert!(!json.contains("eventParams"));
    }

    // @current-session
    #[test]
    fn test_flat_event_schema_user_resurrected() {
        let state = create_test_state();
        let session_id = "def456-session-id";
        let event = create_flat_resurrection_event(&state, 35, session_id);

        // Top-level fields
        assert_eq!(event.event, "user_resurrected");
        assert_eq!(event.session_id, session_id);

        // Properties should match session_start
        assert_eq!(event.properties.version, "1.0.0");
    }
}
