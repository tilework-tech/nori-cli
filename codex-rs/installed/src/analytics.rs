//! Analytics event sending
//!
//! Provides fire-and-forget analytics event sending for install tracking.

use crate::state::InstallSource;
use crate::state::InstallState;
use chrono::DateTime;
use chrono::SecondsFormat;
use chrono::Utc;
use serde::Serialize;

/// Default analytics endpoint URL (only used in release builds)
#[cfg(not(debug_assertions))]
const DEFAULT_ANALYTICS_URL: &str = "https://noriskillsets.dev/api/analytics/track";

/// Environment variable to override the analytics URL (only used in release builds)
#[cfg(not(debug_assertions))]
const ANALYTICS_URL_ENV: &str = "NORI_ANALYTICS_URL";

/// Environment variable to opt out of analytics
pub const ANALYTICS_OPT_OUT_ENV: &str = "NORI_NO_ANALYTICS";

const EXECUTABLE_NAME: &str = "nori-ai-cli";

/// Analytics event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyticsEventType {
    InstallDetected,
    SessionStart,
    UserResurrected,
}

impl AnalyticsEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            AnalyticsEventType::InstallDetected => "noricli_install_detected",
            AnalyticsEventType::SessionStart => "noricli_session_started",
            AnalyticsEventType::UserResurrected => "noricli_user_resurrected",
        }
    }
}

/// Analytics event request payload
#[derive(Debug, Clone, Serialize)]
pub struct TrackEventRequest {
    pub client_id: String,
    pub user_id: String,
    pub event_name: String,
    pub event_params: serde_json::Value,
}

pub fn create_event(
    event_type: AnalyticsEventType,
    state: &InstallState,
    session_id: &str,
    timestamp: DateTime<Utc>,
    days_since_install: i64,
    is_first_install: bool,
    previous_version: Option<String>,
) -> TrackEventRequest {
    let mut params = base_event_params(state, session_id, timestamp, days_since_install);
    if event_type == AnalyticsEventType::InstallDetected {
        params["tilework_cli_is_first_install"] = serde_json::Value::Bool(is_first_install);
        if let Some(prev) = previous_version {
            params["tilework_cli_previous_version"] = serde_json::Value::String(prev);
        }
    }

    TrackEventRequest {
        client_id: state.client_id.clone(),
        user_id: state.client_id.clone(),
        event_name: event_type.as_str().to_string(),
        event_params: params,
    }
}

fn base_event_params(
    state: &InstallState,
    session_id: &str,
    timestamp: DateTime<Utc>,
    days_since_install: i64,
) -> serde_json::Value {
    serde_json::json!({
        // Required tilework_* fields (no cli_ prefix)
        "tilework_source": "nori-cli",
        "tilework_session_id": session_id,
        "tilework_timestamp": timestamp.to_rfc3339_opts(SecondsFormat::Millis, true),
        // CLI-specific fields (tilework_cli_* prefix)
        "tilework_cli_executable_name": EXECUTABLE_NAME,
        "tilework_cli_installed_version": state.installed_version.as_str(),
        "tilework_cli_install_source": install_source_to_string(state.install_source),
        "tilework_cli_days_since_install": days_since_install,
        "tilework_cli_os": std::env::consts::OS,
        "tilework_cli_arch": std::env::consts::ARCH,
    })
}

fn install_source_to_string(source: InstallSource) -> &'static str {
    match source {
        InstallSource::Npm => "npm",
        InstallSource::Bun => "bun",
        InstallSource::Unknown => "unknown",
    }
}

/// Send an analytics event to the backend (release builds only).
///
/// In release builds, sends the event via HTTP POST to the analytics endpoint.
/// Failures are silently ignored (fire-and-forget).
///
/// In debug builds, this is a no-op to avoid noise from development and testing.
#[cfg(not(debug_assertions))]
pub async fn send_event(event: &TrackEventRequest) {
    let url =
        std::env::var(ANALYTICS_URL_ENV).unwrap_or_else(|_| DEFAULT_ANALYTICS_URL.to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(_) => {
            return;
        }
    };

    let _ = client.post(&url).json(event).send().await;
}

/// No-op analytics sending for debug builds.
#[cfg(debug_assertions)]
pub async fn send_event(event: &TrackEventRequest) {
    tracing::debug!(
        "Analytics event skipped (debug build): {}",
        event.event_name
    );
}

