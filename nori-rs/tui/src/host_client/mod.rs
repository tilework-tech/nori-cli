//! A client of a `nori host`: one connection, request correlation, and the
//! host's notifications, independent of any terminal frontend.
//!
//! The [connection](connection.rs) owns the socket and reconnects; this
//! module names the host methods a frontend calls and matches each reply to
//! the frontend's own tag for that call.

mod connection;
mod file_search;
pub(crate) use file_search::FileSearch;
pub(crate) use file_search::HostFileSearch;
pub(crate) use file_search::file_matches;
mod prompt_interrupt;
pub(crate) use prompt_interrupt::PromptInterrupt;
#[cfg(test)]
mod connection_tests;
mod pipe;
#[cfg(test)]
mod pipe_tests;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

pub(crate) use connection::ConnectionEvent;
pub(crate) use connection::drive;
pub(crate) use pipe::drive_pipe;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;

/// Where a host listens and how to authenticate.
pub(crate) struct HostEndpoint {
    pub endpoint: String,
    pub token: String,
    /// The host's local token, from local discovery only: it shows the host
    /// this terminal runs on its machine, so `!` commands run.
    pub local_token: Option<String>,
    /// Secure local discovery only; explicit remote credentials are never refreshed.
    pub refresh_local_token: Option<Box<dyn Fn() -> anyhow::Result<String> + Send + Sync>>,
}

impl HostEndpoint {
    /// Open an authenticated connection, presenting the local token if this
    /// terminal has one.
    pub(crate) async fn connect(
        &self,
    ) -> anyhow::Result<nori_harness::remote_agent::ManagedClient> {
        use nori_harness::remote_agent::ManagedClient;
        match &self.local_token {
            Some(local) => ManagedClient::connect_local(&self.endpoint, &self.token, local).await,
            None => ManagedClient::connect(&self.endpoint, &self.token).await,
        }
    }
}

/// The `_meta` key that scopes `session/list` and `session/new` to a
/// prepared agent server.
pub(crate) const PREPARED_AGENT: &str = "nori/preparedAgent";

/// One host call, named by what it does rather than by its wire method.
#[derive(Debug)]
pub(crate) enum HostCall {
    Initialize,
    List,
    Subscribe {
        session_id: String,
        cursor: i64,
    },
    TakeControl,
    /// Parameters come from the `/new` parser, which knows the host's folder support.
    NewSession(Value),
    /// Parameters come from the conversation, which carries the prompt's ACP shape.
    Prompt(Value),
    FollowUp {
        session_id: String,
        expected_turn: nori_protocol::acp::v1::RequestId,
        message_id: String,
        intent: nori_harness::FollowUpIntent,
        prompt: Vec<nori_protocol::acp::v1::ContentBlock>,
    },
    CancelObserved {
        session_id: String,
        observed_turn_id: String,
    },
    Close {
        session_id: String,
    },
    /// A host too old to report the session's folder gets its own resume method.
    Resume {
        session_id: String,
        cwd: Option<std::path::PathBuf>,
    },
    Respond {
        session_id: String,
        request_id: Value,
        response: nori_protocol::acp::v1::RequestPermissionResponse,
    },
    Delegate {
        session_id: String,
        agent: String,
        task: String,
    },
    ResolveDelegation {
        session_id: String,
        request_id: Value,
        approved: bool,
    },
    /// Start an ACP agent server on the host for this connection's picker.
    PrepareAgent {
        agent: String,
        cwd: std::path::PathBuf,
    },
    /// The prepared server's own saved sessions, not host conversations.
    ListPrepared {
        selection: String,
        cwd: std::path::PathBuf,
    },
    /// Start a host conversation on the prepared server: a new session, or
    /// one of its saved sessions when `saved_session_id` names it.
    ActivatePrepared {
        selection: String,
        cwd: std::path::PathBuf,
        saved_session_id: Option<String>,
    },
    DiscardPrepared {
        selection: String,
    },
    /// Change one session's permission mode. Only a controller may.
    SetMode {
        session_id: String,
        mode: nori_protocol::ApprovalMode,
    },
    /// Files or folders on the host, in the session's folder or, without
    /// one, the host's own.
    FindPaths {
        session_id: Option<String>,
        query: String,
        kind: nori_protocol::PathKind,
        limit: i32,
    },
    /// Let the session's paused queue send again. Only a controller may.
    ResumeQueue {
        session_id: String,
    },
    /// Take one queued prompt back before it is sent. Only a controller may.
    TakeQueued {
        session_id: String,
        entry_id: String,
    },
    /// Keep a sent prompt in the host's history. Only a controller may.
    AddHistory {
        session_id: String,
        text: String,
    },
    /// One prompt of the host's history, counted from the oldest.
    HistoryEntry {
        log_id: i64,
        offset: i64,
    },
    /// The host's most recent distinct prompts, newest first.
    SearchHistory {
        max_results: i32,
    },
    /// How many prompts the host's history holds now.
    HistoryMetadata,
    /// The custom prompts kept in the host's home.
    ListPrompts,
    /// Run a script prompt on the host, in the session's folder. Only a
    /// controller may.
    RunPrompt {
        session_id: String,
        name: String,
        args: Vec<String>,
    },
    /// The session's goal now. Watchers may read it.
    GetGoal {
        session_id: String,
    },
    /// Set, pause, resume or clear the session's goal. Only a controller may.
    SetGoal {
        session_id: String,
        op: nori_protocol::GoalOp,
    },
    /// The session's context report. Watchers may read it.
    SessionContext {
        session_id: String,
        skip_instructions: bool,
    },
    /// Compact the session. Only a controller may.
    Compact {
        session_id: String,
    },
    /// Run a `!` command on the host in the session's folder. Only a
    /// controller may.
    Shell {
        session_id: String,
        command: String,
    },
}

