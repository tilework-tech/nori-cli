//! Couple an interrupt typed before admission to exactly one prompt receipt.

use super::HostCall;
use serde_json::Value;
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone)]
pub(crate) struct PromptInterrupt {
    session: String,
    state: Arc<Mutex<Admission>>,
}

enum Admission {
    Waiting,
    Interrupted,
    Admitted(String),
    Finished,
}

impl PromptInterrupt {
    pub fn new(session: String) -> Self {
        Self {
            session,
            state: Arc::new(Mutex::new(Admission::Waiting)),
        }
    }

    /// Another Enter can arrive while the first prompt is in its admission hook.
    /// The later submission can be refused as busy; it cannot steal the first key.
    pub fn track_first(slot: &mut Option<Self>, pending: &Self) {
        if !slot.as_ref().is_some_and(Self::live) {
            *slot = Some(pending.clone());
        }
    }

    fn lock_state(&self) -> std::sync::MutexGuard<'_, Admission> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn live(&self) -> bool {
        !matches!(*self.lock_state(), Admission::Finished)
    }

    pub fn retire(&self) {
        *self.lock_state() = Admission::Finished;
    }

    pub fn interrupt(&self) -> Option<HostCall> {
        let mut state = self.lock_state();
        match &*state {
            Admission::Admitted(id) => {
                let call = self.cancel(id.clone());
                *state = Admission::Finished;
                Some(call)
            }
            Admission::Waiting => {
                *state = Admission::Interrupted;
                None
            }
            Admission::Interrupted | Admission::Finished => None,
        }
    }

    pub fn admitted(&self, receipt: &Value) -> Option<HostCall> {
        let id = (receipt["session_id"].as_str() == Some(&self.session))
            .then(|| receipt["request_id"].as_str())
            .flatten();
        self.admitted_id(id)
    }

    pub fn admitted_id(&self, id: Option<&str>) -> Option<HostCall> {
        let mut state = self.lock_state();
        let previous = std::mem::replace(&mut *state, Admission::Finished);
        let id = id?.to_owned();
        match previous {
            Admission::Interrupted => Some(self.cancel(id)),
            Admission::Waiting => {
                *state = Admission::Admitted(id);
                None
            }
            Admission::Admitted(_) | Admission::Finished => None,
        }
    }

    fn cancel(&self, observed_turn_id: String) -> HostCall {
        HostCall::CancelObserved {
            session_id: self.session.clone(),
            observed_turn_id,
        }
    }
}
