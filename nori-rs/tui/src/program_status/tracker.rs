//! Folds one session's host events into its program status.
//!
//! The outcome of a turn comes from the prompt's own response, matched by
//! request ID: a cancelled stop reason settles idle, any other stop reason
//! settles done, and an error response or a request failure settles error.
//! The host publishes that response before the phase returns to idle; if the
//! phase goes idle first, the turn settles done (idle after cancelling) until
//! the response corrects it.
//!
//! A permission request or a delegation waiting for approval blocks the
//! session until it is answered. Permissions also close when the turn is
//! cancelled or goes idle; a delegation waits for its own answer.

use nori_protocol::AcpEvent;
use nori_protocol::DelegationStatus;
use nori_protocol::ExternalTurnState;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEndReason;
use nori_protocol::SessionEvent;
use nori_protocol::SessionPhase;
use nori_protocol::acp::v1 as acp;

use super::ProgramState;
use super::ProgramStatus;

/// What the most recent turn left behind.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    #[default]
    Idle,
    Done,
    Error,
}

impl From<Outcome> for ProgramState {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Idle => ProgramState::Idle,
            Outcome::Done => ProgramState::Done,
            Outcome::Error => ProgramState::Error,
        }
    }
}

/// Follows one session's events and answers what the session is doing.
#[derive(Debug, Default, Clone)]
pub(crate) struct ProgramStatusTracker {
    /// The prompt whose response settles the current or latest turn.
    prompt: Option<acp::RequestId>,
    /// The harness is prompting or cancelling and no response has settled
    /// the turn yet.
    prompting: bool,
    /// The last phase was cancelling, so an idle phase means cancelled.
    cancelling: bool,
    /// The agent is running a turn it started itself.
    external_working: bool,
    outcome: Outcome,
    /// Open permission requests, oldest first, with their titles.
    permissions: Vec<(acp::RequestId, Option<String>)>,
    /// Delegations waiting for the user's approval, oldest first, with what
    /// they would start.
    delegations: Vec<(String, String)>,
}

impl ProgramStatusTracker {
    pub(crate) fn observe(&mut self, event: &SessionEvent) {
        match event {
            SessionEvent::Nori(NoriEvent::SessionPhaseChanged(phase)) => self.phase(phase),
            SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request: acp::AgentRequest::RequestPermissionRequest(request),
            }) => self
                .permissions
                .push((request_id.clone(), request.tool_call.fields.title.clone())),
            SessionEvent::Nori(NoriEvent::RequestResolved(resolved)) => {
                self.permissions
                    .retain(|(id, _)| *id != resolved.request_id);
            }
            SessionEvent::Acp(AcpEvent::Response {
                request_id,
                response,
            }) => {
                // Agent-assigned permission IDs and client request IDs are
                // separate namespaces, so a response never answers a
                // permission; `RequestResolved` does.
                if self.prompt.as_ref() == Some(request_id) {
                    self.prompt = None;
                    self.prompting = false;
                    self.cancelling = false;
                    self.outcome = match response {
                        Ok(acp::AgentResponse::PromptResponse(response))
                            if response.stop_reason == acp::StopReason::Cancelled =>
                        {
                            Outcome::Idle
                        }
                        Ok(_) => Outcome::Done,
                        Err(_) => Outcome::Error,
                    };
                }
            }
            SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                if failure.request_id.is_some() && failure.request_id == self.prompt {
                    self.prompt = None;
                    self.prompting = false;
                    self.cancelling = false;
                    self.outcome = Outcome::Error;
                }
            }
            SessionEvent::Nori(NoriEvent::DelegationChanged(delegation)) => {
                self.delegations
                    .retain(|(id, _)| *id != delegation.request_id);
                if delegation.status == DelegationStatus::AwaitingApproval {
                    let task = delegation.task.lines().next().unwrap_or_default().trim();
                    self.delegations.push((
                        delegation.request_id.clone(),
                        format!("Start {}: {task}", delegation.agent),
                    ));
                }
            }
            SessionEvent::Nori(NoriEvent::ExternalTurnChanged(turn)) => match turn.state {
                ExternalTurnState::Working
                | ExternalTurnState::CancelRequested
                | ExternalTurnState::Unconfirmed => self.external_working = true,
                ExternalTurnState::Idle => {
                    self.external_working = false;
                    self.outcome = Outcome::Done;
                }
                ExternalTurnState::Cancelled => {
                    self.external_working = false;
                    self.outcome = Outcome::Idle;
                }
                ExternalTurnState::Superseded => self.external_working = false,
            },
            SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                // A session shut down or closed on purpose keeps the outcome
                // of its last finished turn; one stopped mid-turn is idle.
                let interrupted = self.prompting || self.external_working;
                let outcome = match ended.reason {
                    SessionEndReason::Shutdown | SessionEndReason::Closed if interrupted => {
                        Outcome::Idle
                    }
                    SessionEndReason::Shutdown | SessionEndReason::Closed => self.outcome,
                    SessionEndReason::ConnectionLost
                    | SessionEndReason::SpawnFailed
                    | SessionEndReason::TimedOut => Outcome::Error,
                };
                *self = Self {
                    outcome,
                    ..Self::default()
                };
            }
            SessionEvent::Nori(NoriEvent::ReplayStarted(_)) => *self = Self::default(),
            SessionEvent::Acp(_) | SessionEvent::Nori(_) => {}
        }
    }

    fn phase(&mut self, phase: &SessionPhase) {
        match phase {
            SessionPhase::Prompting { request_id } => {
                self.prompt = Some(request_id.clone());
                self.prompting = true;
                self.cancelling = false;
            }
            SessionPhase::Cancelling { request_id } => {
                self.prompt = Some(request_id.clone());
                self.prompting = true;
                self.cancelling = true;
                self.permissions.clear();
            }
            SessionPhase::Idle => {
                if self.prompting {
                    self.outcome = if self.cancelling {
                        Outcome::Idle
                    } else {
                        Outcome::Done
                    };
                }
                self.prompting = false;
                self.cancelling = false;
                self.permissions.clear();
            }
            SessionPhase::Loading { .. } => {}
        }
    }

    pub(crate) fn status(&self) -> ProgramStatus {
        if let Some((_, title)) = self.permissions.first() {
            return ProgramStatus {
                state: ProgramState::Blocked,
                approval_title: title.clone(),
            };
        }
        if let Some((_, title)) = self.delegations.first() {
            return ProgramStatus {
                state: ProgramState::Blocked,
                approval_title: Some(title.clone()),
            };
        }
        let state = if self.prompting || self.external_working {
            ProgramState::Working
        } else {
            self.outcome.into()
        };
        ProgramStatus {
            state,
            approval_title: None,
        }
    }
}

#[cfg(test)]
#[path = "tracker_tests.rs"]
mod tests;