impl HostCall {
    fn into_wire(self) -> (&'static str, Value) {
        match self {
            // Hosts send `approval_mode_changed` only to clients that declare it.
            Self::Initialize => (
                "initialize",
                json!({"capabilities":{"nori_sessions":{"approval_modes":true}}}),
            ),
            Self::List => ("_nori/session/list", json!({})),
            Self::Subscribe { session_id, cursor } => (
                "_nori/session/subscribe",
                json!({"session_id": session_id, "cursors": {session_id: cursor}}),
            ),
            Self::TakeControl => ("_nori/control/take", json!({})),
            Self::NewSession(params) => ("_nori/session/new", params),
            Self::Prompt(params) => ("_nori/session/prompt", params),
            Self::FollowUp {
                session_id,
                expected_turn,
                message_id,
                intent,
                prompt,
            } => (
                "_nori/session/follow_up",
                json!({"session_id": session_id, "expected_turn": expected_turn,
                    "message_id": message_id, "intent": intent,
                    "prompt": prompt}),
            ),
            Self::CancelObserved {
                session_id,
                observed_turn_id,
            } => (
                "_nori/session/cancel_observed",
                json!({"sessionId": session_id, "observedTurnId": observed_turn_id}),
            ),
            Self::Close { session_id } => ("session/close", json!({"sessionId": session_id})),
            Self::Resume {
                session_id,
                cwd: Some(cwd),
            } => (
                "session/resume",
                json!({"sessionId": session_id, "cwd": cwd, "mcpServers": []}),
            ),
            Self::Resume {
                session_id,
                cwd: None,
            } => ("_nori/session/resume", json!({"session_id": session_id})),
            Self::Respond {
                session_id,
                request_id,
                response,
            } => (
                "_nori/session/respond",
                json!({"session_id": session_id, "request_id": request_id, "response": response}),
            ),
            Self::Delegate {
                session_id,
                agent,
                task,
            } => (
                "_nori/session/delegate",
                json!({"session_id": session_id, "agent": agent, "task": task}),
            ),
            Self::ResolveDelegation {
                session_id,
                request_id,
                approved,
            } => (
                "_nori/session/delegation/resolve",
                json!({"session_id": session_id, "request_id": request_id, "approved": approved}),
            ),
            Self::PrepareAgent { agent, cwd } => (
                "_nori/host/prepare_agent",
                json!({"agent": agent, "cwd": cwd}),
            ),
            Self::ListPrepared { selection, cwd } => (
                "session/list",
                json!({"cwd": cwd, "_meta": {PREPARED_AGENT: {"id": selection}}}),
            ),
            Self::ActivatePrepared {
                selection,
                cwd,
                saved_session_id,
            } => {
                let mut prepared = json!({"id": selection});
                if let Some(saved) = saved_session_id {
                    prepared["savedSessionId"] = json!(saved);
                }
                (
                    "session/new",
                    json!({"cwd": cwd, "mcpServers": [], "_meta": {PREPARED_AGENT: prepared}}),
                )
            }
            Self::DiscardPrepared { selection } => {
                ("_nori/host/discard_agent", json!({"id": selection}))
            }
            Self::SetMode { session_id, mode } => (
                "session/set_mode",
                json!({"sessionId": session_id, "modeId": mode}),
            ),
            Self::FindPaths {
                session_id,
                query,
                kind,
                limit,
            } => (
                "_nori/session/find_paths",
                json!(nori_protocol::FindPathsParams {
                    session_id,
                    query,
                    kind,
                    limit: Some(limit),
                }),
            ),
            Self::ResumeQueue { session_id } => (
                "_nori/session/queue/resume",
                json!(nori_protocol::QueueParams { session_id }),
            ),
            Self::TakeQueued {
                session_id,
                entry_id,
            } => (
                "_nori/session/queue/take",
                json!(nori_protocol::QueueTakeParams {
                    session_id,
                    entry_id
                }),
            ),
            Self::AddHistory { session_id, text } => (
                "_nori/host/history/add",
                json!(nori_protocol::HistoryAddParams { session_id, text }),
            ),
            Self::HistoryEntry { log_id, offset } => (
                "_nori/host/history/entry",
                json!(nori_protocol::HistoryEntryParams { log_id, offset }),
            ),
            Self::SearchHistory { max_results } => (
                "_nori/host/history/search",
                json!(nori_protocol::HistorySearchParams { max_results }),
            ),
            Self::HistoryMetadata => ("_nori/host/history/metadata", json!({})),
            Self::ListPrompts => (
                "_nori/host/prompts",
                json!(nori_protocol::PromptsListParams::default()),
            ),
            Self::RunPrompt {
                session_id,
                name,
                args,
            } => (
                "_nori/session/prompts/run",
                json!(nori_protocol::PromptRunParams {
                    session_id,
                    name,
                    args
                }),
            ),
            Self::GetGoal { session_id } => (
                "_nori/session/goal/get",
                json!(nori_protocol::GoalGetParams { session_id }),
            ),
            Self::SetGoal { session_id, op } => (
                "_nori/session/goal/set",
                json!(nori_protocol::GoalSetParams { session_id, op }),
            ),
            Self::SessionContext {
                session_id,
                skip_instructions,
            } => (
                "_nori/session/context",
                json!(nori_protocol::ContextParams {
                    session_id,
                    skip_instructions,
                }),
            ),
            Self::Compact { session_id } => (
                "_nori/session/compact",
                json!(nori_protocol::CompactParams { session_id }),
            ),
            Self::Shell {
                session_id,
                command,
            } => (
                "_nori/session/shell",
                json!(nori_protocol::ShellParams {
                    session_id,
                    command
                }),
            ),
        }
    }
}

