//! Durable control and message correlation for an opted-in managed binding.

pub mod journal;
mod shared;
pub use shared::SharedRecovery;

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use anyhow::ensure;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;

use crate::remote::SIDE_SESSION_META;
use crate::remote::is_side_session_id;
use journal::Admission;
use journal::Journal;
use journal::Snapshot;
use journal::Source;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    pub binding_id: String,
    pub generation: i64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checkpoint {
    pub journal: Snapshot,
    pub main_session_id: Option<String>,
    pub load_session: bool,
}

impl Checkpoint {
    pub fn continuation(&self) -> &'static str {
        if self.load_session && self.main_session_id.is_some() {
            "load"
        } else {
            "fresh"
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StorageFailure {
    Recorder,
    JournalFull,
    JournalUnavailable,
    AdmissionUncertain,
}

impl std::fmt::Display for StorageFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Recorder => "private recorder persistence failure",
            Self::JournalFull => "binding journal full",
            Self::JournalUnavailable => "binding journal persistence failure",
            Self::AdmissionUncertain => {
                "command admission persistence uncertain; reconcile the original command ID"
            }
        })
    }
}

impl std::error::Error for StorageFailure {}

impl StorageFailure {
    pub fn rpc_code(error: &anyhow::Error, fallback: i32) -> i32 {
        match error.downcast_ref::<Self>() {
            Some(Self::AdmissionUncertain) => -32021,
            Some(_) => -32020,
            None => fallback,
        }
    }
}

#[derive(Default)]
struct Active {
    main: Option<String>,
    has_side_namespace: bool,
    load_session: bool,
    pending: HashMap<String, Request>,
    faulted: bool,
    storage_failure: Option<StorageFailure>,
    frozen: bool,
    producer: Option<tokio::sync::watch::Receiver<shared::DrainResult>>,
}

impl Active {
    fn has_unfinished_requests(&self) -> bool {
        self.pending.values().any(|r| !matches!(r, Request::Side))
    }
}

enum Request {
    Initialize,
    Main { load: Option<String> },
    Load(String),
    Prompt { command: String, session: String },
    Side,
    Other,
}

pub struct Recovery {
    pub journal: Journal,
    active: HashMap<Scope, Active>,
}

impl Recovery {
    pub fn open(root: &Path) -> Result<Self> {
        Ok(Self {
            journal: Journal::open(root, 16 * 1024 * 1024)?,
            active: HashMap::new(),
        })
    }

    pub fn attach(&mut self, scope: &Scope) {
        self.active.entry(scope.clone()).or_default();
    }

    pub fn detach(&mut self, scope: &Scope) {
        self.active.remove(scope);
    }

    pub fn freeze(&mut self, scope: &Scope) -> Result<Checkpoint> {
        let checkpoint = self.checkpoint(scope)?;
        self.active
            .get_mut(scope)
            .context("binding is offline")?
            .frozen = true;
        Ok(checkpoint)
    }

    pub fn thaw(&mut self, scope: &Scope) {
        if let Some(state) = self.active.get_mut(scope) {
            state.frozen = false;
        }
    }

    pub fn recorder_barrier(&mut self, scope: &Scope, result: Result<()>) -> Result<()> {
        let state = self.active.get_mut(scope).context("binding is offline")?;
        if let Err(error) = result {
            state
                .storage_failure
                .get_or_insert(StorageFailure::Recorder);
            let detail = format!("{}: {error}", StorageFailure::Recorder);
            return Err(error.context(StorageFailure::Recorder).context(detail));
        }
        if state.storage_failure == Some(StorageFailure::Recorder) {
            state.storage_failure = None;
        }
        Ok(())
    }

    pub fn check_admission_storage(&self, scope: &Scope) -> Result<()> {
        self.journal
            .check_capacity(&scope.binding_id, scope.generation)?;
        if let Some(failure) = self
            .active
            .get(scope)
            .and_then(|state| state.storage_failure)
        {
            return Err(failure.into());
        }
        Ok(())
    }

    pub fn checkpoint(&self, scope: &Scope) -> Result<Checkpoint> {
        let active = self
            .active
            .get(scope)
            .context("binding is offline; use its last committed checkpoint")?;
        if active.faulted
            && let Some(error) = active.storage_failure
        {
            return Err(anyhow::Error::new(error)
                .context("binding lost durable output; use its last committed checkpoint"));
        }
        ensure!(
            !active.faulted
                && active.producer.as_ref().is_none_or(|producer| {
                    producer.has_changed().is_ok()
                        || producer.borrow().as_ref().is_some_and(Result::is_ok)
                }),
            "binding lost durable output; use its last committed checkpoint"
        );
        ensure!(
            !active.has_unfinished_requests(),
            "binding has an unfinished ACP request"
        );
        Ok(Checkpoint {
            journal: self.journal.export(&scope.binding_id, scope.generation)?,
            main_session_id: active.main.clone(),
            load_session: active.load_session,
        })
    }

