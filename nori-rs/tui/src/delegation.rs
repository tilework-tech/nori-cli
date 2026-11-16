//! Shared command syntax and minimal delegation labels for both terminal frontends.
use nori_protocol::Delegation;
use nori_protocol::DelegationStatus;

pub(crate) fn parse_command(text: &str) -> Option<Result<(String, String), String>> {
    let rest = text.strip_prefix("/delegate")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let Some((agent, task)) = rest.trim().split_once(char::is_whitespace) else {
        return Some(Err("Usage: /delegate <agent> <task>".into()));
    };
    let task = task.trim();
    Some(if task.is_empty() {
        Err("Usage: /delegate <agent> <task>".into())
    } else {
        Ok((agent.to_owned(), task.to_owned()))
    })
}

pub(crate) fn summary(delegation: &Delegation) -> String {
    let status = match delegation.status {
        DelegationStatus::AwaitingApproval => "awaiting approval",
        DelegationStatus::Running => "running",
        DelegationStatus::Declined => "declined",
        DelegationStatus::Completed => "completed",
        DelegationStatus::Failed => "failed",
        DelegationStatus::Cancelled => "cancelled",
    };
    let mut message = format!(
        "Child {} ({}) · parent {} ({}) · {status}",
        delegation.child_id, delegation.agent, delegation.parent_id, delegation.parent_agent,
    );
    if let Some(result) = &delegation.result {
        message.push('\n');
        message.push_str(result);
    }
    message
}
