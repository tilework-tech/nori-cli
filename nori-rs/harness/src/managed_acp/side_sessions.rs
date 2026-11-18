//! Route ephemeral side sessions on an established managed ACP connection.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::NativeForks;
use super::StandardAcpConnection;
use crate::managed_launch::LaunchConnection;
use crate::managed_launch::LaunchRefusal;
use crate::managed_launch::ManagedLaunches;
use crate::managed_launch::SideProcess;

#[path = "parent_close.rs"]
mod parent_close;

#[path = "side_worker.rs"]
mod side_worker;

pub(super) use nori_acp_host::remote::SIDE_SESSION_META as SIDE_META;
use nori_acp_host::remote::SIDE_SESSION_PREFIX as SIDE_PREFIX;
pub(super) use nori_acp_host::remote::is_side_session_id as reserved;

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Parent {
    binding_id: String,
    generation: i64,
    session_id: String,
}

pub(super) struct SideLaunch {
    pub connection: LaunchConnection,
    pub owner: Arc<SideProcess>,
}

pub(super) enum SideEvent {
    Frame(Value),
    Ended(Result<(), String>),
}

struct Side {
    native: bool,
    parent: Parent,
    exit_error: Option<String>,
    input: Option<side_worker::SideInput>,
}

struct Pending {
    method: String,
    sid: Option<String>,
    initialize: Option<Value>,
    side: Option<String>,
}

struct AgentRequest {
    side: Option<String>,
    original_id: Value,
}

struct Router {
    forks: NativeForks,
    registry: ManagedLaunches,
    binding: String,
    generation: i64,
    cwd: PathBuf,
    initialize: Option<Value>,
    parents: HashSet<String>,
    sides: HashMap<String, Side>,
    requests: HashMap<Value, Pending>,
    agent_requests: HashMap<Value, AgentRequest>,
    closing: HashMap<String, parent_close::ClosingParent>,
    cleanup: FuturesUnordered<parent_close::Cleanup>,
    events: mpsc::Sender<(String, SideEvent)>,
    output: mpsc::Sender<Value>,
}

fn mark_parent(fields: &mut Value, parent: &Parent) -> bool {
    let Some(fields) = fields.as_object_mut() else {
        return false;
    };
    let meta = fields.entry("_meta").or_insert_with(|| json!({}));
    if !meta.is_object() {
        *meta = json!({});
    }
    meta[SIDE_META] = json!(parent);
    true
}

fn invalid(id: &Value, code: i32, reason: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":reason}})
}

