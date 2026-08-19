use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhaseView {
    Idle,
    Loading,
    Prompt,
    Cancelling,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionUsageState {
    pub used_tokens: i64,
    pub total_tokens: i64,
    pub cost_display: Option<String>,
}
