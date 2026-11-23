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
use unicode_width::UnicodeWidthStr;

use crate::history_cell::HistoryCell;

/// Result lines shown on screen before the rest moves to the transcript view.
const RESULT_PREVIEW_LINES: i32 = 6;

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

/// A card keeps the word it was printed with, so a running child's card says
/// it started rather than claiming it is still working.
fn status_span(status: DelegationStatus) -> Span<'static> {
    let word = match status {
        DelegationStatus::Running => "started",
        DelegationStatus::AwaitingApproval
        | DelegationStatus::Completed
        | DelegationStatus::Failed
        | DelegationStatus::Declined
        | DelegationStatus::Cancelled => nori_protocol::delegation_status_word(status),
    };
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
fn child_spans(delegation: &Delegation, name: String) -> Vec<Span<'static>> {
    vec![delegation.agent.clone().bold(), " · ".dim(), name.into()]
}

/// The fewest cells a card gives the child's name before it lets the
/// header wrap instead.
const MIN_CARD_NAME: usize = 8;

/// The collapsed line a delivered result prompt reads as, after its `› `
/// marker, in at most `width` cells when it can: ` · delivered to this
/// session` gives way first, then the child's name is cut in the middle.
/// `None` for text that is not a result prompt.
pub(crate) fn delivered_result_spans(message: &str, width: usize) -> Option<Vec<Span<'static>>> {
    const DELIVERED: &str = " · delivered to this session";
    const MIN_NAME: usize = 4;
    let parts = nori_protocol::delegation_result_parts(message)?;
    let lead = format!("from {} · ", parts.agent);
    let status = format!(" · {}", parts.status);
    let fixed = "delegation result ".width() + lead.width() + status.width();
    let name_width = parts.name.width();
    let (name, delivered) = if fixed + name_width + DELIVERED.width() <= width {
        (parts.name.to_owned(), true)
    } else {
        let room = width.saturating_sub(fixed).max(MIN_NAME);
        (crate::session_path::fit_name(parts.name, room), false)
    };
    let mut spans = vec![
        "delegation result ".dim(),
        lead.into(),
        name.into(),
        status.into(),
    ];
    if delivered {
        spans.push(DELIVERED.dim());
    }
    Some(spans)
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
                "Would you like to start a session with {} for your task?",
                delegation.agent
            ),
            "you, with /delegate".to_owned(),
        ),
        DelegationOrigin::Agent => (
            format!(
                "Would you like {} to start a session with {}?",
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

    /// The header in one row of `width` cells when it can: the child's name
    /// is cut in the middle to the room the rest leaves.
    fn header(&self, width: u16) -> Line<'static> {
        let full_name = nori_protocol::session_full_name(&self.delegation.task)
            .unwrap_or_else(|| nori_protocol::UNTITLED_SESSION_NAME.to_owned());
        let fixed = self.header_spans(String::new(), false).width();
        let room = usize::from(width).saturating_sub(fixed).max(MIN_CARD_NAME);
        self.header_spans(crate::session_path::fit_name(&full_name, room), true)
    }

    /// The header with `name` for the child; a failure or cancellation reason
    /// follows only `with_reason`, and wraps rather than taking the name's
    /// room.
    fn header_spans(&self, name: String, with_reason: bool) -> Line<'static> {
        let delegation = &self.delegation;
        let mut spans = match delegation.status {
            DelegationStatus::AwaitingApproval | DelegationStatus::Running => {
                let mut spans = vec![
                    "↳ ".dim(),
                    format!("{} delegated to ", requester(delegation)).into(),
                ];
                spans.extend(child_spans(delegation, name));
                spans
            }
            DelegationStatus::Completed => {
                let mut spans = vec!["○ ".green(), "result from ".into()];
                spans.extend(child_spans(delegation, name));
                spans
            }
            DelegationStatus::Failed => {
                let mut spans = vec!["✕ ".red()];
                spans.extend(child_spans(delegation, name));
                spans
            }
            DelegationStatus::Declined | DelegationStatus::Cancelled => {
                let mut spans = vec!["✕ ".dim(), "delegation to ".dim()];
                spans.extend(child_spans(delegation, name));
                spans
            }
        };
        spans.push(" · ".dim());
        spans.push(status_span(delegation.status));
        if delegation.status == DelegationStatus::Declined {
            spans.push(" by you".dim());
        }
        let reason = first_line(delegation.result.as_deref().unwrap_or_default());
        if with_reason
            && matches!(
                delegation.status,
                DelegationStatus::Failed | DelegationStatus::Cancelled
            )
            && !reason.is_empty()
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

    /// The result text under a rail, each source line wrapped on its own so
    /// continuation rows keep the rail.
    fn result_lines(&self, width: u16, limit: Option<i32>) -> Vec<Line<'static>> {
        if self.delegation.status != DelegationStatus::Completed {
            return Vec::new();
        }
        let result = self.delegation.result.as_deref().unwrap_or_default();
        let lines: Vec<&str> = result.trim_end().lines().collect();
        if lines.is_empty() {
            return vec![Line::from(vec!["  ▏ ".dim(), "(no text)".dim().italic()])];
        }
        let shown = limit
            .and_then(|limit| usize::try_from(limit).ok())
            .map_or(lines.len(), |limit| limit.min(lines.len()));
        let rail = || Line::from("  ▏ ".dim());
        let mut out: Vec<Line<'static>> = lines[..shown]
            .iter()
            .flat_map(|line| {
                crate::wrapping::word_wrap_lines(
                    [Line::from((*line).to_owned())],
                    crate::wrapping::RtOptions::new(usize::from(width.max(1)))
                        .initial_indent(rail())
                        .subsequent_indent(rail()),
                )
            })
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
        let mut lines = vec![self.header(width)];
        lines.extend(self.result_lines(width, Some(RESULT_PREVIEW_LINES)));
        lines.extend(self.footer());
        wrap_each(&lines, width)
    }

    fn transcript_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![self.header(width)];
        let mut brief = self.delegation.task.lines();
        lines.push(Line::from(vec![
            "  brief ".dim(),
            brief.next().unwrap_or_default().to_owned().into(),
        ]));
        lines.extend(brief.map(|line| Line::from(format!("        {line}"))));
        lines.extend(self.result_lines(width, None));
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