impl Router {
    fn open_side(
        &mut self,
        id: &Value,
        params: &Value,
        parent: &Value,
        method: &str,
    ) -> std::result::Result<String, Value> {
        let parent: Parent = serde_json::from_value(parent.clone()).map_err(|_| {
            invalid(
                id,
                -32602,
                "expected bindingId, generation and sessionId in side parent metadata",
            )
        })?;
        if parent.binding_id != self.binding
            || parent.generation != self.generation
            || !self.parents.contains(&parent.session_id)
            || self.closing.contains_key(&parent.session_id)
            || self.parent_close_pending(&parent.session_id)
        {
            return Err(invalid(
                id,
                -32602,
                "side session parent is not open in this binding and generation",
            ));
        }
        let Some(initialize) = self.initialize.clone() else {
            return Err(invalid(id, -32001, "initialize must complete first"));
        };
        if params["cwd"].as_str().map(Path::new) != Some(self.cwd.as_path()) {
            return Err(invalid(id, -32602, "cwd must match the admitted workspace"));
        }
        if params
            .get("mcpServers")
            .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty))
            || params
                .get("additionalDirectories")
                .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty))
        {
            return Err(invalid(
                id,
                -32602,
                "side sessions accept only the admitted workspace and no client MCP servers",
            ));
        }

        let side_id = format!("{SIDE_PREFIX}{}", Uuid::new_v4());
        let input = if method == "session/fork" {
            if params["sessionId"] != parent.session_id {
                return Err(invalid(
                    id,
                    -32602,
                    "fork sessionId must match the marked parent",
                ));
            }
            side_worker::start_fork(
                self.forks.clone(),
                parent.session_id.clone(),
                params.clone(),
                id.clone(),
                side_id.clone(),
                self.events.clone(),
            )
        } else {
            self.start_rebuilt(id, initialize, &side_id)?
        };
        self.sides.insert(
            side_id.clone(),
            Side {
                native: method == "session/fork",
                parent,
                exit_error: None,
                input: Some(input),
            },
        );
        Ok(side_id)
    }

    fn start_rebuilt(
        &self,
        id: &Value,
        initialize: Value,
        side_id: &str,
    ) -> std::result::Result<side_worker::SideInput, Value> {
        let (connection, owner) = self
            .registry
            .spawn_owned_side(&self.binding, self.generation, side_id)
            .map_err(|err| {
                if matches!(
                    err.downcast_ref::<LaunchRefusal>(),
                    Some(LaunchRefusal::AtCapacity)
                ) {
                    invalid(id, -32017, "machine full")
                } else {
                    invalid(id, -32000, &err.to_string())
                }
            })?;
        Ok(side_worker::start_side(
            SideLaunch { connection, owner },
            initialize,
            id.clone(),
            side_id.to_owned(),
            self.events.clone(),
        ))
    }

    fn side_request(
        &mut self,
        sid: &str,
        method: &str,
        id: Option<&Value>,
        frame: Value,
    ) -> std::result::Result<(), Value> {
        let reason = if self.sides.get(sid).is_none_or(|s| s.input.is_none()) {
            Some((-32002, "side session is closed or unknown"))
        } else if !matches!(
            method,
            "session/prompt"
                | "session/close"
                | "session/cancel"
                | "session/set_mode"
                | "session/set_config_option"
        ) {
            Some((-32601, "operation unavailable on a side session"))
        } else if (method == "session/cancel") == id.is_some() {
            Some((
                -32600,
                "session requests require an id; session/cancel is a notification",
            ))
        } else if method == "session/prompt"
            && self
                .requests
                .values()
                .any(|r| r.side.as_deref() == Some(sid) && r.method == method)
        {
            Some((-32015, "session already has an active prompt"))
        } else {
            None
        };
        if let Some((code, message)) = reason {
            return Err(invalid(id.unwrap_or(&Value::Null), code, message));
        }
        self.send_to_side(sid, frame);
        Ok(())
    }

    fn client_reply(&mut self, mut frame: Value, parent: &mpsc::Sender<Value>) -> Result<()> {
        if frame.get("method").is_some() || frame["jsonrpc"] != "2.0" {
            parent.try_send(frame)?;
        } else if frame.get("result").is_some() != frame.get("error").is_some()
            && let Some(reply) = self.agent_requests.remove(&frame["id"])
        {
            frame["id"] = reply.original_id;
            match reply.side {
                Some(side) => self.send_to_side(&side, frame),
                None => parent.try_send(frame)?,
            }
        }
        Ok(())
    }

    async fn client(&mut self, frame: Value, parent: &mpsc::Sender<Value>) -> Result<()> {
        let id = frame.get("id").cloned();
        let Some(method) = frame["method"].as_str().map(str::to_owned) else {
            return self.client_reply(frame, parent);
        };
        if frame["jsonrpc"] != "2.0" || id.as_ref().is_some_and(|v| !v.is_string() && !v.is_i64()) {
            self.output
                .send(invalid(&Value::Null, -32600, "invalid JSON-RPC message"))
                .await?;
            return Ok(());
        }
        if let Some(id) = &id
            && (self.requests.contains_key(id)
                || self
                    .closing
                    .values()
                    .any(|close| close.request_id.as_ref() == Some(id)))
        {
            self.output
                .send(invalid(
                    id,
                    -32600,
                    "duplicate request ID on logical connection",
                ))
                .await?;
            return Ok(());
        }

        let params = &frame["params"];
        let sid = params["sessionId"].as_str().map(str::to_owned);
        if let Some(sid) = sid.as_deref()
            && self.route_closing_parent(sid, &method, id.as_ref()).await?
        {
            return Ok(());
        }
        let target_side = sid
            .as_ref()
            .filter(|sid| self.sides.contains_key(*sid) || reserved(sid));
        let route = match (params["_meta"].get(SIDE_META), target_side) {
            (Some(mark), None) if matches!(method.as_str(), "session/new" | "session/fork") => {
                match &id {
                    Some(id) => self.open_side(id, params, mark, &method).map(Some),
                    None => Err(invalid(
                        &Value::Null,
                        -32600,
                        "opening a side session requires an id",
                    )),
                }
            }
            (Some(_), _) => Err(invalid(
                id.as_ref().unwrap_or(&Value::Null),
                -32602,
                "side parent metadata requires session/new or session/fork on a regular parent",
            )),
            (None, Some(side)) => self
                .side_request(side, &method, id.as_ref(), frame.clone())
                .map(|()| Some(side.clone())),
            (None, None) => Ok(None),
        };
        let target = match route {
            Ok(side) => side,
            Err(error) => {
                if id.is_some() || method != "session/cancel" {
                    self.output.send(error).await?;
                }
                return Ok(());
            }
        };
        if let Some(id) = id {
            self.requests.insert(
                id,
                Pending {
                    initialize: (method == "initialize").then(|| params.clone()),
                    method,
                    sid,
                    side: target.clone(),
                },
            );
        }
        if target.is_none() {
            parent.try_send(frame)?;
        }
        Ok(())
    }

    fn send_to_side(&mut self, sid: &str, message: Value) {
        if let Some(side) = self.sides.get_mut(sid)
            && let Some(input) = &side.input
            && input.sender.try_send(message).is_err()
        {
            side.input = None;
        }
    }

    fn register_agent_request(&mut self, side: Option<&str>, frame: &mut Value) {
        if let Some(id) = frame.get("id").cloned() {
            let public_id = if side.is_some() || self.agent_requests.contains_key(&id) {
                json!(format!("nori-request-{}", Uuid::new_v4()))
            } else {
                id.clone()
            };
            self.agent_requests.insert(
                public_id.clone(),
                AgentRequest {
                    side: side.map(str::to_owned),
                    original_id: id,
                },
            );
            frame["id"] = public_id;
        }
    }

    fn parent_result(&mut self, request: Pending, result: &Value) {
        match request.method.as_str() {
            "initialize" => self.initialize = request.initialize,
            "session/new" | "session/fork" => {
                if let Some(sid) = result["sessionId"].as_str() {
                    self.parents.insert(sid.to_owned());
                }
            }
            "session/resume" | "session/load" => self.parents.extend(request.sid),
            _ => {}
        }
    }

    async fn agent(&mut self, side: Option<&str>, mut frame: Value) -> Result<()> {
        if side.is_none() {
            let fields = if frame.get("method").is_some() {
                "params"
            } else {
                "result"
            };
            if let Some(meta) = frame
                .get_mut(fields)
                .and_then(|fields| fields.get_mut("_meta"))
                .and_then(Value::as_object_mut)
            {
                meta.remove(SIDE_META);
            }
        }
        if frame["method"].is_string() {
            if let Some(side) = side {
                let Some(record) = self.sides.get_mut(side).filter(|s| s.input.is_some()) else {
                    return Ok(());
                };
                if !mark_parent(&mut frame["params"], &record.parent) {
                    record.input = None;
                    return Ok(());
                }
            }
            self.register_agent_request(side, &mut frame);
        } else if self
            .requests
            .get(&frame["id"])
            .is_some_and(|r| r.side.as_deref() == side)
            && let Some(request) = self.requests.remove(&frame["id"])
        {
            if frame.get("result").is_some() {
                match side {
                    Some(sid)
                        if matches!(request.method.as_str(), "session/new" | "session/fork") =>
                    {
                        if let Some(record) = self.sides.get_mut(sid) {
                            if record.input.is_none() {
                                frame = invalid(
                                    &frame["id"],
                                    -32002,
                                    "side session ended during launch",
                                );
                            } else if !mark_parent(&mut frame["result"], &record.parent) {
                                record.input = None;
                                frame =
                                    invalid(&frame["id"], -32000, "invalid side session result");
                            }
                        }
                    }
                    None => {
                        if matches!(request.method.as_str(), "session/new" | "session/fork")
                            && frame["result"]["sessionId"].as_str().is_some_and(reserved)
                        {
                            self.output
                                .send(invalid(
                                    &frame["id"],
                                    -32000,
                                    "adapter returned a reserved side session ID",
                                ))
                                .await?;
                            return Ok(());
                        }
                        if request.method == "session/list"
                            && let Some(list) = frame
                                .pointer_mut("/result/sessions")
                                .and_then(Value::as_array_mut)
                        {
                            list.retain(|item| !item["sessionId"].as_str().is_some_and(reserved));
                        }
                        if request.method == "session/close"
                            && let Some(parent) = request.sid.as_deref()
                        {
                            self.begin_parent_cleanup(parent, frame);
                            return Ok(());
                        }
                        self.parent_result(request, &frame["result"]);
                    }
                    _ => {}
                }
            }
        } else if side.is_some() {
            return Ok(());
        }
        self.output.send(frame).await?;
        Ok(())
    }

    async fn ended(&mut self, sid: &str, status: Result<(), String>) -> Result<()> {
        let parent = match status {
            Ok(()) => self.sides.remove(sid).map(|side| side.parent.session_id),
            Err(reason) => self.sides.get_mut(sid).map(|side| {
                side.input = None;
                side.exit_error = Some(reason);
                side.parent.session_id.clone()
            }),
        };
        self.agent_requests
            .retain(|_, r| r.side.as_deref() != Some(sid));
        let lost: Vec<_> = self
            .requests
            .extract_if(|_, r| r.side.as_deref() == Some(sid))
            .map(|(id, _)| id)
            .collect();
        for id in lost {
            self.output
                .send(invalid(&id, -32002, "side session ended"))
                .await?;
        }
        if let Some(parent) = parent {
            self.finish_parent_cleanup(&parent).await?;
        }
        Ok(())
    }
}