    /// A reply returned here must go straight to the caller without dispatch.
    pub fn incoming(
        &mut self,
        scope: &Scope,
        message: &Value,
        command: Option<&str>,
    ) -> Result<Option<Value>> {
        let binding = scope.binding_id.as_str();
        let generation = scope.generation;
        let id = message.get("id");
        let method_field = message.get("method");
        ensure!(
            message["jsonrpc"] == "2.0"
                && id.is_none_or(|id| id.is_string() || id.is_i64())
                && method_field.is_none_or(Value::is_string),
            "invalid JSON-RPC message"
        );
        if method_field.is_some() {
            ensure!(
                message.get("result").is_none() && message.get("error").is_none(),
                "invalid JSON-RPC request"
            );
        } else {
            ensure!(
                id.is_some() && message.get("result").is_some() != message.get("error").is_some(),
                "invalid JSON-RPC response"
            );
        }
        let method = message["method"].as_str();
        let session = message["params"]["sessionId"].as_str();
        if let Some(method) = method
            && method.starts_with("session/")
        {
            ensure!(
                (method == "session/cancel") == id.is_none(),
                "session request has an invalid id"
            );
        }
        if let Some(command) = command {
            ensure!(
                method == Some("session/prompt"),
                "commandId is reserved for session/prompt"
            );
            if self.journal.command(binding, generation, command).is_ok() {
                let admission =
                    self.journal
                        .admit(binding, generation, command, &identity(message))?;
                return replay_command(admission, &message["id"]).map(Some);
            }
        }
        if method.is_none() || method == Some("session/cancel") {
            self.journal.check(binding, generation)?;
        } else {
            self.journal.check_open(binding, generation)?;
            self.check_admission_storage(scope)?;
        }
        let state = self
            .active
            .get_mut(scope)
            .context("binding is offline; import a checkpoint in a new generation")?;
        ensure!(!state.faulted, "binding lost durable output");
        ensure!(!state.frozen, "binding is checkpointing");
        let Some(id) = message.get("id").filter(|_| method.is_some()) else {
            return Ok(None);
        };
        let id = id.to_string();
        ensure!(!state.pending.contains_key(&id), "duplicate ACP request id");
        let selecting_main = state.main.is_none()
            && !state
                .pending
                .values()
                .any(|r| matches!(r, Request::Main { .. }));
        let entry = match method {
            Some("initialize") => Request::Initialize,
            Some("session/new" | "session/fork")
                if message["params"]["_meta"].get(SIDE_SESSION_META).is_some() =>
            {
                Request::Side
            }
            Some(method)
                if state.has_side_namespace
                    && method.starts_with("session/")
                    && method != "session/new"
                    && session != state.main.as_deref()
                    && session.is_some_and(is_side_session_id) =>
            {
                Request::Side
            }
            Some("session/new" | "session/load" | "session/resume") if selecting_main => {
                Request::Main {
                    load: session.map(str::to_owned),
                }
            }
            Some("session/load") => {
                Request::Load(session.context("missing load sessionId")?.to_owned())
            }
            Some("session/prompt") if session.is_some() && session == state.main.as_deref() => {
                let command = command.context("main-session prompt requires commandId")?;
                let session = session.context("missing prompt sessionId")?;
                let admission =
                    self.journal
                        .admit(binding, generation, command, &identity(message))?;
                if admission != Admission::Dispatch {
                    return replay_command(admission, &message["id"]).map(Some);
                }
                Request::Prompt {
                    command: command.to_owned(),
                    session: session.to_owned(),
                }
            }
            _ => Request::Other,
        };
        state.pending.insert(id, entry);
        Ok(None)
    }

    /// Call on the binding's producer before delivering to any socket, including while detached.
    pub fn outgoing(&mut self, scope: &Scope, message: &Value) -> Result<Option<Source>> {
        let state = self
            .active
            .get_mut(scope)
            .context("output from retired binding")?;
        let binding = scope.binding_id.as_str();
        let generation = scope.generation;
        self.journal.check(binding, generation)?;
        let stored = if message.get("method").is_none() {
            let request = message
                .get("id")
                .and_then(|id| state.pending.remove(&id.to_string()));
            match request {
                Some(Request::Side) => {
                    if message["result"]["_meta"].get(SIDE_SESSION_META).is_some()
                        && message["result"]["sessionId"]
                            .as_str()
                            .is_some_and(is_side_session_id)
                    {
                        state.has_side_namespace = true;
                    }
                    return Ok(None);
                }
                Some(Request::Initialize) => {
                    state.load_session = message.pointer("/result/agentCapabilities/loadSession")
                        == Some(&Value::Bool(true));
                    return Ok(None);
                }
                Some(Request::Main { load }) if message.get("result").is_some() => {
                    state.main = message
                        .pointer("/result/sessionId")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .or(load);
                    return Ok(None);
                }
                Some(Request::Prompt { command, session }) => self
                    .journal
                    .finish_prompt(binding, generation, &command, &session, message),
                _ => return Ok(None),
            }
        } else {
            let Some(session) = message["params"]["sessionId"].as_str() else {
                return Ok(None);
            };
            if state.main.as_deref() != Some(session)
                || state
                    .pending
                    .values()
                    .any(|r| matches!(r, Request::Load(id) if id == session))
            {
                return Ok(None);
            }
            self.journal.append(binding, generation, session, message)
        };
        if let Err(error) = &stored {
            state.faulted = true;
            if let Some(failure) = error.downcast_ref::<StorageFailure>() {
                state.storage_failure = Some(*failure);
            }
        }
        Ok(Some(stored?.source))
    }
}

fn identity(message: &Value) -> Value {
    json!({"method":message["method"], "params":message["params"]})
}

fn replay_command(status: Admission, id: &Value) -> Result<Value> {
    match status {
        Admission::Completed(mut reply) => {
            ensure!(reply.is_object(), "invalid recorded command response");
            reply["id"] = id.clone();
            Ok(reply)
        }
        Admission::Interrupted => bail!("command interrupted; outcome unknown"),
        Admission::InFlight => bail!("command in flight"),
        Admission::Dispatch => bail!("command was not previously admitted"),
    }
}

#[cfg(test)]
mod tests;
