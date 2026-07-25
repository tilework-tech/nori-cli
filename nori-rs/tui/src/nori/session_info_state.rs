//! Latest-state reduction for partial ACP session-info patches.

use std::collections::BTreeMap;

use nori_protocol::acp::MaybeUndefined;
use serde_json::Value;

use super::session_info::SessionInfoOrigin;
use crate::presentation::SessionInfoPatch;

const MAX_METADATA_DEPTH: usize = 16;
const MAX_PATCH_NODES: usize = 128;
const MAX_STATE_FIELDS: usize = 64;
const MAX_RETAINED_VALUE_NODES: usize = 64;
const MAX_RETAINED_STRING_CHARS: usize = 1024;
const MAX_PATH_SEGMENT_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamped<T> {
    value: T,
    origin: SessionInfoOrigin,
}

/// Latest structured session state. Partial metadata objects merge
/// recursively; live data outranks agent replay, which outranks transcript
/// replay.
#[derive(Debug, Default)]
pub(crate) struct SessionInfoState {
    title: Option<Stamped<Option<String>>>,
    updated_at: Option<Stamped<Option<String>>>,
    meta: BTreeMap<Vec<String>, Stamped<Value>>,
}

impl SessionInfoState {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn apply(&mut self, patch: &SessionInfoPatch, origin: SessionInfoOrigin) {
        apply_optional_field(&mut self.title, &patch.title, origin);
        apply_optional_field(&mut self.updated_at, &patch.updated_at, origin);

        if let Some(meta) = &patch.meta {
            let mut visited = 0;
            for (key, value) in meta {
                let Some(key) = bounded_key(key) else {
                    continue;
                };
                self.apply_meta_value(vec![key], value, origin, 0, &mut visited);
                if visited >= MAX_PATCH_NODES {
                    break;
                }
            }
        }
    }

    fn apply_meta_value(
        &mut self,
        path: Vec<String>,
        value: &Value,
        origin: SessionInfoOrigin,
        depth: usize,
        visited: &mut usize,
    ) {
        if *visited >= MAX_PATCH_NODES {
            return;
        }
        *visited += 1;

        if let Value::Object(object) = value {
            if object.is_empty() || depth >= MAX_METADATA_DEPTH {
                return;
            }
            for (key, child) in object {
                let Some(key) = bounded_key(key) else {
                    continue;
                };
                let mut child_path = path.clone();
                child_path.push(key);
                self.apply_meta_value(child_path, child, origin, depth + 1, visited);
                if *visited >= MAX_PATCH_NODES {
                    break;
                }
            }
            return;
        }

        if self
            .meta
            .iter()
            .any(|(existing, stamped)| is_ancestor(existing, &path) && stamped.origin > origin)
            || self
                .meta
                .get(&path)
                .is_some_and(|stamped| stamped.origin > origin)
            || self
                .meta
                .iter()
                .any(|(existing, stamped)| is_ancestor(&path, existing) && stamped.origin > origin)
        {
            return;
        }

        self.meta.retain(|existing, stamped| {
            !((is_ancestor(existing, &path) || is_ancestor(&path, existing))
                && stamped.origin <= origin)
        });
        if !self.meta.contains_key(&path) && self.meta.len() >= MAX_STATE_FIELDS {
            let eviction = self
                .meta
                .iter()
                .filter(|(_, stamped)| stamped.origin <= origin)
                .min_by_key(|(_, stamped)| stamped.origin)
                .map(|(path, _)| path.clone());
            let Some(eviction) = eviction else {
                return;
            };
            self.meta.remove(&eviction);
        }
        let mut retained_nodes = 0;
        self.meta.insert(
            path,
            Stamped {
                value: bounded_value(value, 0, &mut retained_nodes),
                origin,
            },
        );
    }
}

fn apply_optional_field(
    current: &mut Option<Stamped<Option<String>>>,
    update: &MaybeUndefined<String>,
    origin: SessionInfoOrigin,
) {
    let value = match update {
        MaybeUndefined::Undefined => return,
        MaybeUndefined::Null => None,
        MaybeUndefined::Value(value) => {
            Some(value.chars().take(MAX_RETAINED_STRING_CHARS).collect())
        }
    };
    if current
        .as_ref()
        .is_none_or(|stamped| stamped.origin <= origin)
    {
        *current = Some(Stamped { value, origin });
    }
}

