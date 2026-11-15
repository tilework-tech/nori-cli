//! Command syntax and presentation of delegated child sessions, shared by the
//! local and attached terminal frontends.
//!
//! A delegation reads as one compact line while its child works and as a
//! short result block once it ends. The full brief and result stay in the
//! transcript view so many delegations remain readable on screen.
use nori_protocol::Delegation;
use nori_protocol::DelegationOrigin;
use nori_protocol::DelegationStatus;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

use crate::history_cell::HistoryCell;

/// Result lines shown on screen before the rest moves to the transcript view.
const RESULT_PREVIEW_LINES: usize = 6;

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

/// Key names a frontend offers next to a delegation; `None` hides the hint.
#[derive(Clone, Debug, Default)]
pub(crate) struct DelegationHints {
    /// Opens the transcript view, where the full brief and result are shown.
    pub(crate) transcript: Option<String>,
    /// Opens the session list, where the child's history can be opened.
    pub(crate) sessions: Option<String>,
}

/// Who asked for the child, as the parent's transcript names them.
fn requester(delegation: &Delegation) -> String {
    match delegation.origin {
        DelegationOrigin::User => "you".to_owned(),
        DelegationOrigin::Agent => delegation.parent_agent.clone(),
    }
}

fn status_span(status: DelegationStatus) -> Span<'static> {
    let word = nori_protocol::delegation_status_word(status);
    match status {
        DelegationStatus::AwaitingApproval => {
            Span::styled(word, crate::style::component_theme().warning)
        }
        DelegationStatus::Running => word.cyan(),
        DelegationStatus::Completed => word.green(),
        DelegationStatus::Failed => word.red(),
        DelegationStatus::Declined | DelegationStatus::Cancelled => word.dim(),
    }
}

/// The child as `agent · name`, the form every delegation surface uses.
fn child_spans(delegation: &Delegation) -> Vec<Span<'static>> {
    vec![
        delegation.agent.clone().bold(),
        " · ".dim(),
        nori_protocol::delegation_child_name(delegation).into(),
    ]
}

fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

/// The question, attribution rows, and task shown before a child of a
/// different agent kind starts.
pub(crate) fn approval_lines(delegation: &Delegation) -> (String, Vec<Line<'static>>) {
    let (title, asked_by) = match delegation.origin {
        DelegationOrigin::User => (
            format!(
                "Would you like to start a {} session for your task?",
                delegation.agent
            ),
            "you, with /delegate".to_owned(),
        ),
        DelegationOrigin::Agent => (
            format!(
                "Would you like {} to start a {} session?",
                delegation.parent_agent, delegation.agent
            ),
            format!("{} (this session)", delegation.parent_agent),
        ),
    };
    let label = |name: &'static str| format!("{name:<10}").dim();
    let mut lines = vec![
        Line::from(vec![label("asked by"), asked_by.into()]),
        Line::from(vec![
            label("runs on"),
            delegation.agent.clone().bold(),
            " · a different agent from this session".dim(),
        ]),
    ];
    let mut task = delegation.task.lines();
    lines.push(Line::from(vec![
        label("task"),
        task.next().unwrap_or_default().to_owned().into(),
    ]));
    lines.extend(task.map(|line| Line::from(format!("{:<10}{line}", ""))));
    (title, lines)
}

/// One delegation state change in the parent's transcript.
#[derive(Debug)]
pub(crate) struct DelegationCell {
    delegation: Delegation,
    hints: DelegationHints,
}

impl DelegationCell {
    pub(crate) fn new(delegation: Delegation, hints: DelegationHints) -> Self {
        Self { delegation, hints }
    }

