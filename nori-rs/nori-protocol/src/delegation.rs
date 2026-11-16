//! Text shared by every surface that names a delegated child or reads its result.
use crate::Delegation;
use crate::DelegationOrigin;
use crate::DelegationStatus;

/// First line of every result prompt a parent agent receives from a child.
pub const DELEGATION_RESULT_HEADER: &str = "[Nori delegation result]";

const NAME_CHARS: i32 = 40;

const CHILD_LINE_PREFIX: &str = "Child session: ";

/// What a session without a title or task is called.
pub const UNTITLED_SESSION_NAME: &str = "new session";

/// The first non-empty line of a session's task or title, whole.
pub fn session_full_name(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

/// A short readable name for a session, taken from the first non-empty line of
/// its task or title. A long line keeps its start and end, so sessions whose
/// names share a long prefix still read apart.
pub fn session_display_name(text: &str) -> Option<String> {
    session_full_name(text).map(|line| shorten_name(&line, NAME_CHARS))
}

/// `name` in at most `limit` characters, cut in the middle with `…`.
pub fn shorten_name(name: &str, limit: i32) -> String {
    let limit = usize::try_from(limit).unwrap_or_default();
    let count = name.chars().count();
    if count <= limit {
        return name.to_owned();
    }
    if limit == 0 {
        return String::new();
    }
    let room = limit.saturating_sub(1);
    let head: String = name.chars().take(room.div_ceil(2)).collect();
    let tail: String = name.chars().skip(count - room / 2).collect();
    format!("{}…{}", head.trim_end(), tail.trim_start())
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
/// its prompt names no session. A delegation the user started with
/// `/delegate` says so before the result.
pub fn delegation_result_prompt(delegation: &Delegation) -> String {
    let header = format!(
        "{DELEGATION_RESULT_HEADER} from {} · {} · {}",
        delegation.agent,
        session_full_name(&delegation.task).unwrap_or_else(|| UNTITLED_SESSION_NAME.to_owned()),
        delegation_status_word(delegation.status),
    );
    let result = delegation.result.as_deref().unwrap_or_default();
    // A parent agent that did not ask for this child would otherwise think
    // someone else started it.
    let origin = match delegation.origin {
        DelegationOrigin::User => USER_STARTED_LINE,
        DelegationOrigin::Agent => "",
    };
    if delegation.status == DelegationStatus::Declined {
        return format!("{header}\n{origin}{result}");
    }
    format!(
        "{header}\n{CHILD_LINE_PREFIX}{}\n{origin}{result}",
        delegation.child_id
    )
}

const USER_STARTED_LINE: &str =
    "The user started this delegation with /delegate; you did not request it.\n";

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

/// The parts of a result prompt's readable summary line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DelegationResultParts<'a> {
    pub agent: &'a str,
    pub name: &'a str,
    pub status: &'a str,
}

/// The agent, child name and status a result prompt names, or `None` for
/// other text.
pub fn delegation_result_parts(prompt: &str) -> Option<DelegationResultParts<'_>> {
    let rest = delegation_result_summary(prompt)?.strip_prefix("from ")?;
    let (agent, rest) = rest.split_once(PARTS_SEPARATOR)?;
    let (name, status) = rest.rsplit_once(PARTS_SEPARATOR)?;
    Some(DelegationResultParts {
        agent,
        name,
        status,
    })
}

const PARTS_SEPARATOR: &str = " · ";

#[cfg(test)]
mod tests {
    use super::*;
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
             The user started this delegation with /delegate; you did not request it.\n\
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
             The user started this delegation with /delegate; you did not request it.\n\
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
    fn long_names_keep_their_start_and_end_so_shared_prefixes_stay_apart() {
        let parent =
            session_display_name("shared prefix for the parser review: parent side").unwrap();
        let child = session_display_name(
            "shared prefix for the parser review: child side mock:request-permission:z",
        )
        .unwrap();
        assert!(parent.chars().count() <= 40, "{parent}");
        assert!(child.chars().count() <= 40, "{child}");
        assert!(parent.starts_with("shared prefix"), "{parent}");
        assert!(parent.ends_with("parent side"), "{parent}");
        assert!(child.ends_with("permission:z"), "{child}");
        assert_eq!(
            session_display_name("Review the parser").as_deref(),
            Some("Review the parser")
        );
    }

    #[test]
    fn a_result_header_names_the_child_by_its_whole_task_line() {
        let task = "mock:request-permission:review-the-lexer-tokens and the parser too";
        let prompt = delegation_result_prompt(&delegation(
            DelegationStatus::Completed,
            task,
            Some("Looks good."),
        ));
        let parts = delegation_result_parts(&prompt).expect("a result prompt");
        assert_eq!(parts.agent, "codex");
        assert_eq!(parts.name, task);
        assert_eq!(parts.status, "done");
        let dotted = delegation_result_prompt(&delegation(
            DelegationStatus::Failed,
            "lint · then test",
            Some("Agent process exited unexpectedly."),
        ));
        assert_eq!(
            delegation_result_parts(&dotted),
            Some(DelegationResultParts {
                agent: "codex",
                name: "lint · then test",
                status: "failed",
            })
        );
        assert_eq!(delegation_result_parts("Review the parser"), None);
    }

    #[test]
    fn a_child_without_a_task_line_has_a_readable_placeholder_name() {
        assert_eq!(
            delegation_child_name(&delegation(DelegationStatus::Running, "  \n ", None)),
            "new session"
        );
    }

    #[test]
    fn a_delegation_the_agent_asked_for_does_not_say_the_user_started_it() {
        let mut requested = delegation(
            DelegationStatus::Completed,
            "Review the parser",
            Some("Done."),
        );
        requested.origin = DelegationOrigin::Agent;
        let prompt = delegation_result_prompt(&requested);
        assert_eq!(
            prompt,
            "[Nori delegation result] from codex · Review the parser · done\n\
             Child session: aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\n\
             Done."
        );
    }
}