pub fn route_side_sessions(
    mut parent: StandardAcpConnection,
    forks: NativeForks,
    registry: ManagedLaunches,
    binding: String,
    generation: i64,
    cwd: PathBuf,
) -> StandardAcpConnection {
    let (input, mut commands) = mpsc::channel(64);
    let (output, incoming) = mpsc::channel(64);
    let (events, mut child_events) = mpsc::channel(64);
    let finished = tokio::spawn(async move {
        let mut router = Router {
            forks,
            registry,
            binding,
            generation,
            cwd,
            initialize: None,
            parents: HashSet::new(),
            sides: HashMap::new(),
            requests: HashMap::new(),
            agent_requests: HashMap::new(),
            closing: HashMap::new(),
            cleanup: FuturesUnordered::new(),
            events,
            output,
        };
        let routed = loop {
            let result = tokio::select! {
                message = commands.recv() => match message {
                    Some(message) => router.client(message, &parent.input).await,
                    None => break Ok(()),
                },
                frame = parent.output.recv() => match frame {
                    Some(frame) => router.agent(None, frame).await,
                    None => break Ok(()),
                },
                Some((parent, outcome)) = router.cleanup.next(), if !router.cleanup.is_empty() => {
                    if let Some(close) = router.closing.get_mut(&parent) {
                        close.outcome = Some(outcome);
                    }
                    router.finish_parent_cleanup(&parent).await
                }
                Some((sid, frame)) = child_events.recv() => match frame {
                    SideEvent::Frame(frame) => router.agent(Some(&sid), frame).await,
                    SideEvent::Ended(status) => router.ended(&sid, status).await,
                }
            };
            if result.is_err() {
                break result;
            }
        };
        drop(router);
        drop(parent.input);
        drop(parent.output);
        routed.and(parent.finished.await?)
    });
    StandardAcpConnection {
        input,
        output: incoming,
        finished,
    }
}
