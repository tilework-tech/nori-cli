//! Delivery receipts for user input submitted against a known, running turn.

use nori_protocol::acp::v1 as acp;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowUpIntent {
    Steer,
    Queue,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowUpStatus {
    Injected,
    Queued,
    UnsupportedQueued,
    IdleQueued,
    Rejected,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FollowUpReceipt {
    pub message_id: String,
    pub sender: String,
    pub status: FollowUpStatus,
}

pub const SENDER_META: &str = "nori.dev/followUpSender";

pub fn supports_guarded_steering(initialize: &Value) -> bool {
    initialize["protocolVersion"] == 1
        && initialize["_meta"]["steering"]["supported"] == true
        && matches!(
            (
                initialize["agentInfo"]["name"].as_str(),
                initialize["agentInfo"]["version"].as_str()
            ),
            (
                Some("@agentclientprotocol/claude-agent-acp"),
                Some("0.102.0" | "0.108.1" | "0.109.0")
            ) | (Some("@agentclientprotocol/codex-acp"), Some("2.7.0"))
        )
}

pub fn steering_params(session_id: &str, prompt: &Value) -> Value {
    json!({"sessionId":session_id,"prompt":prompt,"_meta":{"steering":{"idleBehavior":"promptRequired"}}})
}

pub fn steering_error(code: i32) -> FollowUpStatus {
    match code {
        -32601 => FollowUpStatus::UnsupportedQueued,
        -32600 | -32602 | -32000 | -32002 => FollowUpStatus::Rejected,
        _ => FollowUpStatus::Unknown,
    }
}

pub fn steering_result(result: &Value) -> FollowUpStatus {
    match result["outcome"].as_str() {
        Some("injected") => FollowUpStatus::Injected,
        Some("promptRequired") if result["reason"] == "noRunningTurn" => FollowUpStatus::IdleQueued,
        Some("failed" | "rejected") => FollowUpStatus::Rejected,
        _ => FollowUpStatus::Unknown,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) enum PromptOrigin {
    LocalComposer,
    External,
}

#[derive(Serialize)]
pub(crate) struct Submission {
    pub origin: PromptOrigin,
    pub message_id: String,
    pub sender: String,
    pub expected_turn: acp::RequestId,
    pub intent: FollowUpIntent,
    pub prompt: Vec<acp::ContentBlock>,
}

impl Submission {
    pub(crate) fn receipt(&self, status: FollowUpStatus) -> FollowUpReceipt {
        FollowUpReceipt {
            message_id: self.message_id.clone(),
            sender: self.sender.clone(),
            status,
        }
    }
}
