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
    pub cost: Option<SessionCost>,
}

/// Cumulative session cost from the agent's last usage update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCost {
    /// The amount in hundredths of the currency unit, rounded.
    pub hundredths: i64,
    /// ISO 4217 code, such as `USD`.
    pub currency: String,
}

impl SessionCost {
    #[must_use]
    pub fn new(amount: f64, currency: impl Into<String>) -> Self {
        Self {
            hundredths: (amount * 100.0).round() as i64,
            currency: currency.into(),
        }
    }

    /// The amount with two decimal places, such as `3.42`.
    #[must_use]
    pub fn amount(&self) -> String {
        let sign = if self.hundredths < 0 { "-" } else { "" };
        let hundredths = self.hundredths.abs();
        format!("{sign}{}.{:02}", hundredths / 100, hundredths % 100)
    }
}
