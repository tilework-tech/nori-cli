//! A session's context report: its usage at prompt boundaries, the latest
//! reading, and the instruction files its agent loads, as seen on the host.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

use crate::acp::v1 as acp;

/// Context history derived from raw ACP and Harness lifecycle events.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextTimeline {
    pub turns: Vec<ContextTurn>,
    pub current: Option<ContextObservation>,
    pub segments: Vec<ContextSegment>,
    pub incomplete_history: bool,
    pub epoch: i64,
}

/// One admitted prompt, with immutable boundary observations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextTurn {
    pub number: i64,
    pub request_id: acp::RequestId,
    pub start: ContextCheckpoint,
    pub end: Option<ContextCheckpoint>,
}

/// The latest known usage when a prompt boundary was observed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextCheckpoint {
    pub recorded_at: String,
    pub observation: Option<ContextObservation>,
    pub epoch: i64,
}

/// An original ACP usage payload with stable observation metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextObservation {
    pub id: String,
    pub observed_at: String,
    pub usage: acp::UsageUpdate,
}

/// Observed growth attributed to a turn, or an unattributed baseline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextSegment {
    pub turn: Option<i64>,
    pub tokens: i64,
}

impl ContextTimeline {
    /// Latest turn's observed growth, when its readings are comparable.
    pub fn current_turn_delta(&self) -> Option<i64> {
        let turn = self.turns.last()?;
        if turn.end.is_some() {
            return turn.delta();
        }
        let start = turn.start.observation.as_ref()?;
        let current = self.current.as_ref()?;
        if turn.start.epoch != self.epoch
            || start.usage.size != current.usage.size
            || start.id == current.id
        {
            return None;
        }
        let used = i64::try_from(current.usage.used).ok()?;
        used.checked_sub(i64::try_from(start.usage.used).ok()?)
    }
}

impl ContextTurn {
    /// Observed change between completed boundaries, when comparable.
    pub fn delta(&self) -> Option<i64> {
        let end = self.end.as_ref()?;
        if self.start.epoch != end.epoch {
            return None;
        }
        let start = self.start.observation.as_ref()?;
        let end = end.observation.as_ref()?;
        if start.usage.size != end.usage.size || start.id == end.id {
            return None;
        }
        i64::try_from(end.usage.used)
            .ok()?
            .checked_sub(i64::try_from(start.usage.used).ok()?)
    }
}

/// Params of `_nori/session/context`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextParams {
    pub session_id: String,
    /// Leave `instructions` empty, for a caller that wants only the usage.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip_instructions: bool,
}

/// An instruction file the session's agent loads, with its path as the host
/// shows it (`~` is the host's home) and its token count when known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextInstruction {
    pub path: String,
    pub tokens: Option<i64>,
}

/// The latest usage the agent reported for the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextUsage {
    pub used: i64,
    pub size: i64,
}

/// Result of `_nori/session/context`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContextReport {
    pub timeline: ContextTimeline,
    /// The active instruction files in the session's folder on the host.
    pub instructions: Vec<ContextInstruction>,
    /// The latest usage, kept after its update leaves the replay window.
    pub usage: Option<ContextUsage>,
}
