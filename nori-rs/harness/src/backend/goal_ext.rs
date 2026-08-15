//! Bridge types for the ACP `_session/goal` extension.
//!
//! The maintained Claude and Codex adapters advertise a goal capability in the
//! top-level `_meta` of the initialize response and publish goal snapshots in
//! the `_meta.goal` of `session_info_update` notifications. When the extension
//! is usable, the agent's native goal loop owns continuation and the harness
//! mirrors its state into `ThreadGoalState`; the nori-client MCP loop remains
//! the fallback for agents without the extension (or when an extension call
//! fails).

use nori_protocol::acp;
use serde::Deserialize;

use super::thread_goal::GoalStatus;
use super::thread_goal::ThreadGoalSnapshot;
use super::thread_goal::ThreadGoalState;

/// Wire value of the only extension version this bridge understands.
const SUPPORTED_GOAL_EXTENSION_VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GoalExtAction {
    Set,
    Pause,
    Resume,
    Clear,
}

impl GoalExtAction {
    pub(super) fn wire_name(self) -> &'static str {
        match self {
            GoalExtAction::Set => "set",
            GoalExtAction::Pause => "pause",
            GoalExtAction::Resume => "resume",
            GoalExtAction::Clear => "clear",
        }
    }

    fn from_wire(name: &str) -> Option<Self> {
        match name {
            "set" => Some(GoalExtAction::Set),
            "pause" => Some(GoalExtAction::Pause),
            "resume" => Some(GoalExtAction::Resume),
            "clear" => Some(GoalExtAction::Clear),
            _ => None,
        }
    }
}

/// Goal capability advertised by the agent at initialize.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GoalExtCapability {
    pub(super) control_method: String,
    pub(super) actions: Vec<GoalExtAction>,
}

impl GoalExtCapability {
    /// Parses `_meta.goal` from the initialize response. Returns `None` when
    /// the capability is absent, malformed, or of an unsupported version —
    /// callers then fall back to the nori-client MCP goal loop.
    pub(super) fn from_initialize_meta(meta: Option<&acp::v1::Meta>) -> Option<Self> {
        #[derive(Deserialize)]
        struct WireCapability {
            version: i64,
            #[serde(rename = "controlMethod")]
            control_method: String,
            actions: Vec<String>,
        }

        let goal = meta?.get("goal")?;
        let wire: WireCapability = serde_json::from_value(goal.clone()).ok()?;
        if wire.version != SUPPORTED_GOAL_EXTENSION_VERSION {
            return None;
        }
        if !wire.control_method.starts_with('_') {
            return None;
        }
        // Unknown actions are ignored rather than rejected so newer adapters
        // stay compatible; `set` is the floor for driving a goal at all.
        let actions: Vec<GoalExtAction> = wire
            .actions
            .iter()
            .filter_map(|a| GoalExtAction::from_wire(a))
            .collect();
        if !actions.contains(&GoalExtAction::Set) || !actions.contains(&GoalExtAction::Clear) {
            return None;
        }
        Some(GoalExtCapability {
            control_method: wire.control_method,
            actions,
        })
    }

    pub(super) fn supports(&self, action: GoalExtAction) -> bool {
        self.actions.contains(&action)
    }
}

