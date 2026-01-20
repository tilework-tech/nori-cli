//! Analytics event sending
//!
//! Provides fire-and-forget analytics event sending for install tracking.

use chrono::DateTime;
use chrono::SecondsFormat;
use chrono::Utc;
use serde::Serialize;

/// Default analytics endpoint URL
pub const DEFAULT_ANALYTICS_URL: &str = "https://noriskillsets.dev/api/analytics/track";

/// Environment variable to override the analytics URL
pub const ANALYTICS_URL_ENV: &str = "NORI_ANALYTICS_URL";

/// Environment variable to opt out of analytics
pub const ANALYTICS_OPT_OUT_ENV: &str = "NORI_NO_ANALYTICS";

/// Analytics event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyticsEventType {
    AppInstall,
    AppUpdate,
    SessionStart,
    UserResurrected,
}

impl AnalyticsEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            AnalyticsEventType::AppInstall => "app_install",
            AnalyticsEventType::AppUpdate => "app_update",
            AnalyticsEventType::SessionStart => "session_start",
            AnalyticsEventType::UserResurrected => "user_resurrected",
        }
    }
}

/// Analytics event request payload
#[derive(Debug, Clone, Serialize)]
pub struct TrackEventRequest {
    pub event: String,
    pub client_id: String,
    pub session_id: String,
    pub timestamp: String,
    pub properties: EventProperties,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventProperties {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub node_version: String,
    pub is_ci: bool,
}

impl EventProperties {
    pub fn new(version: &str) -> Self {
        Self {
            version: version.to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            node_version: node_version(),
            is_ci: is_ci_env(),
        }
    }
}

pub fn create_event(
    event_type: AnalyticsEventType,
    client_id: &str,
    session_id: &str,
    timestamp: DateTime<Utc>,
    properties: EventProperties,
) -> TrackEventRequest {
    TrackEventRequest {
        event: event_type.as_str().to_string(),
        client_id: client_id.to_string(),
        session_id: session_id.to_string(),
        timestamp: timestamp.to_rfc3339_opts(SecondsFormat::Millis, true),
        properties,
    }
}

/// Send an analytics event to the backend.
///
/// It sends the event via HTTP POST to the analytics endpoint.
/// Failures are silently ignored (fire-and-forget).
pub async fn send_event(event: &TrackEventRequest) {
    let url =
        std::env::var(ANALYTICS_URL_ENV).unwrap_or_else(|_| DEFAULT_ANALYTICS_URL.to_string());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build();

    let client = match client {
        Ok(c) => c,
        Err(_) => {
            return;
        }
    };

    let _ = client.post(&url).json(event).send().await;
}

fn node_version() -> String {
    std::env::var("NORI_NODE_VERSION")
        .or_else(|_| std::env::var("NODE_VERSION"))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn is_ci_env() -> bool {
    std::env::var("CI")
        .map(|value| value != "0" && !value.is_empty())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_create_event_payload() {
        let now = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let properties = EventProperties::new("1.0.0");
        let event = create_event(
            AnalyticsEventType::SessionStart,
            "c4f24cc9-acde-4d20-87e1-1d6bfa8e7a67",
            "7b7b7d6d-5a0f-4b76-9c7c-4d7ff6f1b0b3",
            now,
            properties,
        );

        assert_eq!(event.event, "session_start");
        assert_eq!(event.client_id, "c4f24cc9-acde-4d20-87e1-1d6bfa8e7a67");
        assert_eq!(event.session_id, "7b7b7d6d-5a0f-4b76-9c7c-4d7ff6f1b0b3");
        assert!(event.timestamp.contains("2025-01-15T10:30:00"));
        assert_eq!(event.properties.version, "1.0.0");
    }
}
