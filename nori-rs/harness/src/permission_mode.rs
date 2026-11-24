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
        nori_protocol::ApprovalMode::from(self).name()
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

impl From<PermissionMode> for nori_protocol::ApprovalMode {
    fn from(mode: PermissionMode) -> Self {
        match mode {
            PermissionMode::Ask => Self::Ask,
            PermissionMode::Agent => Self::Agent,
            PermissionMode::ApproveAll => Self::ApproveAll,
        }
    }
}

impl From<nori_protocol::ApprovalMode> for PermissionMode {
    fn from(mode: nori_protocol::ApprovalMode) -> Self {
        match mode {
            nori_protocol::ApprovalMode::Ask => Self::Ask,
            nori_protocol::ApprovalMode::Agent => Self::Agent,
            nori_protocol::ApprovalMode::ApproveAll => Self::ApproveAll,
        }
    }
}

/// The modes a session offers: all three when its agent can ask before
/// every edit and command, otherwise Agent and Approve all.
pub fn available(ask_available: bool) -> Vec<nori_protocol::ApprovalMode> {
    PermissionMode::ALL
        .into_iter()
        .filter(|mode| ask_available || *mode != PermissionMode::Ask)
        .map(nori_protocol::ApprovalMode::from)
        .collect()
}

/// What clients are told about a session running under `policy`.
pub fn announced(policy: AskForApproval, ask_available: bool) -> nori_protocol::SessionEvent {
    nori_protocol::SessionEvent::Nori(nori_protocol::NoriEvent::ApprovalModeChanged(
        nori_protocol::SessionApprovalModes {
            current: PermissionMode::from_approval_policy(policy).into(),
            available: available(ask_available),
        },
    ))
}
