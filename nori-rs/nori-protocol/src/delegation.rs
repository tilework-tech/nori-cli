//! Text shared by every surface that names a delegated child or reads its result.
use crate::Delegation;
use crate::DelegationStatus;

/// First line of every result prompt a parent agent receives from a child.
pub const DELEGATION_RESULT_HEADER: &str = "[Nori delegation result]";

const NAME_CHARS: usize = 40;

/// A short readable name for a session, taken from the first non-empty line of
/// its task or title.
pub fn session_display_name(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    if line.chars().count() <= NAME_CHARS {
        return Some(line.to_owned());
    }
    let cut: String = line.chars().take(NAME_CHARS - 1).collect();
    Some(format!("{}…", cut.trim_end()))
}

/// The readable name of a delegated child.
pub fn delegation_child_name(delegation: &Delegation) -> String {
    session_display_name(&delegation.task).unwrap_or_else(|| {
        let id: String = delegation.child_id.chars().take(8).collect();
        format!("session {id}")
    })
}

/// The word used for a delegation's state, matching the session status words.
pub fn delegation_status_word(status: DelegationStatus) -> &'static str {
    match status {
        DelegationStatus::AwaitingApproval => "waiting on you",
        DelegationStatus::Running => "working",
        DelegationStatus::Completed => "ready",
        DelegationStatus::Failed => "failed",
        DelegationStatus::Declined => "declined",
        DelegationStatus::Cancelled => "cancelled",
    }
}

/// The prompt that carries a child's outcome back to its parent agent.
///
/// The first line names the child readably; the second keeps its ID so the
/// agent can still refer to that session.
pub fn delegation_result_prompt(delegation: &Delegation) -> String {
    let outcome = match delegation.status {
        DelegationStatus::Completed => "completed",
        other => delegation_status_word(other),
    };
    format!(
        "{DELEGATION_RESULT_HEADER} from {} · {} · {outcome}\nChild session: {}\n{}",
        delegation.agent,
        delegation_child_name(delegation),
        delegation.child_id,
        delegation.result.as_deref().unwrap_or_default(),
    )
}

/// The readable summary line of a result prompt, or `None` for other text.
pub fn delegation_result_summary(prompt: &str) -> Option<&str> {
    let first = prompt.lines().next()?;
    first
        .strip_prefix(DELEGATION_RESULT_HEADER)
        .map(str::trim)
        .filter(|summary| !summary.is_empty())
}