/// A host error reply. The message is absent when the host sent none.
#[derive(Debug)]
pub(crate) struct HostError {
    pub code: Option<i64>,
    pub message: Option<String>,
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message.as_deref().unwrap_or("Host request failed."))
    }
}

impl std::error::Error for HostError {}

/// What the connection delivered, with replies matched to their tags.
pub(crate) enum HostEvent<T> {
    /// A connection is up and must be initialized before anything else.
    Connected,
    /// The connection dropped. The host may have applied any unanswered call;
    /// none of them is retried.
    Disconnected {
        reason: String,
        unanswered: Vec<T>,
    },
    /// Recovery is impossible; the client sends nothing more.
    Stopped(String),
    /// `_nori/session/event` parameters: one session envelope.
    Session(Value),
    RoleChanged(String),
    Reply {
        tag: T,
        result: Result<Value, HostError>,
    },
    /// Requests from the host and replies to nothing pending.
    Ignored,
}

pub(crate) struct HostSessionClient<T> {
    requests: mpsc::UnboundedSender<Value>,
    pending: HashMap<i64, T>,
    next_id: i64,
    stopped: bool,
}

impl<T> HostSessionClient<T> {
    pub fn new(requests: mpsc::UnboundedSender<Value>) -> Self {
        Self {
            requests,
            pending: HashMap::new(),
            next_id: 0,
            stopped: false,
        }
    }

    pub fn request(&mut self, call: HostCall, tag: T) -> Result<(), T> {
        if self.stopped {
            return Err(tag);
        }
        self.next_id += 1;
        let id = self.next_id;
        let (method, params) = call.into_wire();
        if self
            .requests
            .send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .is_err()
        {
            return Err(tag);
        }
        self.pending.insert(id, tag);
        Ok(())
    }

    pub fn has_pending(&self, matches: impl Fn(&T) -> bool) -> bool {
        self.pending.values().any(matches)
    }

    /// Forget every unanswered call and send nothing more.
    pub fn stop(&mut self) {
        self.stopped = true;
        self.pending.clear();
    }

    pub fn incoming(&mut self, event: ConnectionEvent) -> HostEvent<T> {
        match event {
            ConnectionEvent::Connected => HostEvent::Connected,
            ConnectionEvent::Disconnected(reason) => HostEvent::Disconnected {
                reason,
                unanswered: self.pending.drain().map(|(_, tag)| tag).collect(),
            },
            ConnectionEvent::Stopped(reason) => {
                self.stop();
                HostEvent::Stopped(reason)
            }
            ConnectionEvent::Message(mut message) => match message.get("method").map(Value::as_str)
            {
                Some(Some("_nori/session/event")) => HostEvent::Session(message["params"].take()),
                Some(Some("_nori/control/changed")) => HostEvent::RoleChanged(
                    message["params"]["role"]
                        .as_str()
                        .unwrap_or("watcher")
                        .to_owned(),
                ),
                // Requests from the host share the id space with replies;
                // only replies settle a pending call.
                Some(_) => HostEvent::Ignored,
                None => {
                    let Some(tag) = message["id"]
                        .as_i64()
                        .and_then(|id| self.pending.remove(&id))
                    else {
                        return HostEvent::Ignored;
                    };
                    let result = match message.get("error") {
                        Some(error) => Err(HostError {
                            code: error["code"].as_i64(),
                            message: error["message"].as_str().map(str::to_owned),
                        }),
                        None => Ok(message["result"].take()),
                    };
                    HostEvent::Reply { tag, result }
                }
            },
        }
    }
}
