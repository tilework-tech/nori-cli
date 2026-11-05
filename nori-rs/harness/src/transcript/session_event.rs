use nori_protocol::AcpEvent;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use serde::Deserialize;
use serde::Serialize;

use super::types::now_iso8601;

/// Original public event and Nori's observation metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "StoredSessionEvent")]
pub struct SessionEventEntry {
    pub event: SessionEvent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub historical_evidence: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub history_incomplete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_kind: Option<String>,
}

impl SessionEventEntry {
    pub(crate) fn new(event: SessionEvent) -> Self {
        let response_kind = match &event {
            SessionEvent::Acp(AcpEvent::Response {
                response: Ok(response),
                ..
            }) => match response {
                acp::AgentResponse::PromptResponse(_) => Some("prompt"),
                acp::AgentResponse::NewSessionResponse(_) => Some("new_session"),
                acp::AgentResponse::ForkSessionResponse(_) => Some("fork"),
                _ => None,
            },
            _ => None,
        }
        .map(str::to_string);
        Self {
            event,
            observation_id: Some(uuid::Uuid::new_v4().to_string()),
            observed_at: Some(now_iso8601()),
            historical_evidence: false,
            history_incomplete: false,
            response_kind,
        }
    }
}

impl PartialEq for SessionEventEntry {
    fn eq(&self, other: &Self) -> bool {
        serde_json::to_value(self).ok() == serde_json::to_value(other).ok()
    }
}

#[derive(Deserialize)]
struct StoredSessionEvent {
    event: serde_json::Value,
    #[serde(default)]
    observation_id: Option<String>,
    #[serde(default)]
    observed_at: Option<String>,
    #[serde(default)]
    historical_evidence: bool,
    #[serde(default)]
    history_incomplete: bool,
    #[serde(default)]
    response_kind: Option<String>,
}

impl TryFrom<StoredSessionEvent> for SessionEventEntry {
    type Error = serde_json::Error;

    fn try_from(stored: StoredSessionEvent) -> Result<Self, Self::Error> {
        let mut event: SessionEvent = serde_json::from_value(stored.event.clone())?;
        // ACP's untagged response enum lets an empty authentication response
        // absorb other payloads. The private kind retains the original variant.
        if let SessionEvent::Acp(AcpEvent::Response { response, .. }) = &mut event
            && let Some(payload) = stored.event.pointer("/event/response/Ok")
        {
            let kind = stored
                .response_kind
                .as_deref()
                .or_else(|| payload.get("stopReason").map(|_| "prompt"));
            let restored = match kind {
                Some("prompt") => Some(acp::AgentResponse::PromptResponse(serde_json::from_value(
                    payload.clone(),
                )?)),
                Some("new_session") => Some(acp::AgentResponse::NewSessionResponse(
                    serde_json::from_value(payload.clone())?,
                )),
                Some("fork") => Some(acp::AgentResponse::ForkSessionResponse(
                    serde_json::from_value(payload.clone())?,
                )),
                _ => None,
            };
            if let Some(restored) = restored {
                *response = Ok(restored);
            }
        }
        Ok(Self {
            event,
            observation_id: stored.observation_id,
            observed_at: stored.observed_at,
            historical_evidence: stored.historical_evidence,
            history_incomplete: stored.history_incomplete,
            response_kind: stored.response_kind,
        })
    }
}
