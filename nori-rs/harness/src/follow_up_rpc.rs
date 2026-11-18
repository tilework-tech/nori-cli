//! Parameters accepted on the authenticated host control and binding transports.

use anyhow::Result;
use anyhow::ensure;
use nori_protocol::acp::v1 as acp;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::FollowUpIntent;

pub(crate) const METHOD: &str = "_nori/session/follow_up";

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FollowUpParams {
    pub session_id: String,
    pub expected_turn: acp::RequestId,
    pub message_id: String,
    pub intent: FollowUpIntent,
    pub prompt: Vec<acp::ContentBlock>,
}

impl FollowUpParams {
    pub fn parse(value: Value) -> Result<Self> {
        let params: Self = serde_json::from_value(value)?;
        ensure!(
            params.expected_turn != acp::RequestId::Null,
            "expected_turn must identify a turn"
        );
        ensure!(!params.session_id.is_empty(), "session_id is required");
        ensure!(
            !params.message_id.trim().is_empty() && params.message_id.len() <= 256,
            "invalid message_id"
        );
        ensure!(
            params.prompt.iter().any(
                |block| !matches!(block, acp::ContentBlock::Text(text) if text.text.is_empty())
            ),
            "prompt content is empty"
        );
        Ok(params)
    }
}
