//! Analytics event sending
//!
//! Provides fire-and-forget analytics event sending for install tracking.

use crate::state::InstallSource;
use crate::state::InstallState;
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

const EXECUTABLE_NAME: &str = "nori-ai-cli";

/// Analytics event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalyticsEventType {
    InstallCompleted,
    SessionStart,
    UserResurrected,
}

impl AnalyticsEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            AnalyticsEventType::InstallCompleted => "nori_install_completed",
            AnalyticsEventType::SessionStart => "nori_session_start",
            AnalyticsEventType::UserResurrected => "nori_user_resurrected",
        }
    }
}

/// Analytics event request payload
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
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
    if event_type == AnalyticsEventType::InstallCompleted {
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
        "tilework_cli_user_id": state.client_id.as_str(),
        "tilework_cli_executable_name": EXECUTABLE_NAME,
        "tilework_cli_installed_version": state.installed_version.as_str(),
        "tilework_cli_install_source": install_source_to_string(state.install_source),
        "tilework_cli_days_since_install": days_since_install,
        "tilework_cli_session_id": session_id,
        "tilework_cli_timestamp": timestamp.to_rfc3339_opts(SecondsFormat::Millis, true),
        "tilework_cli_os": std::env::consts::OS,
        "tilework_cli_arch": std::env::consts::ARCH,
        "tilework_cli_node_version": node_version(),
        "tilework_cli_is_ci": is_ci_env(),
    })
}

fn install_source_to_string(source: InstallSource) -> &'static str {
    match source {
        InstallSource::Npm => "npm",
        InstallSource::Bun => "bun",
        InstallSource::Unknown => "unknown",
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
    fn test_create_install_completed_event_first_install() {
        let state = create_test_state();
        let now = Utc.with_ymd_and_hms(2025, 1, 15, 10, 30, 0).unwrap();
        let event = create_event(
            AnalyticsEventType::InstallCompleted,
            &state,
            "7b7b7d6d-5a0f-4b76-9c7c-4d7ff6f1b0b3",
            now,
            0,
            true,
            None,
        );

        assert_eq!(event.event_name, "nori_install_completed");
        assert_eq!(event.client_id, state.client_id);
        assert_eq!(event.user_id, state.client_id);

        let params = &event.event_params;
        assert_eq!(params["tilework_cli_user_id"], state.client_id);
        assert_eq!(params["tilework_cli_install_source"], "bun");
        assert_eq!(params["tilework_cli_installed_version"], "1.0.0");
        assert_eq!(params["tilework_cli_is_first_install"], true);
        assert_eq!(params["tilework_cli_days_since_install"], 0);
        assert_eq!(params["tilework_cli_executable_name"], "nori-ai-cli");
        assert!(params.get("tilework_cli_previous_version").is_none());
    }

    #[test]
    fn test_create_install_completed_event_upgrade() {
        let mut state = create_test_state();
        state.installed_version = "2.0.0".to_string();
        state.install_source = InstallSource::Npm;

        let now = Utc.with_ymd_and_hms(2025, 1, 20, 10, 30, 0).unwrap();
        let event = create_event(
            AnalyticsEventType::InstallCompleted,
            &state,
            "7b7b7d6d-5a0f-4b76-9c7c-4d7ff6f1b0b3",
            now,
            5,
            false,
            Some("1.0.0".to_string()),
        );

        let params = &event.event_params;
        assert_eq!(params["tilework_cli_user_id"], state.client_id);
        assert_eq!(params["tilework_cli_install_source"], "npm");
        assert_eq!(params["tilework_cli_installed_version"], "2.0.0");
        assert_eq!(params["tilework_cli_is_first_install"], false);
        assert_eq!(params["tilework_cli_previous_version"], "1.0.0");
        assert_eq!(params["tilework_cli_days_since_install"], 5);
    }

    #[test]
    fn test_create_session_start_event() {
        let state = create_test_state();
        let now = Utc.with_ymd_and_hms(2025, 1, 20, 10, 30, 0).unwrap();
        let event = create_event(
            AnalyticsEventType::SessionStart,
            &state,
            "7b7b7d6d-5a0f-4b76-9c7c-4d7ff6f1b0b3",
            now,
            5,
            false,
            None,
        );

        assert_eq!(event.event_name, "nori_session_start");

        let params = &event.event_params;
        assert_eq!(params["tilework_cli_user_id"], state.client_id);
        assert_eq!(params["tilework_cli_installed_version"], "1.0.0");
        assert_eq!(params["tilework_cli_install_source"], "bun");
        assert_eq!(params["tilework_cli_days_since_install"], 5);
        assert!(params.get("tilework_cli_is_first_install").is_none());
        assert!(params.get("tilework_cli_previous_version").is_none());
    }
}