    fn header(&self) -> Line<'static> {
        let delegation = &self.delegation;
        let mut spans = match delegation.status {
            DelegationStatus::AwaitingApproval | DelegationStatus::Running => {
                let mut spans = vec![
                    "↳ ".dim(),
                    format!("{} delegated to ", requester(delegation)).into(),
                ];
                spans.extend(child_spans(delegation));
                spans
            }
            DelegationStatus::Completed => {
                let mut spans = vec!["○ ".green(), "result from ".into()];
                spans.extend(child_spans(delegation));
                spans
            }
            DelegationStatus::Failed => {
                let mut spans = vec!["✕ ".red()];
                spans.extend(child_spans(delegation));
                spans
            }
            DelegationStatus::Declined | DelegationStatus::Cancelled => {
                let mut spans = vec!["✕ ".dim(), "delegation to ".dim()];
                spans.extend(child_spans(delegation));
                spans
            }
        };
        spans.push(" · ".dim());
        spans.push(status_span(delegation.status));
        if delegation.status == DelegationStatus::Declined {
            spans.push(" by you".dim());
        }
        let reason = first_line(delegation.result.as_deref().unwrap_or_default());
        if matches!(
            delegation.status,
            DelegationStatus::Failed | DelegationStatus::Cancelled
        ) && !reason.is_empty()
        {
            spans.push(format!(" · {reason}").dim());
        }
        Line::from(spans)
    }

    fn footer(&self) -> Option<Line<'static>> {
        let delegation = &self.delegation;
        let mut parts = match delegation.status {
            DelegationStatus::AwaitingApproval | DelegationStatus::Running => {
                return self
                    .hints
                    .transcript
                    .as_ref()
                    .map(|key| Line::from(format!("  {key} shows the brief").dim()));
            }
            DelegationStatus::Completed => {
                vec![format!("returns to {} on its own", delegation.parent_agent)]
            }
            DelegationStatus::Failed => {
                vec![format!("sent to {} as a failure", delegation.parent_agent)]
            }
            DelegationStatus::Declined | DelegationStatus::Cancelled => return None,
        };
        if let Some(key) = &self.hints.sessions {
            parts.push(format!("its history: {key}"));
        }
        Some(Line::from(format!("  {}", parts.join(" · ")).dim()))
    }

    fn result_lines(&self, limit: Option<usize>) -> Vec<Line<'static>> {
        if self.delegation.status != DelegationStatus::Completed {
            return Vec::new();
        }
        let result = self.delegation.result.as_deref().unwrap_or_default();
        let lines: Vec<&str> = result.trim_end().lines().collect();
        if lines.is_empty() {
            return vec![Line::from(vec!["  ▏ ".dim(), "(no text)".dim().italic()])];
        }
        let shown = limit.map_or(lines.len(), |limit| limit.min(lines.len()));
        let mut out: Vec<Line<'static>> = lines[..shown]
            .iter()
            .map(|line| Line::from(vec!["  ▏ ".dim(), (*line).to_owned().into()]))
            .collect();
        if shown < lines.len() {
            let more = lines.len() - shown;
            let hint = self
                .hints
                .transcript
                .as_ref()
                .map(|key| format!(" · {key} shows all"))
                .unwrap_or_default();
            out.push(Line::from(format!("  ▏ … {more} more lines{hint}").dim()));
        }
        out
    }
}

impl HistoryCell for DelegationCell {
    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![self.header()];
        lines.extend(self.result_lines(Some(RESULT_PREVIEW_LINES)));
        lines.extend(self.footer());
        wrap_each(&lines, width)
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![self.header()];
        let mut brief = self.delegation.task.lines();
        lines.push(Line::from(vec![
            "  brief ".dim(),
            brief.next().unwrap_or_default().to_owned().into(),
        ]));
        lines.extend(brief.map(|line| Line::from(format!("        {line}"))));
        lines.extend(self.result_lines(None));
        lines.push(Line::from(
            format!("  child session {}", self.delegation.child_id).dim(),
        ));
        wrap_each(&lines, width)
    }
}

/// Wrap every line on its own so a wrapped line continues under its text.
fn wrap_each(lines: &[Line<'static>], width: u16) -> Vec<Line<'static>> {
    lines
        .iter()
        .flat_map(|line| {
            crate::wrapping::word_wrap_lines(
                [line],
                crate::wrapping::RtOptions::new(usize::from(width.max(1)))
                    .subsequent_indent("    ".into()),
            )
        })
        .collect()
}
