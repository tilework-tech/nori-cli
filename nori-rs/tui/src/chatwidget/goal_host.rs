//! The goal of a session on the host is read and changed with
//! `_nori/session/goal/*`, so every terminal on the host shows and changes
//! the same goal. A widget learns its session's goal from the host when the
//! session comes on screen, and ignores a goal for any other session.

use super::*;
use crate::app_event::HarnessAction;
use crate::app_event_sender::HostReply;
use crate::host_client::HostCall;
use nori_protocol::GoalOp;
use nori_protocol::ThreadGoalStatus;

/// The goal statuses a terminal sets on a host; the agent sets the others.
const ONLY_ACTIVE_OR_PAUSED: &str = "The host sets a goal only to active or paused.";

impl ChatWidget {
    fn host_goal_session(&self) -> Option<String> {
        self.managed_conversation_id
            .filter(|_| !self.cloud_mode)
            .map(|id| id.to_string())
    }

    /// Sends a goal action of a host session to the host; any other action,
    /// or any widget without one, is returned for the harness.
    pub(super) fn goal_action_on_host(&self, action: HarnessAction) -> Option<HarnessAction> {
        let Some(session_id) = self.host_goal_session() else {
            return Some(action);
        };
        let op = match action {
            HarnessAction::LoadGoal => {
                self.load_goal_from_host(session_id, true);
                return None;
            }
            HarnessAction::SetGoal { objective, .. } => GoalOp::Set { objective },
            HarnessAction::SetGoalStatus(ThreadGoalStatus::Paused) => GoalOp::Pause,
            HarnessAction::SetGoalStatus(ThreadGoalStatus::Active) => GoalOp::Resume,
            HarnessAction::SetGoalStatus(
                ThreadGoalStatus::Blocked
                | ThreadGoalStatus::UsageLimited
                | ThreadGoalStatus::BudgetLimited
                | ThreadGoalStatus::Complete,
            ) => {
                self.app_event_tx
                    .send(AppEvent::HarnessActionFailed(ONLY_ACTIVE_OR_PAUSED.into()));
                return None;
            }
            HarnessAction::ClearGoal => GoalOp::Clear,
            action @ (HarnessAction::Cancel
            | HarnessAction::ReclaimQueuedPrompt { .. }
            | HarnessAction::ResumeQueue
            | HarnessAction::Shutdown { .. }
            | HarnessAction::Compact
            | HarnessAction::Branch
            | HarnessAction::UndoTo(_)
            | HarnessAction::LoadUndoSnapshots
            | HarnessAction::RunUserShell(_)
            | HarnessAction::AddHistory(_)
            | HarnessAction::HistoryEntry { .. }
            | HarnessAction::SearchHistory { .. }
            | HarnessAction::LoadCustomPrompts) => return Some(action),
        };
        // Reading the goal is open to watchers; changing it is not.
        if let Some(reason) = self.host_input_refusal() {
            self.app_event_tx
                .send(AppEvent::HarnessActionFailed(reason));
            return None;
        }
        self.app_event_tx.send(AppEvent::HostRequest {
            call: HostCall::SetGoal { session_id, op },
            reply: HostReply::ReportError(self.app_event_tx.clone()),
        });
        None
    }

    /// Learn the goal of the host session now on screen, without showing it.
    pub(super) fn load_host_goal_quietly(&self) {
        if let Some(session_id) = self.host_goal_session() {
            self.load_goal_from_host(session_id, false);
        }
    }

    fn load_goal_from_host(&self, session_id: String, asked: bool) {
        let tx = self.app_event_tx.clone();
        let reply = tx.host_receipt(HostCall::GetGoal {
            session_id: session_id.clone(),
        });
        tokio::spawn(async move {
            let loaded = reply.await.and_then(|reply| {
                serde_json::from_value::<nori_protocol::GoalResult>(reply).map_err(Into::into)
            });
            match loaded {
                Ok(loaded) => tx.send(AppEvent::HostGoalLoaded {
                    session_id,
                    goal: loaded.goal,
                    asked,
                }),
                Err(error) if asked => tx.send(AppEvent::HarnessActionFailed(format!(
                    "Could not read the goal: {error}"
                ))),
                // A session the host no longer runs, such as a finished
                // child, has no goal to learn.
                Err(error) => tracing::debug!(%error, "goal unavailable from the host"),
            }
        });
    }

    /// A host session's goal arrived. It is kept only by the widget showing
    /// that session, and shown only when the user asked for it.
    pub(crate) fn on_host_goal_loaded(
        &mut self,
        session_id: &str,
        goal: Option<nori_protocol::ThreadGoal>,
        asked: bool,
    ) {
        if self.host_goal_session().as_deref() != Some(session_id) {
            return;
        }
        match (goal, asked) {
            (Some(goal), true) => self.handle_thread_goal_updated(goal),
            (None, true) => {
                self.current_goal = None;
                self.pending_goal_status = false;
                self.pending_goal_edit = false;
                self.add_info_message("No goal is currently set.".to_string(), None);
                self.request_redraw();
            }
            // A live change that arrived after the question is newer.
            (_, false) if self.live_goal_seen => {}
            (goal, false) => self.current_goal = goal,
        }
    }
}