/// Check if running in a CI environment.
///
/// Returns true if CI environment variable is set to a truthy value.
pub fn is_ci_env() -> bool {
    std::env::var("CI")
        .map(|value| value != "0" && !value.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use pretty_assertions::assert_eq;

    fn create_test_state() -> InstallState {
        let now = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        InstallState::new_first_install(
            "c4f24cc9-acde-4d20-87e1-1d6bfa8e7a67".to_string(),
            "1.0.0".to_string(),
            InstallSource::Bun,
            now,
        )
    }

    #[test]
    fn test_create_install_detected_event_first_install() {
        let state = create_test_state();
        let now = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let event = create_event(
            AnalyticsEventType::InstallDetected,
            &state,
            "1737373800",
            now,
            0,
            true,
            None,
        );

        assert_eq!(event.event_name, "noricli_install_detected");
        assert_eq!(event.client_id, state.client_id);
        assert_eq!(event.user_id, state.client_id);

        let params = &event.event_params;
        // Required tilework_* fields
        assert_eq!(params["tilework_source"], "nori-cli");
        assert_eq!(params["tilework_session_id"], "1737373800");
        assert!(
            params["tilework_timestamp"]
                .as_str()
                .unwrap()
                .contains("2025-01-15")
        );

        // CLI-specific fields
        assert_eq!(params["tilework_cli_install_source"], "bun");
        assert_eq!(params["tilework_cli_installed_version"], "1.0.0");
        assert_eq!(params["tilework_cli_is_first_install"], true);
        assert_eq!(params["tilework_cli_days_since_install"], 0);
        assert_eq!(params["tilework_cli_executable_name"], "nori-ai-cli");
        assert!(params.get("tilework_cli_previous_version").is_none());

        // Removed fields should NOT be present
        assert!(params.get("tilework_cli_user_id").is_none());
    }

    #[test]
    fn test_create_install_detected_event_upgrade() {
        let mut state = create_test_state();
        state.installed_version = "2.0.0".to_string();
        state.install_source = InstallSource::Npm;

        let now = Utc.with_ymd_and_hms(2025, 1, 20, 10, 30, 0).unwrap();
        let event = create_event(
            AnalyticsEventType::InstallDetected,
            &state,
            "1737373800",
            now,
            5,
            false,
            Some("1.0.0".to_string()),
        );

        let params = &event.event_params;
        // Required tilework_* fields
        assert_eq!(params["tilework_source"], "nori-cli");
        assert_eq!(params["tilework_session_id"], "1737373800");

        // CLI-specific fields
        assert_eq!(params["tilework_cli_install_source"], "npm");
        assert_eq!(params["tilework_cli_installed_version"], "2.0.0");
        assert_eq!(params["tilework_cli_is_first_install"], false);
        assert_eq!(params["tilework_cli_previous_version"], "1.0.0");
        assert_eq!(params["tilework_cli_days_since_install"], 5);

        // Removed fields should NOT be present
        assert!(params.get("tilework_cli_user_id").is_none());
    }

    #[test]
    fn test_create_session_start_event() {
        let state = create_test_state();
        let now = Utc.with_ymd_and_hms(2025, 1, 20, 10, 30, 0).unwrap();
        let event = create_event(
            AnalyticsEventType::SessionStart,
            &state,
            "1737373800",
            now,
            5,
            false,
            None,
        );

        assert_eq!(event.event_name, "noricli_session_started");

        let params = &event.event_params;
        // Required tilework_* fields (no cli_ prefix)
        assert_eq!(params["tilework_source"], "nori-cli");
        assert_eq!(params["tilework_session_id"], "1737373800");
        assert!(
            params["tilework_timestamp"]
                .as_str()
                .unwrap()
                .contains("2025-01-20")
        );

        // CLI-specific fields
        assert_eq!(params["tilework_cli_installed_version"], "1.0.0");
        assert_eq!(params["tilework_cli_install_source"], "bun");
        assert_eq!(params["tilework_cli_days_since_install"], 5);
        assert_eq!(params["tilework_cli_executable_name"], "nori-ai-cli");
        assert_eq!(params["tilework_cli_os"], std::env::consts::OS);
        assert_eq!(params["tilework_cli_arch"], std::env::consts::ARCH);

        // Install-only fields should NOT be present
        assert!(params.get("tilework_cli_is_first_install").is_none());
        assert!(params.get("tilework_cli_previous_version").is_none());

        // Removed fields should NOT be present
        assert!(params.get("tilework_cli_user_id").is_none());
        assert!(params.get("tilework_cli_session_id").is_none());
        assert!(params.get("tilework_cli_timestamp").is_none());
        assert!(params.get("tilework_cli_node_version").is_none());
        assert!(params.get("tilework_cli_is_ci").is_none());
    }

    // @current-session
    #[test]
    fn test_track_event_request_uses_snake_case() {
        let state = create_test_state();
        let now = Utc.with_ymd_and_hms(2025, 1, 20, 10, 30, 0).unwrap();
        let event = create_event(
            AnalyticsEventType::SessionStart,
            &state,
            "1737373800",
            now,
            5,
            false,
            None,
        );

        let json = serde_json::to_string(&event).expect("serialization failed");

        // Should use snake_case field names per API spec
        assert!(json.contains("\"client_id\""), "should contain client_id");
        assert!(json.contains("\"user_id\""), "should contain user_id");
        assert!(json.contains("\"event_name\""), "should contain event_name");
        assert!(
            json.contains("\"event_params\""),
            "should contain event_params"
        );

        // Should NOT use camelCase field names
        assert!(
            !json.contains("\"clientId\""),
            "should not contain clientId"
        );
        assert!(!json.contains("\"userId\""), "should not contain userId");
        assert!(
            !json.contains("\"eventName\""),
            "should not contain eventName"
        );
        assert!(
            !json.contains("\"eventParams\""),
            "should not contain eventParams"
        );
    }
}
