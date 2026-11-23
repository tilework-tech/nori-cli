//! The three permission modes host clients choose between, and the CLI
//! `approval_policy` each one stands for.

use nori_config::AskForApproval;

/// A session's permission mode, as ACP session modes and the `mode`
/// config option name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    Ask,
    Agent,
    ApproveAll,
}

impl PermissionMode {
    pub const ALL: [Self; 3] = [Self::Ask, Self::Agent, Self::ApproveAll];

    pub fn id(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Agent => "agent",
            Self::ApproveAll => "approve-all",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::Agent => "Agent",
            Self::ApproveAll => "Approve all",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Ask => "Ask before every edit and command.",
            Self::Agent => "The agent asks when it judges it needs to.",
            Self::ApproveAll => "Nori approves every permission request.",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.id() == id)
    }

    pub fn approval_policy(self) -> AskForApproval {
        match self {
            Self::Ask => AskForApproval::UnlessTrusted,
            Self::Agent => AskForApproval::OnRequest,
            Self::ApproveAll => AskForApproval::Never,
        }
    }

    /// The mode a CLI setting reads as. The legacy `on-failure` asks exactly
    /// as `on-request` does for ACP agents.
    pub fn from_approval_policy(policy: AskForApproval) -> Self {
        match policy {
            AskForApproval::UnlessTrusted => Self::Ask,
            AskForApproval::OnFailure | AskForApproval::OnRequest => Self::Agent,
            AskForApproval::Never => Self::ApproveAll,
        }
    }
}