fn is_ancestor(ancestor: &[String], descendant: &[String]) -> bool {
    ancestor.len() < descendant.len() && descendant.starts_with(ancestor)
}

fn bounded_value(value: &Value, depth: usize, visited: &mut usize) -> Value {
    if *visited >= MAX_RETAINED_VALUE_NODES || depth >= MAX_METADATA_DEPTH {
        return Value::Null;
    }
    *visited += 1;
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(value) => {
            Value::String(value.chars().take(MAX_RETAINED_STRING_CHARS).collect())
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .take(MAX_RETAINED_VALUE_NODES.saturating_sub(*visited))
                .map(|value| bounded_value(value, depth + 1, visited))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .take(MAX_RETAINED_VALUE_NODES.saturating_sub(*visited))
                .filter_map(|(key, value)| {
                    Some((bounded_key(key)?, bounded_value(value, depth + 1, visited)))
                })
                .collect(),
        ),
    }
}

fn bounded_key(key: &str) -> Option<String> {
    key.chars()
        .nth(MAX_PATH_SEGMENT_CHARS)
        .is_none()
        .then(|| key.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(segments: &[&str]) -> Vec<String> {
        segments
            .iter()
            .map(|segment| (*segment).to_string())
            .collect()
    }

    fn patch(meta: Value) -> SessionInfoPatch {
        SessionInfoPatch {
            title: MaybeUndefined::Undefined,
            updated_at: MaybeUndefined::Undefined,
            meta: Some(meta.as_object().expect("metadata object").clone()),
        }
    }

    fn meta_value<'a>(state: &'a SessionInfoState, field_path: &[&str]) -> Option<&'a Value> {
        state
            .meta
            .get(&path(field_path))
            .and_then(|stamped| (!stamped.value.is_null()).then_some(&stamped.value))
    }

    #[test]
    fn latest_state_merges_partial_metadata_with_source_precedence() {
        let mut state = SessionInfoState::default();
        state.apply(
            &patch(serde_json::json!({
                "codex": {"threadStatus": {"type": "idle"}}
            })),
            SessionInfoOrigin::AgentReplay,
        );
        state.apply(
            &patch(serde_json::json!({
                "codex": {
                    "threadStatus": {"type": "active"},
                    "goal": {"status": "paused"}
                }
            })),
            SessionInfoOrigin::TranscriptReplay,
        );

        assert_eq!(
            meta_value(&state, &["codex", "threadStatus", "type"]),
            Some(&Value::String("idle".to_string()))
        );
        assert_eq!(
            meta_value(&state, &["codex", "goal", "status"]),
            Some(&Value::String("paused".to_string()))
        );

        state.apply(
            &patch(serde_json::json!({
                "codex": {"threadStatus": {"type": "systemError"}}
            })),
            SessionInfoOrigin::Live,
        );
        state.apply(
            &patch(serde_json::json!({
                "codex": {"threadStatus": {"type": "idle"}}
            })),
            SessionInfoOrigin::AgentReplay,
        );
        assert_eq!(
            meta_value(&state, &["codex", "threadStatus", "type"]),
            Some(&Value::String("systemError".to_string()))
        );
    }

    #[test]
    fn nested_null_clears_state_and_reset_allows_a_new_replay() {
        let mut state = SessionInfoState::default();
        state.apply(
            &patch(serde_json::json!({
                "codex": {"goal": {"status": "active"}}
            })),
            SessionInfoOrigin::Live,
        );
        state.apply(
            &patch(serde_json::json!({"codex": {"goal": null}})),
            SessionInfoOrigin::Live,
        );
        assert_eq!(meta_value(&state, &["codex", "goal", "status"]), None);

        state.reset();
        state.apply(
            &patch(serde_json::json!({
                "codex": {"goal": {"status": "restored"}}
            })),
            SessionInfoOrigin::TranscriptReplay,
        );
        assert_eq!(
            meta_value(&state, &["codex", "goal", "status"]),
            Some(&Value::String("restored".to_string()))
        );
    }

    #[test]
    fn higher_priority_ancestors_and_descendants_block_lower_priority_replay() {
        let mut state = SessionInfoState::default();
        state.apply(
            &patch(serde_json::json!({"vendor": {"branch": null}})),
            SessionInfoOrigin::Live,
        );
        state.apply(
            &patch(serde_json::json!({
                "vendor": {"branch": {"child": "transcript"}}
            })),
            SessionInfoOrigin::TranscriptReplay,
        );
        assert_eq!(meta_value(&state, &["vendor", "branch", "child"]), None);

        state.reset();
        state.apply(
            &patch(serde_json::json!({
                "vendor": {"branch": {"child": "live"}}
            })),
            SessionInfoOrigin::Live,
        );
        state.apply(
            &patch(serde_json::json!({"vendor": {"branch": null}})),
            SessionInfoOrigin::TranscriptReplay,
        );
        assert_eq!(
            meta_value(&state, &["vendor", "branch", "child"]),
            Some(&Value::String("live".to_string()))
        );
    }

    #[test]
    fn empty_objects_do_not_clear_recursively_merged_state() {
        let mut state = SessionInfoState::default();
        state.apply(
            &patch(serde_json::json!({
                "codex": {"goal": {"status": "active"}}
            })),
            SessionInfoOrigin::Live,
        );
        state.apply(
            &patch(serde_json::json!({"codex": {"goal": {}}})),
            SessionInfoOrigin::Live,
        );

        assert_eq!(
            meta_value(&state, &["codex", "goal", "status"]),
            Some(&Value::String("active".to_string()))
        );
    }

    #[test]
    fn retained_metadata_state_has_a_hard_field_limit() {
        let mut state = SessionInfoState::default();
        let fields = (0..200)
            .map(|index| (format!("field{index:03}"), Value::from(index)))
            .collect();
        state.apply(
            &SessionInfoPatch {
                title: MaybeUndefined::Undefined,
                updated_at: MaybeUndefined::Undefined,
                meta: Some(fields),
            },
            SessionInfoOrigin::TranscriptReplay,
        );

        assert!(
            state.meta.len() <= 64,
            "retained metadata grew to {} fields",
            state.meta.len()
        );
    }

    #[test]
    fn overlong_metadata_keys_are_not_retained() {
        let mut state = SessionInfoState::default();
        let overlong_key = "k".repeat(MAX_PATH_SEGMENT_CHARS + 1);
        state.apply(
            &SessionInfoPatch {
                title: MaybeUndefined::Undefined,
                updated_at: MaybeUndefined::Undefined,
                meta: Some(
                    [(overlong_key, Value::String("private".to_string()))]
                        .into_iter()
                        .collect(),
                ),
            },
            SessionInfoOrigin::Live,
        );

        assert!(state.meta.is_empty());
    }

    #[test]
    fn undefined_scalar_fields_do_not_clear_but_explicit_null_does() {
        let mut state = SessionInfoState::default();
        state.apply(
            &SessionInfoPatch {
                title: MaybeUndefined::Value("Live title".to_string()),
                updated_at: MaybeUndefined::Undefined,
                meta: None,
            },
            SessionInfoOrigin::Live,
        );
        state.apply(
            &SessionInfoPatch {
                title: MaybeUndefined::Undefined,
                updated_at: MaybeUndefined::Undefined,
                meta: None,
            },
            SessionInfoOrigin::Live,
        );
        assert_eq!(
            state
                .title
                .as_ref()
                .and_then(|value| value.value.as_deref()),
            Some("Live title")
        );

        state.apply(
            &SessionInfoPatch {
                title: MaybeUndefined::Null,
                updated_at: MaybeUndefined::Undefined,
                meta: None,
            },
            SessionInfoOrigin::Live,
        );
        assert_eq!(
            state
                .title
                .as_ref()
                .and_then(|value| value.value.as_deref()),
            None
        );
    }
}
