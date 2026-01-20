//! Analytics event sending
//!
//! Provides fire-and-forget analytics event sending for install tracking.

use crate::state::InstallState;
use chrono::DateTime;
use chrono::Utc;
use serde::Serialize;
use tracing::debug;

/// Analytics event request payload
#[derive(Debug, Clone, Serialize)]
pub struct TrackEventRequest {
    /// Event name
    pub event: String,

    /// Client identifier (UUID string)
    pub client_id: String,

    /// Session identifier (ephemeral per run)
    pub session_id: String,

    /// Event timestamp (ISO-8601)
    pub timestamp: String,

    /// Event properties
    pub properties: AnalyticsProperties,
}

/// Analytics event properties
#[derive(Debug, Clone, Serialize)]
pub struct AnalyticsProperties {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub node_version: String,
    pub is_ci: bool,
}

/// Build the shared analytics properties payload.
pub fn build_properties(version: &str) -> AnalyticsProperties {
    AnalyticsProperties {
        version: version.to_string(),
        os: normalized_os().to_string(),
        arch: normalized_arch().to_string(),
        node_version: node_version(),
        is_ci: is_ci(),
    }
}

/// Create an analytics event payload.
pub fn create_track_event(
    state: &InstallState,
    event: &str,
    session_id: &str,
    timestamp: DateTime<Utc>,
    properties: AnalyticsProperties,
) -> TrackEventRequest {
    TrackEventRequest {
        event: event.to_string(),
        client_id: state.client_id.clone(),
        session_id: session_id.to_string(),
        timestamp: timestamp.to_rfc3339(),
        properties,
    }
}

/// Send an analytics event to the backend.
///
/// It sends the event via HTTP POST to the analytics endpoint. Failures are
/// silently ignored (fire-and-forget).
pub async fn send_event(event: &TrackEventRequest) {
    /// Default analytics endpoint URL
    const DEFAULT_ANALYTICS_URL: &str = "https://noriskillsets.dev/api/analytics/track";

    /// Environment variable to override the analytics URL
    const ANALYTICS_URL_ENV: &str = "NORI_ANALYTICS_URL";

    let url =
        std::env::var(ANALYTICS_URL_ENV).unwrap_or_else(|_| DEFAULT_ANALYTICS_URL.to_string());
    debug!("Sending analytics event to {}: {}", url, event.event);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(_) => return,
    };

    let _ = client.post(&url).json(event).send().await;
}

fn normalized_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}

fn normalized_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    }
}

fn node_version() -> String {
    std::env::var("NORI_NODE_VERSION")
        .or_else(|_| std::env::var("NODE_VERSION"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn is_ci() -> bool {
    let truthy = |value: &str| matches!(value, "1" | "true" | "TRUE");
    if let Ok(value) = std::env::var("CI") {
        return truthy(&value);
    }

    [
        "GITHUB_ACTIONS",
        "BUILDKITE",
        "CIRCLECI",
        "GITLAB_CI",
        "TEAMCITY_VERSION",
        "JENKINS_URL",
        "TRAVIS",
        "APPVEYOR",
    ]
    .iter()
    .any(|key| std::env::var(key).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    fn create_test_state() -> InstallState {
        let now = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        InstallState::new_first_install(
            "7b9f7433-2b41-4d2f-94cc-2b27fe58b035".to_string(),
            "1.0.0".to_string(),
            crate::state::InstallSource::Bun,
            now,
        )
    }

    #[test]
    fn test_create_track_event() {
        let state = create_test_state();
        let properties = build_properties("1.0.0");
        let timestamp = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let event =
            create_track_event(&state, "session_start", "session-id", timestamp, properties);

        assert_eq!(event.client_id, "7b9f7433-2b41-4d2f-94cc-2b27fe58b035");
        assert_eq!(event.session_id, "session-id");
        assert_eq!(event.event, "session_start");
        assert_eq!(event.timestamp, "2025-01-15T10:30:00+00:00");
    }

    #[test]
    fn test_event_serialization() {
        let state = create_test_state();
        let properties = AnalyticsProperties {
            version: "1.0.0".to_string(),
            os: "darwin".to_string(),
            arch: "arm64".to_string(),
            node_version: "20.0.0".to_string(),
            is_ci: false,
        };
        let event = create_track_event(
            &state,
            "session_start",
            "session-id",
            Utc::now(),
            properties,
        );

        let json = serde_json::to_string(&event).expect("serialization failed");

        assert!(json.contains("\"client_id\""));
        assert!(json.contains("\"session_id\""));
        assert!(json.contains("\"event\""));
        assert!(json.contains("\"properties\""));
    }
}
