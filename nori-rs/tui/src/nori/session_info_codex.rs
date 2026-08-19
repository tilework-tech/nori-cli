//! Friendly projections for known Codex ACP session metadata.

use std::collections::BTreeSet;

use nori_protocol::acp::v1 as acp;
use serde_json::Value;

use super::session_info::Assignment;
use super::session_info::MAX_DISPLAY_CHARS;
use super::session_info::sanitize;

const MAX_WAITING_FLAGS: usize = 8;

pub(super) fn assignments(
    meta: &acp::Meta,
    consumed: &mut BTreeSet<Vec<String>>,
) -> Vec<Assignment> {
    let mut assignments = Vec::new();

    if let Some(status) = string_value(meta, &["codex", "threadStatus", "type"]) {
        consumed.insert(path(&["codex", "threadStatus", "type"]));
        assignments.push(Assignment {
            name: "status".to_string(),
            value: sanitize(status, MAX_DISPLAY_CHARS),
        });
    }

    if let Some(flags) = value_at(meta, &["codex", "threadStatus", "activeFlags"])
        .and_then(Value::as_array)
        .filter(|flags| flags.len() <= MAX_WAITING_FLAGS)
    {
        let mut waiting = Vec::with_capacity(flags.len());
        let mut valid = true;
        for flag in flags {
            let Some(waiting_value) = (match flag.as_str() {
                Some("waitingOnApproval") => Some("approval"),
                Some("waitingOnUserInput") => Some("user_input"),
                _ => None,
            }) else {
                valid = false;
                break;
            };
            waiting.push(waiting_value);
        }
        if valid {
            consumed.insert(path(&["codex", "threadStatus", "activeFlags"]));
            assignments.extend(waiting.into_iter().map(|value| Assignment {
                name: "waiting".to_string(),
                value: value.to_string(),
            }));
        }
    }

    push_known_string(
        meta,
        consumed,
        &mut assignments,
        &["codex", "goal", "objective"],
        "goal.objective",
    );
    push_known_string(
        meta,
        consumed,
        &mut assignments,
        &["codex", "goal", "status"],
        "goal.status",
    );
    if let Some(value) = value_at(meta, &["codex", "goal", "tokenBudget"]).and_then(Value::as_i64) {
        consumed.insert(path(&["codex", "goal", "tokenBudget"]));
        assignments.push(Assignment {
            name: "goal.token_budget".to_string(),
            value: format_integer(value),
        });
    }
    if let Some(value) =
        value_at(meta, &["codex", "goal", "timeUsedSeconds"]).and_then(Value::as_i64)
    {
        consumed.insert(path(&["codex", "goal", "timeUsedSeconds"]));
        assignments.push(Assignment {
            name: "goal.time_used".to_string(),
            value: format!("{value}s"),
        });
    }
    if let Some(value) = value_at(meta, &["codex", "goal", "createdAt"]).and_then(Value::as_i64) {
        consumed.insert(path(&["codex", "goal", "createdAt"]));
        assignments.push(Assignment {
            name: "goal.created_at".to_string(),
            value: value.to_string(),
        });
    }
    push_known_string(
        meta,
        consumed,
        &mut assignments,
        &["codex", "goal", "controlMethod"],
        "goal.control_method",
    );

    push_known_string(
        meta,
        consumed,
        &mut assignments,
        &["codex", "error", "message"],
        "error.message",
    );
    push_known_string(
        meta,
        consumed,
        &mut assignments,
        &["codex", "error", "turnId"],
        "error.turn_id",
    );
    push_known_string(
        meta,
        consumed,
        &mut assignments,
        &["codex", "error", "codexErrorInfo"],
        "error.info",
    );
    push_known_string(
        meta,
        consumed,
        &mut assignments,
        &["codex", "error", "additionalDetails"],
        "error.details",
    );
    push_known_bool(
        meta,
        consumed,
        &mut assignments,
        &["codex", "error", "willRetry"],
        "error.will_retry",
    );
    push_known_bool(
        meta,
        consumed,
        &mut assignments,
        &["codex", "archived"],
        "archived",
    );
    push_known_bool(
        meta,
        consumed,
        &mut assignments,
        &["codex", "closed"],
        "closed",
    );

    assignments
}

fn push_known_string(
    meta: &acp::Meta,
    consumed: &mut BTreeSet<Vec<String>>,
    assignments: &mut Vec<Assignment>,
    field_path: &[&str],
    name: &str,
) {
    if let Some(value) = string_value(meta, field_path) {
        consumed.insert(path(field_path));
        assignments.push(Assignment {
            name: name.to_string(),
            value: sanitize(value, MAX_DISPLAY_CHARS),
        });
    }
}

fn push_known_bool(
    meta: &acp::Meta,
    consumed: &mut BTreeSet<Vec<String>>,
    assignments: &mut Vec<Assignment>,
    field_path: &[&str],
    name: &str,
) {
    if let Some(value) = value_at(meta, field_path).and_then(Value::as_bool) {
        consumed.insert(path(field_path));
        assignments.push(Assignment {
            name: name.to_string(),
            value: value.to_string(),
        });
    }
}

fn string_value<'a>(meta: &'a acp::Meta, field_path: &[&str]) -> Option<&'a str> {
    value_at(meta, field_path).and_then(Value::as_str)
}

fn value_at<'a>(meta: &'a acp::Meta, field_path: &[&str]) -> Option<&'a Value> {
    let (first, rest) = field_path.split_first()?;
    let mut value = meta.get(*first)?;
    for segment in rest {
        value = value.as_object()?.get(*segment)?;
    }
    Some(value)
}

fn path(segments: &[&str]) -> Vec<String> {
    segments
        .iter()
        .map(|segment| (*segment).to_string())
        .collect()
}

fn format_integer(value: i64) -> String {
    let raw = value.to_string();
    let (sign, digits) = raw
        .strip_prefix('-')
        .map_or(("", raw.as_str()), |digits| ("-", digits));
    let mut formatted = String::with_capacity(raw.len() + digits.len() / 3);
    formatted.push_str(sign);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }
    formatted
}
