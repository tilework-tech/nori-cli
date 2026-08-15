//! Structured ACP session-info state and user-facing history rendering.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use nori_protocol::ReplaySource;
use nori_protocol::acp::MaybeUndefined;
use nori_protocol::acp::v1 as acp;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use serde_json::Value;

use crate::history_cell::PlainHistoryCell;
use crate::presentation::SessionInfoPatch;

pub(crate) use super::session_info_state::SessionInfoState;

const MAX_ASSIGNMENTS: usize = 64;
const MAX_METADATA_DEPTH: usize = 16;
const MAX_AGENT_NAME_CHARS: usize = 80;
const MAX_AGENT_VERSION_CHARS: usize = 40;
pub(super) const MAX_DISPLAY_CHARS: usize = 160;
const MAX_PATH_CHARS: usize = 120;
const MAX_PATH_SEGMENT_CHARS: usize = 64;
/// Characters of an agent-supplied session title kept for single-line status
/// surfaces (the footer segment and the `/status` card). Titles are
/// agent-controlled free text and are routinely a whole paragraph long.
pub(crate) const MAX_TITLE_DISPLAY_CHARS: usize = 48;

/// How much ACP session-info detail reaches the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionInfoDetail {
    /// Render the full metadata dump. Unstable builds only: it documents what
    /// a harness supports and how agents differ, which is worth the noise
    /// while developing against an agent.
    Metadata,
    /// Render nothing. The merged state still feeds the footer and the
    /// `/status` card, so the useful part of the update survives.
    Hidden,
}

