//! Analytics event sending
//!
//! Provides fire-and-forget analytics event sending for install tracking.

use crate::state::InstallSource;
use crate::state::InstallState;
use serde::Serialize;

/// Event name for install/upgrade events
pub const EVENT_PLUGIN_INSTALL_COMPLETED: &str = "plugin_install_completed";

/// Event name for session start events
pub const EVENT_SESSION_STARTED: &str = "nori_session_started";

/// Analytics event request payload
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackEventRequest {
    /// Client identifier (always "nori-cli")
    pub client_id: String,

    /// Privacy-protecting user identifier
    pub user_id: String,

    /// Name of the event
    pub event_name: String,

    /// Event-specific parameters
    pub event_params: serde_json::Value,
}

/// Type of install event
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallEventType {
    /// First time installation
    FirstInstall,
    /// Version upgrade
    Upgrade { previous_version: &'static str },
}

/// Create an install/upgrade event
pub fn create_install_event(
    state: &InstallState,
    event_type: InstallEventType,
) -> TrackEventRequest {
    let (is_first_install, previous_version) = match event_type {
        InstallEventType::FirstInstall => (true, None),
        InstallEventType::Upgrade { previous_version } => (false, Some(previous_version)),
    };

    let mut params = serde_json::json!({
        "install_type": "free",
        "install_source": install_source_to_string(state.install_source),
        "installed_version": state.installed_version,
        "is_first_install": is_first_install,
    });

    if let Some(prev) = previous_version {
        params["previous_version"] = serde_json::Value::String(prev.to_string());
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
        "install_type": "free",
        "installed_version": state.installed_version,
        "install_source": install_source_to_string(state.install_source),
        "days_since_install": days_since_install,
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use chrono::Utc;

    fn create_test_state() -> InstallState {
        let now = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        InstallState::new_first_install(
            "sha256:testhash".to_string(),
            "1.0.0".to_string(),
            InstallSource::Bun,
            now,
        )
    }

    #[test]
    fn test_create_first_install_event() {
        let state = create_test_state();
        let event = create_install_event(&state, InstallEventType::FirstInstall);

        assert_eq!(event.client_id, "nori-cli");
        assert_eq!(event.user_id, "sha256:testhash");
        assert_eq!(event.event_name, EVENT_PLUGIN_INSTALL_COMPLETED);

        let params = &event.event_params;
        assert_eq!(params["install_type"], "free");
        assert_eq!(params["install_source"], "bun");
        assert_eq!(params["installed_version"], "1.0.0");
        assert_eq!(params["is_first_install"], true);
        assert!(params.get("previous_version").is_none());
    }

    #[test]
    fn test_create_upgrade_event() {
        let mut state = create_test_state();
        state.installed_version = "2.0.0".to_string();
        state.install_source = InstallSource::Npm;

        let event = create_install_event(
            &state,
            InstallEventType::Upgrade {
                previous_version: "1.0.0",
            },
        );

        assert_eq!(event.event_name, EVENT_PLUGIN_INSTALL_COMPLETED);

        let params = &event.event_params;
        assert_eq!(params["install_source"], "npm");
        assert_eq!(params["installed_version"], "2.0.0");
        assert_eq!(params["is_first_install"], false);
        assert_eq!(params["previous_version"], "1.0.0");
    }

    #[test]
    fn test_create_session_event() {
        let state = create_test_state();
        let event = create_session_event(&state, 5);

        assert_eq!(event.client_id, "nori-cli");
        assert_eq!(event.user_id, "sha256:testhash");
        assert_eq!(event.event_name, EVENT_SESSION_STARTED);

        let params = &event.event_params;
        assert_eq!(params["install_type"], "free");
        assert_eq!(params["installed_version"], "1.0.0");
        assert_eq!(params["install_source"], "bun");
        assert_eq!(params["days_since_install"], 5);
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
            "sha256:test".to_string(),
            "1.0.0".to_string(),
            InstallSource::Unknown,
            now,
        );

        let event = create_session_event(&state, 0);
        assert_eq!(event.event_params["install_source"], "unknown");
    }
}
