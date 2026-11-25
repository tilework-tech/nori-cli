//! `_nori/session/goal/*`: any terminal reads a session's goal; the
//! controller sets, pauses, resumes or clears it. Clients follow changes in
//! `goal_changed` events and find the latest goal on `_nori/session/list`
//! rows.

use nori_protocol::GoalGetParams;
use nori_protocol::GoalOp;
use nori_protocol::GoalResult;
use nori_protocol::GoalSetParams;
use nori_protocol::ThreadGoalStatus;
use serde_json::Value;
use serde_json::json;

use super::ManagedRemoteHost;
use super::RemoteError;

pub(super) const GET: &str = "_nori/session/goal/get";
pub(super) const SET: &str = "_nori/session/goal/set";

impl ManagedRemoteHost {
    pub(super) async fn goal(&self, method: &str, params: Value) -> Result<Value, RemoteError> {
        let invalid = |error: serde_json::Error| RemoteError::new(-32602, error);
        let failed = |error: anyhow::Error| RemoteError::new(-32000, format!("{error:#}"));
        let goal = match method {
            GET => {
                let GoalGetParams { session_id } =
                    serde_json::from_value(params).map_err(invalid)?;
                self.running(&session_id)?.goal().await.map_err(failed)?
            }
            SET => {
                let GoalSetParams { session_id, op } =
                    serde_json::from_value(params).map_err(invalid)?;
                let handle = self.running(&session_id)?;
                match op {
                    GoalOp::Set { objective } => Some(
                        handle
                            .set_goal(objective, Some(ThreadGoalStatus::Active))
                            .await
                            .map_err(failed)?,
                    ),
                    GoalOp::Pause => Some(
                        handle
                            .set_goal_status(ThreadGoalStatus::Paused)
                            .await
                            .map_err(failed)?,
                    ),
                    GoalOp::Resume => Some(
                        handle
                            .set_goal_status(ThreadGoalStatus::Active)
                            .await
                            .map_err(failed)?,
                    ),
                    GoalOp::Clear => {
                        handle.clear_goal().await.map_err(failed)?;
                        None
                    }
                }
            }
            _ => return Err(RemoteError::new(-32601, "method not found")),
        };
        Ok(json!(GoalResult { goal }))
    }
}
