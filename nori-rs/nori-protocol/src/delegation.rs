//! Text shared by every surface that names a delegated child or reads its result.
use crate::Delegation;
use crate::DelegationStatus;

/// First line of every result prompt a parent agent receives from a child.
pub const DELEGATION_RESULT_HEADER: &str = "[Nori delegation result]";

const NAME_CHARS: i32 = 40;

const CHILD_LINE_PREFIX: &str = "Child session: ";

/// What a session without a title or task is called.
pub const UNTITLED_SESSION_NAME: &str = "new session";

/// A short readable name for a session, taken from the first non-empty line of
/// its task or title.
pub fn session_display_name(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let limit = usize::try_from(NAME_CHARS).unwrap_or_default();
    if line.chars().count() <= limit {
        return Some(line.to_owned());
    }
    let cut: String = line.chars().take(limit - 1).collect();
    Some(format!("{}…", cut.trim_end()))
}

/// The readable name of a delegated child.
pub fn delegation_child_name(delegation: &Delegation) -> String {
    session_display_name(&delegation.task).unwrap_or_else(|| UNTITLED_SESSION_NAME.to_owned())
}

/// The word used for a delegation's state, matching the session status words.
pub fn delegation_status_word(status: DelegationStatus) -> &'static str {
    match status {
        DelegationStatus::AwaitingApproval => "waiting on you",
        DelegationStatus::Running => "working",
        DelegationStatus::Completed => "done",
        DelegationStatus::Failed => "failed",
        DelegationStatus::Declined => "declined",
        DelegationStatus::Cancelled => "cancelled",
    }
}

/// The prompt that carries a child's outcome back to its parent agent.
///
/// The first line names the child readably; the second keeps its ID so the
/// agent can still refer to that session. A declined child never started, so
/// its prompt names no session.
pub fn delegation_result_prompt(delegation: &Delegation) -> String {
    let header = format!(
        "{DELEGATION_RESULT_HEADER} from {} · {} · {}",
        delegation.agent,
        delegation_child_name(delegation),
        delegation_status_word(delegation.status),
    );
    let result = delegation.result.as_deref().unwrap_or_default();
    if delegation.status == DelegationStatus::Declined {
        return format!("{header}\n{result}");
    }
    format!(
        "{header}\n{CHILD_LINE_PREFIX}{}\n{result}",
        delegation.child_id
    )
}

/// The readable summary line of a result prompt, or `None` for other text.
pub fn delegation_result_summary(prompt: &str) -> Option<&str> {
    let mut lines = prompt.lines();
    let summary = lines
        .next()?
        .strip_prefix(DELEGATION_RESULT_HEADER)
        .map(str::trim)
        .filter(|summary| !summary.is_empty())?;
    let declined = summary.ends_with(&format!(
        " · {}",
        delegation_status_word(DelegationStatus::Declined)
    ));
    (declined || lines.next()?.starts_with(CHILD_LINE_PREFIX)).then_some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DelegationOrigin;
    use pretty_assertions::assert_eq;

    fn delegation(status: DelegationStatus, task: &str, result: Option<&str>) -> Delegation {
        Delegation {
            request_id: "request-1".into(),
            parent_id: "11111111-2222-3333-4444-555555555555".into(),
            child_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into(),
            parent_agent: "claude-code".into(),
            agent: "codex".into(),
            task: task.into(),
            status,
            depth: 1,
            result: result.map(str::to_owned),
            origin: DelegationOrigin::User,
        }
    }

    #[test]
    fn a_finished_child_reports_done_with_its_answer_and_session() {
        let prompt = delegation_result_prompt(&delegation(
            DelegationStatus::Completed,
            "Review the parser\nwith care",
            Some("Looks good."),
        ));
        assert_eq!(
            prompt,
            "[Nori delegation result] from codex · Review the parser · done\n\
             Child session: aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\n\
             Looks good."
        );
        assert_eq!(
            delegation_result_summary(&prompt),
            Some("from codex · Review the parser · done")
        );
    }

    #[test]
    fn a_declined_delegation_names_no_session_and_still_reads_as_a_result() {
        let prompt = delegation_result_prompt(&delegation(
            DelegationStatus::Declined,
            "Review the parser",
            Some("The user declined this delegation."),
        ));
        assert_eq!(
            prompt,
            "[Nori delegation result] from codex · Review the parser · declined\n\
             The user declined this delegation."
        );
        assert_eq!(
            delegation_result_summary(&prompt),
            Some("from codex · Review the parser · declined")
        );
    }

    #[test]
    fn text_that_only_starts_with_the_header_is_not_a_result() {
        assert_eq!(
            delegation_result_summary("[Nori delegation result] from me\nhello"),
            None
        );
        assert_eq!(delegation_result_summary("Review the parser"), None);
    }

    #[test]
    fn a_child_without_a_task_line_has_a_readable_placeholder_name() {
        assert_eq!(
            delegation_child_name(&delegation(DelegationStatus::Running, "  \n ", None)),
            "new session"
        );
    }
}
