use codex_protocol::protocol::ThreadGoalStatus;
use codex_protocol::protocol::validate_thread_goal_objective;
use nori_protocol::ClientEvent;
use nori_protocol::SessionUpdateInfo;
use nori_protocol::SessionUpdateKind;
use nori_protocol::ThreadGoalUpdated;

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThreadGoalSnapshot {
    pub(crate) objective: String,
    pub(crate) status: ThreadGoalStatus,
    pub(crate) tokens_used: i64,
    pub(crate) time_used_seconds: i64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

impl ThreadGoalSnapshot {
    pub(crate) fn into_client_goal(self) -> nori_protocol::ThreadGoal {
        nori_protocol::ThreadGoal {
            objective: self.objective,
            status: client_status(self.status),
            tokens_used: self.tokens_used,
            time_used_seconds: self.time_used_seconds,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredThreadGoal {
    objective: String,
    status: ThreadGoalStatus,
    tokens_used: i64,
    accumulated_active_seconds: i64,
    active_started_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ThreadGoalState {
    goal: Option<StoredThreadGoal>,
}

impl ThreadGoalState {
    pub(crate) fn snapshot(&self, now: i64) -> Option<ThreadGoalSnapshot> {
        self.goal.as_ref().map(|goal| goal.snapshot(now))
    }

    pub(crate) fn set_objective(
        &mut self,
        objective: String,
        status: Option<ThreadGoalStatus>,
        now: i64,
    ) -> Result<ThreadGoalSnapshot, String> {
        validate_thread_goal_objective(&objective)?;
        let status = status.unwrap_or(ThreadGoalStatus::Active);
        self.goal = Some(StoredThreadGoal {
            objective,
            status,
            tokens_used: 0,
            accumulated_active_seconds: 0,
            active_started_at: active_started_at(status, now),
            created_at: now,
            updated_at: now,
        });
        Ok(self.snapshot(now).expect("goal was just set"))
    }

    pub(crate) fn set_status(
        &mut self,
        status: ThreadGoalStatus,
        now: i64,
    ) -> Result<ThreadGoalSnapshot, String> {
        let Some(goal) = self.goal.as_mut() else {
            return Err("cannot update goal: no goal exists".to_string());
        };
        goal.apply_status(status, now);
        Ok(goal.snapshot(now))
    }

    pub(crate) fn clear(&mut self) -> bool {
        self.goal.take().is_some()
    }
}

impl StoredThreadGoal {
    fn snapshot(&self, now: i64) -> ThreadGoalSnapshot {
        ThreadGoalSnapshot {
            objective: self.objective.clone(),
            status: self.status,
            tokens_used: self.tokens_used,
            time_used_seconds: self.active_seconds(now),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    fn apply_status(&mut self, status: ThreadGoalStatus, now: i64) {
        self.accumulated_active_seconds = self.active_seconds(now);
        self.status = status;
        self.active_started_at = active_started_at(status, now);
        self.updated_at = now;
    }

    fn active_seconds(&self, now: i64) -> i64 {
        let current_active_seconds = self
            .active_started_at
            .map(|started_at| now.saturating_sub(started_at))
            .unwrap_or(0);
        self.accumulated_active_seconds + current_active_seconds
    }
}

fn active_started_at(status: ThreadGoalStatus, now: i64) -> Option<i64> {
    match status {
        ThreadGoalStatus::Active => Some(now),
        ThreadGoalStatus::Paused
        | ThreadGoalStatus::Blocked
        | ThreadGoalStatus::UsageLimited
        | ThreadGoalStatus::BudgetLimited
        | ThreadGoalStatus::Complete => None,
    }
}

fn client_status(status: ThreadGoalStatus) -> nori_protocol::ThreadGoalStatus {
    match status {
        ThreadGoalStatus::Active => nori_protocol::ThreadGoalStatus::Active,
        ThreadGoalStatus::Paused => nori_protocol::ThreadGoalStatus::Paused,
        ThreadGoalStatus::Blocked => nori_protocol::ThreadGoalStatus::Blocked,
        ThreadGoalStatus::UsageLimited => nori_protocol::ThreadGoalStatus::UsageLimited,
        ThreadGoalStatus::BudgetLimited => nori_protocol::ThreadGoalStatus::BudgetLimited,
        ThreadGoalStatus::Complete => nori_protocol::ThreadGoalStatus::Complete,
    }
}

pub(super) fn now_seconds() -> i64 {
    let now = std::time::SystemTime::now();
    now.duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

impl AcpBackend {
    pub(super) async fn handle_thread_goal_get(&self) {
        let now = now_seconds();
        let goal = self.thread_goal_state.lock().await.snapshot(now);
        match goal {
            Some(goal) => {
                self.emit_thread_goal_updated(goal).await;
            }
            None => {
                emit_client_event(
                    &self.backend_event_tx,
                    self.transcript_recorder.as_ref(),
                    ClientEvent::SessionUpdateInfo(SessionUpdateInfo {
                        kind: SessionUpdateKind::SessionInfo,
                        message: "Usage: /goal <objective>".to_string(),
                        hint: Some("No goal is currently set.".to_string()),
                        usage: None,
                    }),
                )
                .await;
            }
        }
    }

    pub(super) async fn handle_thread_goal_set(
        &self,
        objective: Option<String>,
        status: Option<ThreadGoalStatus>,
    ) {
        let now = now_seconds();
        let result = {
            let mut state = self.thread_goal_state.lock().await;
            match objective {
                Some(objective) => state.set_objective(objective, status, now),
                None => match status {
                    Some(status) => state.set_status(status, now),
                    None => Err("goal update must include an objective or status".to_string()),
                },
            }
        };

        match result {
            Ok(goal) => self.emit_thread_goal_updated(goal).await,
            Err(message) => self.send_error(&message).await,
        }
    }

    pub(super) async fn handle_thread_goal_clear(&self) {
        let cleared = self.thread_goal_state.lock().await.clear();
        if cleared {
            emit_client_event(
                &self.backend_event_tx,
                self.transcript_recorder.as_ref(),
                ClientEvent::ThreadGoalCleared,
            )
            .await;
        } else {
            emit_client_event(
                &self.backend_event_tx,
                self.transcript_recorder.as_ref(),
                ClientEvent::SessionUpdateInfo(SessionUpdateInfo {
                    kind: SessionUpdateKind::SessionInfo,
                    message: "No goal to clear".to_string(),
                    hint: Some("This session does not currently have a goal.".to_string()),
                    usage: None,
                }),
            )
            .await;
        }
    }

    async fn emit_thread_goal_updated(&self, goal: ThreadGoalSnapshot) {
        emit_client_event(
            &self.backend_event_tx,
            self.transcript_recorder.as_ref(),
            ClientEvent::ThreadGoalUpdated(ThreadGoalUpdated {
                goal: goal.into_client_goal(),
            }),
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use codex_protocol::protocol::ThreadGoalStatus;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn setting_objective_creates_active_goal() {
        let mut goals = ThreadGoalState::default();

        let goal = goals
            .set_objective(
                "Ship the ACP goal command".to_string(),
                Some(ThreadGoalStatus::Active),
                10,
            )
            .expect("valid objective");

        assert_eq!(goal.objective, "Ship the ACP goal command");
        assert_eq!(goal.status, ThreadGoalStatus::Active);
        assert_eq!(goal.tokens_used, 0);
        assert_eq!(goal.time_used_seconds, 0);
        assert_eq!(goal.created_at, 10);
        assert_eq!(goal.updated_at, 10);
    }

    #[test]
    fn updating_status_preserves_objective_and_accumulates_active_time() {
        let mut goals = ThreadGoalState::default();
        goals
            .set_objective("Keep going".to_string(), None, 10)
            .expect("valid objective");

        let goal = goals
            .set_status(ThreadGoalStatus::Paused, 25)
            .expect("existing goal");

        assert_eq!(goal.objective, "Keep going");
        assert_eq!(goal.status, ThreadGoalStatus::Paused);
        assert_eq!(goal.time_used_seconds, 15);
        assert_eq!(goal.created_at, 10);
        assert_eq!(goal.updated_at, 25);
    }

    #[test]
    fn paused_time_does_not_accumulate_until_resumed() {
        let mut goals = ThreadGoalState::default();
        goals
            .set_objective("Keep going".to_string(), None, 10)
            .expect("valid objective");
        goals
            .set_status(ThreadGoalStatus::Paused, 25)
            .expect("existing goal");
        goals
            .set_status(ThreadGoalStatus::Active, 100)
            .expect("existing goal");

        let goal = goals.snapshot(130).expect("goal exists");

        assert_eq!(goal.status, ThreadGoalStatus::Active);
        assert_eq!(goal.time_used_seconds, 45);
    }

    #[test]
    fn clearing_goal_reports_whether_goal_existed() {
        let mut goals = ThreadGoalState::default();
        assert_eq!(goals.clear(), false);
        goals
            .set_objective("Keep going".to_string(), None, 10)
            .expect("valid objective");

        assert_eq!(goals.clear(), true);
        assert_eq!(goals.snapshot(20), None);
    }
}