/// One `_meta.goal` value observed on a `session_info_update` notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GoalExtUpdate {
    /// `"goal": null` — the agent-side goal was cleared.
    Cleared,
    Snapshot(GoalExtSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GoalExtSnapshot {
    pub(super) objective: String,
    pub(super) status: GoalStatus,
}

/// Extracts a goal update from a `session_info_update` `_meta`, if present.
/// Returns `None` when there is no `goal` key or the value is malformed
/// (malformed values are skipped so a buggy adapter cannot wedge goal state).
pub(super) fn goal_update_from_session_info_meta(
    meta: Option<&acp::v1::Meta>,
) -> Option<GoalExtUpdate> {
    #[derive(Deserialize)]
    struct WireSnapshot {
        objective: String,
        status: String,
    }

    let goal = meta?.get("goal")?;
    if goal.is_null() {
        return Some(GoalExtUpdate::Cleared);
    }
    let wire: WireSnapshot = serde_json::from_value(goal.clone()).ok()?;
    let status = match wire.status.as_str() {
        "active" => GoalStatus::Active,
        "paused" => GoalStatus::Paused,
        "blocked" => GoalStatus::Blocked,
        // The extension's single "limited" status folds nori's two budget
        // statuses together; usage-limited is the conservative reading.
        "limited" => GoalStatus::UsageLimited,
        "complete" => GoalStatus::Complete,
        _ => return None,
    };
    Some(GoalExtUpdate::Snapshot(GoalExtSnapshot {
        objective: wire.objective,
        status,
    }))
}

/// Result of mirroring one agent goal update into `ThreadGoalState`.
#[derive(Debug)]
pub(super) enum GoalMirrorOutcome {
    Unchanged,
    Updated(ThreadGoalSnapshot),
    Cleared,
}

/// Mirrors an agent-published goal update into the harness goal store.
/// A status change on the same objective updates the stored goal in place
/// (preserving time/token accounting); a new objective replaces it.
///
/// Guards keep a stale or competing native loop from corrupting the store:
/// nothing is mirrored while snapshots are blocked (harness stopped driving),
/// a snapshot for a different objective is ignored unless the extension
/// already owns the goal, and a null snapshot only clears extension-owned
/// goals.
pub(super) fn mirror_update(
    state: &mut ThreadGoalState,
    update: &GoalExtUpdate,
    now: i64,
) -> GoalMirrorOutcome {
    if state.ext_snapshots_blocked() {
        return GoalMirrorOutcome::Unchanged;
    }
    match update {
        GoalExtUpdate::Cleared => {
            if !state.ext_driven() {
                return GoalMirrorOutcome::Unchanged;
            }
            state.ext_cleared_by_agent();
            if state.clear() {
                GoalMirrorOutcome::Cleared
            } else {
                GoalMirrorOutcome::Unchanged
            }
        }
        GoalExtUpdate::Snapshot(snapshot) => {
            let existing = state.snapshot(now);
            if !state.ext_driven()
                && existing
                    .as_ref()
                    .is_some_and(|existing| existing.objective != snapshot.objective)
            {
                return GoalMirrorOutcome::Unchanged;
            }
            let result = match existing {
                Some(existing) if existing.objective == snapshot.objective => {
                    if existing.status == snapshot.status {
                        state.mark_ext_driven();
                        return GoalMirrorOutcome::Unchanged;
                    }
                    state.set_status(snapshot.status, now)
                }
                Some(_) | None => {
                    state.set_objective(snapshot.objective.clone(), Some(snapshot.status), now)
                }
            };
            match result {
                Ok(updated) => {
                    state.mark_ext_driven();
                    GoalMirrorOutcome::Updated(updated)
                }
                Err(error) => {
                    tracing::warn!("ignoring unusable agent goal snapshot: {error}");
                    GoalMirrorOutcome::Unchanged
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(value: serde_json::Value) -> acp::v1::Meta {
        match value {
            serde_json::Value::Object(map) => map,
            other => panic!("expected object, got {other}"),
        }
    }

    #[test]
    fn capability_parses_full_codex_advertisement() {
        let meta = meta(json!({
            "steering": { "supported": true },
            "goal": {
                "version": 1,
                "controlMethod": "_session/goal",
                "actions": ["set", "pause", "resume", "clear"],
            },
        }));
        let cap = GoalExtCapability::from_initialize_meta(Some(&meta))
            .expect("codex-style capability should parse");
        assert_eq!(cap.control_method, "_session/goal");
        assert!(cap.supports(GoalExtAction::Set));
        assert!(cap.supports(GoalExtAction::Pause));
        assert!(cap.supports(GoalExtAction::Resume));
        assert!(cap.supports(GoalExtAction::Clear));
    }

    #[test]
    fn capability_parses_minimal_claude_advertisement() {
        let meta = meta(json!({
            "goal": {
                "version": 1,
                "controlMethod": "_session/goal",
                "actions": ["set", "clear"],
            },
        }));
        let cap = GoalExtCapability::from_initialize_meta(Some(&meta))
            .expect("claude-style capability should parse");
        assert!(cap.supports(GoalExtAction::Set));
        assert!(cap.supports(GoalExtAction::Clear));
        assert!(!cap.supports(GoalExtAction::Pause));
        assert!(!cap.supports(GoalExtAction::Resume));
    }

    #[test]
    fn capability_rejects_unsupported_version() {
        let meta = meta(json!({
            "goal": { "version": 2, "controlMethod": "_session/goal", "actions": ["set", "clear"] },
        }));
        assert_eq!(GoalExtCapability::from_initialize_meta(Some(&meta)), None);
    }

    #[test]
    fn capability_requires_set_and_clear() {
        for actions in [json!(["set"]), json!(["clear"])] {
            let meta = meta(json!({
                "goal": { "version": 1, "controlMethod": "_session/goal", "actions": actions },
            }));
            assert_eq!(GoalExtCapability::from_initialize_meta(Some(&meta)), None);
        }
    }

    #[test]
    fn capability_requires_extension_method_prefix() {
        let meta = meta(json!({
            "goal": { "version": 1, "controlMethod": "session/goal", "actions": ["set", "clear"] },
        }));
        assert_eq!(GoalExtCapability::from_initialize_meta(Some(&meta)), None);
    }

    #[test]
    fn capability_ignores_unknown_actions() {
        let meta = meta(json!({
            "goal": {
                "version": 1,
                "controlMethod": "_session/goal",
                "actions": ["set", "clear", "hibernate"],
            },
        }));
        let cap = GoalExtCapability::from_initialize_meta(Some(&meta)).expect("should parse");
        assert_eq!(cap.actions.len(), 2);
    }

    #[test]
    fn capability_absent_meta_is_none() {
        assert_eq!(GoalExtCapability::from_initialize_meta(None), None);
        let no_goal = meta(json!({ "steering": { "supported": true } }));
        assert_eq!(
            GoalExtCapability::from_initialize_meta(Some(&no_goal)),
            None
        );
    }

    #[test]
    fn snapshot_parses_each_status() {
        for (wire, expected) in [
            ("active", GoalStatus::Active),
            ("paused", GoalStatus::Paused),
            ("blocked", GoalStatus::Blocked),
            ("limited", GoalStatus::UsageLimited),
            ("complete", GoalStatus::Complete),
        ] {
            let meta = meta(json!({
                "goal": { "objective": "ship it", "status": wire, "iterations": 3 },
            }));
            let update = goal_update_from_session_info_meta(Some(&meta))
                .unwrap_or_else(|| panic!("status {wire} should parse"));
            assert_eq!(
                update,
                GoalExtUpdate::Snapshot(GoalExtSnapshot {
                    objective: "ship it".to_string(),
                    status: expected,
                })
            );
        }
    }

    #[test]
    fn snapshot_null_goal_is_cleared() {
        let meta = meta(json!({ "goal": null }));
        assert_eq!(
            goal_update_from_session_info_meta(Some(&meta)),
            Some(GoalExtUpdate::Cleared)
        );
    }

    #[test]
    fn snapshot_missing_goal_key_is_none() {
        let meta = meta(json!({ "other": 1 }));
        assert_eq!(goal_update_from_session_info_meta(Some(&meta)), None);
        assert_eq!(goal_update_from_session_info_meta(None), None);
    }

    #[test]
    fn snapshot_unknown_status_is_skipped() {
        let meta = meta(json!({
            "goal": { "objective": "ship it", "status": "meditating" },
        }));
        assert_eq!(goal_update_from_session_info_meta(Some(&meta)), None);
    }

    fn snapshot(objective: &str, status: GoalStatus) -> GoalExtUpdate {
        GoalExtUpdate::Snapshot(GoalExtSnapshot {
            objective: objective.to_string(),
            status,
        })
    }

    #[test]
    fn mirror_new_snapshot_creates_goal() {
        let mut state = ThreadGoalState::default();
        let outcome = mirror_update(&mut state, &snapshot("ship it", GoalStatus::Active), 100);
        let GoalMirrorOutcome::Updated(updated) = outcome else {
            panic!("expected Updated, got {outcome:?}");
        };
        assert_eq!(updated.objective, "ship it");
        assert_eq!(updated.status, GoalStatus::Active);
        assert!(state.snapshot(100).is_some());
    }

    #[test]
    fn mirror_identical_snapshot_is_unchanged() {
        let mut state = ThreadGoalState::default();
        mirror_update(&mut state, &snapshot("ship it", GoalStatus::Active), 100);
        let outcome = mirror_update(&mut state, &snapshot("ship it", GoalStatus::Active), 160);
        assert!(matches!(outcome, GoalMirrorOutcome::Unchanged));
    }

    #[test]
    fn mirror_status_change_keeps_goal_identity() {
        let mut state = ThreadGoalState::default();
        mirror_update(&mut state, &snapshot("ship it", GoalStatus::Active), 100);
        let outcome = mirror_update(&mut state, &snapshot("ship it", GoalStatus::Complete), 160);
        let GoalMirrorOutcome::Updated(updated) = outcome else {
            panic!("expected Updated, got {outcome:?}");
        };
        assert_eq!(updated.status, GoalStatus::Complete);
        // Time accrued while active is preserved: the goal was updated in
        // place, not replaced (a replacement would reset elapsed time to 0).
        assert_eq!(updated.time_used_seconds, 60);
    }

    #[test]
    fn mirror_new_objective_replaces_goal() {
        let mut state = ThreadGoalState::default();
        mirror_update(&mut state, &snapshot("ship it", GoalStatus::Active), 100);
        let outcome = mirror_update(&mut state, &snapshot("test it", GoalStatus::Active), 160);
        let GoalMirrorOutcome::Updated(updated) = outcome else {
            panic!("expected Updated, got {outcome:?}");
        };
        assert_eq!(updated.objective, "test it");
        assert_eq!(updated.time_used_seconds, 0);
    }

    #[test]
    fn mirror_cleared_drops_goal() {
        let mut state = ThreadGoalState::default();
        mirror_update(&mut state, &snapshot("ship it", GoalStatus::Active), 100);
        assert!(matches!(
            mirror_update(&mut state, &GoalExtUpdate::Cleared, 160),
            GoalMirrorOutcome::Cleared
        ));
        assert!(state.snapshot(160).is_none());
        // Clearing an already-empty state reports no change.
        assert!(matches!(
            mirror_update(&mut state, &GoalExtUpdate::Cleared, 170),
            GoalMirrorOutcome::Unchanged
        ));
    }

    #[test]
    fn mirror_blocked_ignores_snapshots_and_clears() {
        let mut state = ThreadGoalState::default();
        mirror_update(&mut state, &snapshot("ship it", GoalStatus::Active), 100);
        state.stop_ext_driving();
        assert!(matches!(
            mirror_update(&mut state, &snapshot("ship it", GoalStatus::Complete), 160),
            GoalMirrorOutcome::Unchanged
        ));
        assert!(matches!(
            mirror_update(&mut state, &GoalExtUpdate::Cleared, 170),
            GoalMirrorOutcome::Unchanged
        ));
        assert!(state.snapshot(170).is_some());
        assert!(!state.ext_driven());
    }

    #[test]
    fn mirror_does_not_hijack_foreign_goal() {
        let mut state = ThreadGoalState::default();
        state
            .set_objective("mcp goal".to_string(), None, 100)
            .expect("set local goal");
        // Not ext-driven: a snapshot for a different objective must not steal
        // the goal, and a null snapshot must not clear it.
        assert!(matches!(
            mirror_update(&mut state, &snapshot("other goal", GoalStatus::Active), 160),
            GoalMirrorOutcome::Unchanged
        ));
        assert!(!state.ext_driven());
        assert!(matches!(
            mirror_update(&mut state, &GoalExtUpdate::Cleared, 170),
            GoalMirrorOutcome::Unchanged
        ));
        assert!(state.snapshot(170).is_some());
    }

    #[test]
    fn mirror_marks_ext_driven_and_agent_clear_reopens() {
        let mut state = ThreadGoalState::default();
        mirror_update(&mut state, &snapshot("ship it", GoalStatus::Active), 100);
        assert!(state.ext_driven());
        mirror_update(&mut state, &GoalExtUpdate::Cleared, 160);
        assert!(!state.ext_driven());
        assert!(!state.ext_snapshots_blocked());
        assert!(matches!(
            mirror_update(&mut state, &snapshot("next", GoalStatus::Active), 170),
            GoalMirrorOutcome::Updated(_)
        ));
    }

    #[test]
    fn mirror_empty_objective_is_skipped() {
        let mut state = ThreadGoalState::default();
        let outcome = mirror_update(&mut state, &snapshot("   ", GoalStatus::Active), 100);
        assert!(matches!(outcome, GoalMirrorOutcome::Unchanged));
        assert!(state.snapshot(100).is_none());
    }
}