impl SessionInfoDetail {
    pub(crate) fn for_build() -> Self {
        if crate::version::is_unstable_build() {
            Self::Metadata
        } else {
            Self::Hidden
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SessionInfoOrigin {
    TranscriptReplay,
    AgentReplay,
    Live,
}

impl SessionInfoOrigin {
    pub(crate) fn from_replay_source(source: Option<ReplaySource>) -> Self {
        match source {
            Some(ReplaySource::Transcript) => Self::TranscriptReplay,
            Some(ReplaySource::Agent) => Self::AgentReplay,
            None => Self::Live,
        }
    }

    fn suffix(self) -> &'static str {
        match self {
            Self::TranscriptReplay => " (transcript replay)",
            Self::AgentReplay => " (agent replay)",
            Self::Live => "",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Assignment {
    pub(super) name: String,
    pub(super) value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionInfoDisplay {
    header: String,
    assignments: Vec<Assignment>,
}

impl SessionInfoDisplay {
    pub(crate) fn text(&self) -> String {
        if self.assignments.is_empty() {
            return self.header.clone();
        }
        format!(
            "{}\n  {}",
            self.header,
            self.assignments
                .iter()
                .map(|assignment| format!("{}={}", assignment.name, assignment.value))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }

    pub(crate) fn history_cell(&self) -> PlainHistoryCell {
        let mut lines = vec![Line::from(vec!["• ".dim(), self.header.clone().into()])];
        if !self.assignments.is_empty() {
            let mut fields: Vec<Span<'static>> = vec!["  ".into()];
            for (index, assignment) in self.assignments.iter().enumerate() {
                if index > 0 {
                    fields.push(", ".into());
                }
                fields.push(assignment.name.clone().into());
                fields.push("=".into());
                fields.push(assignment.value.clone().cyan().bold());
            }
            lines.push(Line::from(fields));
        }
        PlainHistoryCell::new(lines)
    }
}

/// Render one session-info patch, or `None` when the build suppresses the
/// metadata dump.
pub(crate) fn display(
    agent_info: Option<&acp::Implementation>,
    fallback_agent_name: &str,
    patch: &SessionInfoPatch,
    origin: SessionInfoOrigin,
    detail: SessionInfoDetail,
) -> Option<SessionInfoDisplay> {
    match detail {
        SessionInfoDetail::Metadata => {}
        SessionInfoDetail::Hidden => return None,
    }

    let agent_name = agent_info
        .and_then(|agent| agent.title.as_deref())
        .filter(|title| !title.is_empty())
        .or_else(|| {
            agent_info
                .map(|agent| agent.name.as_str())
                .filter(|name| !name.is_empty())
        })
        .or_else(|| (!fallback_agent_name.is_empty()).then_some(fallback_agent_name))
        .unwrap_or("Agent");
    let agent_name = sanitize(agent_name, MAX_AGENT_NAME_CHARS);
    let version = agent_info
        .map(|agent| agent.version.as_str())
        .filter(|version| !version.is_empty())
        .map(|version| format!(" {}", sanitize(version, MAX_AGENT_VERSION_CHARS)))
        .unwrap_or_default();
    let header = format!("{agent_name}{version} session updated{}:", origin.suffix());

    let mut assignments = Vec::new();
    if let Some(value) = maybe_undefined_assignment(&patch.title) {
        assignments.push(Assignment {
            name: "title".to_string(),
            value,
        });
    }
    if let Some(value) = maybe_undefined_assignment(&patch.updated_at) {
        assignments.push(Assignment {
            name: "updated_at".to_string(),
            value,
        });
    }

    if let Some(meta) = &patch.meta {
        let mut consumed = BTreeSet::new();
        if agent_info.is_some_and(is_codex_agent) {
            assignments.extend(super::session_info_codex::assignments(meta, &mut consumed));
        }
        let mut unknown = Vec::new();
        let unknown_limit = MAX_ASSIGNMENTS.saturating_sub(assignments.len());
        let truncated = collect_unknown_assignments(meta, &consumed, &mut unknown, unknown_limit);
        assignments.extend(unknown);
        if truncated {
            assignments.truncate(MAX_ASSIGNMENTS.saturating_sub(1));
            assignments.push(Assignment {
                name: "metadata.omitted".to_string(),
                value: "<more>".to_string(),
            });
        }
    }

    Some(SessionInfoDisplay {
        header,
        assignments,
    })
}

fn is_codex_agent(agent: &acp::Implementation) -> bool {
    const CODEX_NAME: &str = "codex-acp";
    const CODEX_SUFFIX: &str = "/codex-acp";

    agent.name.eq_ignore_ascii_case(CODEX_NAME)
        || agent
            .name
            .len()
            .checked_sub(CODEX_SUFFIX.len())
            .and_then(|start| agent.name.get(start..))
            .is_some_and(|suffix| suffix.eq_ignore_ascii_case(CODEX_SUFFIX))
}

fn maybe_undefined_assignment(value: &MaybeUndefined<String>) -> Option<String> {
    match value {
        MaybeUndefined::Undefined => None,
        MaybeUndefined::Null => Some("<null>".to_string()),
        MaybeUndefined::Value(value) => Some(sanitize(value, MAX_DISPLAY_CHARS)),
    }
}

fn collect_unknown_assignments(
    meta: &acp::Meta,
    consumed: &BTreeSet<Vec<String>>,
    assignments: &mut Vec<Assignment>,
    limit: usize,
) -> bool {
    if limit == 0 {
        return !meta.is_empty();
    }
    let (sorted, map_truncated) = sorted_prefix(meta, limit.saturating_add(consumed.len()));
    for (key, value) in sorted {
        if assignments.len() >= limit
            || collect_unknown_value(
                vec![sanitize(key, MAX_PATH_SEGMENT_CHARS)],
                value,
                consumed,
                assignments,
                limit,
                0,
            )
        {
            return true;
        }
    }
    map_truncated
}

fn collect_unknown_value(
    field_path: Vec<String>,
    value: &Value,
    consumed: &BTreeSet<Vec<String>>,
    assignments: &mut Vec<Assignment>,
    limit: usize,
    depth: usize,
) -> bool {
    if consumed.contains(&field_path) {
        return false;
    }
    if let Value::Object(object) = value
        && !object.is_empty()
    {
        if depth >= MAX_METADATA_DEPTH {
            if assignments.len() >= limit {
                return true;
            }
            assignments.push(Assignment {
                name: display_path(&field_path),
                value: "<object>".to_string(),
            });
            return true;
        }
        let remaining = limit.saturating_sub(assignments.len());
        let (sorted, map_truncated) =
            sorted_prefix(object, remaining.saturating_add(consumed.len()));
        for (key, child) in sorted {
            if assignments.len() >= limit {
                return true;
            }
            let mut child_path = field_path.clone();
            child_path.push(sanitize(key, MAX_PATH_SEGMENT_CHARS));
            if collect_unknown_value(child_path, child, consumed, assignments, limit, depth + 1) {
                return true;
            }
        }
        return map_truncated;
    }

    if assignments.len() >= limit {
        return true;
    }
    assignments.push(Assignment {
        name: display_path(&field_path),
        value: json_type(value).to_string(),
    });
    false
}

fn sorted_prefix(
    object: &serde_json::Map<String, Value>,
    limit: usize,
) -> (Vec<(&String, &Value)>, bool) {
    let mut prefix = BTreeMap::new();
    let mut truncated = false;
    for (key, value) in object {
        prefix.insert(key, value);
        if prefix.len() > limit {
            prefix.pop_last();
            truncated = true;
        }
    }
    (prefix.into_iter().collect(), truncated)
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "<null>",
        Value::Bool(_) => "<boolean>",
        Value::Number(_) => "<number>",
        Value::String(_) => "<string>",
        Value::Array(_) => "<array>",
        Value::Object(_) => "<object>",
    }
}

pub(crate) fn sanitize(value: &str, max_chars: usize) -> String {
    let mut characters = value.chars();
    let mut sanitized = String::with_capacity(max_chars.saturating_add(1));
    for character in characters.by_ref().take(max_chars) {
        sanitized.push(if character.is_control() {
            ' '
        } else {
            character
        });
    }
    if characters.next().is_some() {
        sanitized.push('…');
    }
    sanitized
}

fn display_path(segments: &[String]) -> String {
    let mut path = String::with_capacity(MAX_PATH_CHARS.saturating_add(1));
    let mut truncated = false;
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 && !push_bounded(&mut path, ".", MAX_PATH_CHARS) {
            truncated = true;
            break;
        }
        if !push_bounded(&mut path, segment, MAX_PATH_CHARS) {
            truncated = true;
            break;
        }
    }
    if truncated {
        path.pop();
        path.push('…');
    }
    path
}

fn push_bounded(target: &mut String, value: &str, max_chars: usize) -> bool {
    let remaining = max_chars.saturating_sub(target.chars().count());
    for character in value.chars().take(remaining) {
        target.push(character);
    }
    value.chars().nth(remaining).is_none()
}
