//! Duplex relay for one admitted binding and generation.

mod native;

pub use native::NativeForks;

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use tokio::io::AsyncRead;
use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_util::codec::FramedRead;
use tokio_util::codec::LinesCodec;

use crate::backend::managed_additions::ManagedMcp;
use crate::backend::managed_additions::ManagedSessionAdditions;

pub use nori_acp_host::remote::StandardAcpConnection;
mod side_sessions;
pub use side_sessions::route_side_sessions;

fn normalize_close(frame: &mut Value) {
    match (frame.get("result"), frame.get("error")) {
        (Some(result), None) if result.is_object() => {}
        (None, Some(error)) if error["code"] == -32002 => {
            *frame = json!({"jsonrpc":"2.0","id":frame["id"],"result":{}});
        }
        (None, Some(error)) if error["code"].is_i64() && error["message"].is_string() => {}
        _ => {
            *frame = json!({"jsonrpc":"2.0","id":frame["id"],"error":{
                "code":-32603,"message":"adapter did not confirm session close"
            }});
        }
    }
}

struct Request {
    method: String,
    sid: Option<String>,
    mcp: Option<ManagedMcp>,
}

struct Relay {
    cwd: PathBuf,
    native: native::Native,
    initialize: Option<Value>,
    requests: HashMap<Value, Request>,
    agent_requests: HashSet<Value>,
    additions: ManagedSessionAdditions,
    echo: bool,
    status: bool,
    output: mpsc::Sender<Value>,
}

impl Relay {
    fn check_request(
        &self,
        method: &str,
        params: &Value,
        id: Option<&Value>,
    ) -> Option<(i32, &'static str)> {
        if !params.is_null() && !params.is_object()
            || params
                .get("_meta")
                .is_some_and(|v| !v.is_null() && !v.is_object())
        {
            return Some((-32602, "expected object parameters and metadata"));
        }
        if method.starts_with("session/") && (method == "session/cancel") == id.is_some() {
            return Some((
                -32600,
                "session requests require an id; session/cancel is a notification",
            ));
        }
        if params["sessionId"]
            .as_str()
            .is_some_and(|sid| self.native.inaccessible(sid))
        {
            return Some((-32602, "unknown session"));
        }
        if id.is_some_and(|id| self.requests.contains_key(id) || self.native.reserved(id)) {
            return Some((-32600, "duplicate request ID on logical connection"));
        }
        if params["_meta"].get(side_sessions::SIDE_META).is_some() {
            return Some((-32601, "side sessions are unavailable on this connection"));
        }
        if matches!(method, "session/load" | "session/resume" | "session/fork")
            && params["sessionId"]
                .as_str()
                .is_some_and(side_sessions::reserved)
        {
            return Some((-32002, "side session cannot be restored or forked"));
        }
        if method == "initialize" {
            return (id.is_none() || self.initialize.is_some() || !self.requests.is_empty())
                .then_some((-32600, "expected a single initialize request"));
        }
        let Some(initialize) = &self.initialize else {
            return Some((-32001, "initialize must complete first"));
        };
        if method.starts_with("_nori/") {
            return Some((-32601, "host control is unavailable on a scoped connection"));
        }
        if method == "session/fork"
            && !initialize
                .pointer("/agentCapabilities/sessionCapabilities/fork")
                .is_some_and(Value::is_object)
        {
            return Some((-32601, "agent does not advertise session/fork"));
        }
        if matches!(
            method,
            "session/new" | "session/fork" | "session/load" | "session/resume"
        ) && params["cwd"].as_str().map(Path::new) != Some(self.cwd.as_path())
        {
            return Some((-32602, "cwd must match the admitted workspace"));
        }
        if let Some(paths) = params.get("additionalDirectories") {
            let authorized = paths.as_array().is_some_and(|paths| {
                paths.iter().all(|value| {
                    value
                        .as_str()
                        .map(Path::new)
                        .filter(|p| p.is_absolute())
                        .and_then(|p| p.canonicalize().ok())
                        .is_some_and(|p| p.starts_with(&self.cwd) && p.is_dir())
                })
            });
            if !authorized {
                return Some((
                    -32602,
                    "additional directories must be inside the admitted workspace",
                ));
            }
        }
        if method.starts_with("session/") && method != "session/new" && method != "session/list" {
            let Some(sid) = params["sessionId"].as_str().filter(|s| !s.is_empty()) else {
                return Some((-32602, "sessionId is required"));
            };
            if method == "session/prompt"
                && self
                    .requests
                    .values()
                    .any(|req| req.method == method && req.sid.as_deref() == Some(sid))
            {
                return Some((-32015, "session already has an active prompt"));
            }
        }
        None
    }

    async fn client(&mut self, mut message: Value, writer: &mpsc::Sender<Value>) -> Result<()> {
        let id = message.get("id").cloned();
        if message["jsonrpc"] != "2.0"
            || id
                .as_ref()
                .is_some_and(|id| !id.is_string() && !id.is_i64())
            || message
                .get("method")
                .is_some_and(|method| !method.is_string())
        {
            self.output.send(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"invalid JSON-RPC message"}})).await?;
            return Ok(());
        }
        let Some(method) = message["method"].as_str().map(str::to_owned) else {
            if message.get("result").is_some() != message.get("error").is_some()
                && id
                    .as_ref()
                    .is_some_and(|id| self.agent_requests.remove(id) && !self.native.ambiguous(id))
            {
                self.native.write(writer, message)?;
            }
            return Ok(());
        };
        if let Some((code, reason)) = self.check_request(&method, &message["params"], id.as_ref()) {
            if let Some(id) = id {
                self.output
                    .send(json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":reason}}))
                    .await?;
            }
            return Ok(());
        }
        let mut mcp = None;
        let sid = message["params"]["sessionId"].as_str().map(str::to_owned);
        if matches!(
            method.as_str(),
            "session/new" | "session/load" | "session/resume" | "session/fork"
        ) && let Some(init) = &self.initialize
        {
            mcp = self
                .additions
                .prepare_mcp(init, &mut message["params"])
                .await?;
        }
        if method == "session/prompt"
            && let Some(sid) = &sid
        {
            let params = &mut message["params"];
            if self.echo {
                let key = nori_protocol::PROMPT_ECHO_ID_META_KEY;
                let echo_id = params["_meta"][key]
                    .as_str()
                    .map_or_else(|| uuid::Uuid::new_v4().to_string(), str::to_owned);
                params["_meta"][key] = json!(echo_id);
                if let Some(blocks) = params["prompt"].as_array() {
                    for block in blocks {
                        self.notify(sid, json!({"sessionUpdate":"user_message_chunk","content":block,"messageId":echo_id,"_meta":{(key):echo_id}})).await?;
                    }
                }
            }
            self.additions.prompt(sid, params).await?;
            if self.status {
                self.turn_status(sid, "working").await?;
            }
        }
        if let Some(id) = id {
            self.requests.insert(id, Request { method, sid, mcp });
        }
        self.native.write(writer, message)?;
        Ok(())
    }

    async fn agent(&mut self, mut message: Value, writer: &mpsc::Sender<Value>) -> Result<()> {
        ensure!(message["jsonrpc"] == "2.0", "invalid adapter JSON-RPC");
        if self
            .native
            .agent(&mut message, &self.requests, &self.agent_requests, writer)?
        {
            return Ok(());
        }
        if message["method"].is_string() {
            if let Some(id) = message.get("id") {
                ensure!(
                    self.agent_requests.insert(id.clone()),
                    "duplicate adapter request ID"
                );
            }
            if message["method"] == "session/update"
                && let Some(sid) = message["params"]["sessionId"].as_str()
            {
                if self.requests.values().any(|request| {
                    request.method == "session/load" && request.sid.as_deref() == Some(sid)
                }) {
                    self.additions.opened(sid, None).await?;
                }
                self.additions.update(sid, &message["params"]).await?;
            }
        } else {
            let request = self
                .requests
                .remove(&message["id"])
                .context("unsolicited adapter response")?;
            let sid = request.sid.as_deref();
            if request.method == "session/close"
                && sid.is_some_and(|sid| self.native.knows_parent(sid))
            {
                normalize_close(&mut message);
            }
            if let Some(result) = message.get("result") {
                match request.method.as_str() {
                    "initialize" => self.initialize = Some(result.clone()),
                    "session/new" | "session/fork" | "session/load" | "session/resume" => {
                        let resolved =
                            if matches!(request.method.as_str(), "session/new" | "session/fork") {
                                result["sessionId"].as_str()
                            } else {
                                sid
                            }
                            .context("missing agent session ID")?;
                        if self.native.private(resolved) {
                            self.output.send(json!({"jsonrpc":"2.0","id":message["id"],"error":{"code":-32603,"message":"adapter returned an existing session ID"}})).await?;
                            return Ok(());
                        }
                        self.native.observed(resolved);
                        self.additions.opened(resolved, request.mcp).await?;
                    }
                    "session/close" => {
                        if let Some(sid) = sid {
                            self.native.parent_closed(sid, writer)?;
                            self.additions.closed(sid).await?;
                        }
                    }
                    "session/list" => {
                        if let Some(sessions) = message
                            .get_mut("result")
                            .and_then(|r| r.get_mut("sessions"))
                            .and_then(Value::as_array_mut)
                        {
                            sessions.retain(|session| {
                                session["sessionId"]
                                    .as_str()
                                    .is_none_or(|sid| self.native.list_visible(sid))
                            });
                        }
                    }
                    _ => {}
                }
            }
            if self.status
                && request.method == "session/prompt"
                && let Some(sid) = sid
            {
                self.turn_status(sid, "idle").await?;
            }
        }
        self.output.send(message).await?;
        Ok(())
    }

    async fn notify(&self, sid: &str, update: Value) -> Result<()> {
        self.output.send(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":sid,"update":update}})).await?;
        Ok(())
    }

    async fn turn_status(&self, sid: &str, status: &str) -> Result<()> {
        self.notify(
            sid,
            json!({"sessionUpdate":"session_info_update","_meta":{"nori":{"status":status}}}),
        )
        .await
    }
}

pub fn connect(
    reader: impl AsyncRead + Unpin + Send + 'static,
    writer: impl AsyncWrite + Unpin + Send + 'static,
    cwd: PathBuf,
    additions: ManagedSessionAdditions,
    echo: bool,
    status: bool,
) -> StandardAcpConnection {
    let (_, commands) = NativeForks::channel();
    start(
        reader,
        writer,
        cwd,
        additions,
        (echo, status),
        native::Native::default(),
        commands,
    )
}

pub fn connect_with_forks(
    reader: impl AsyncRead + Unpin + Send + 'static,
    writer: impl AsyncWrite + Unpin + Send + 'static,
    cwd: PathBuf,
    additions: ManagedSessionAdditions,
    echo: bool,
    status: bool,
    agent: String,
) -> (StandardAcpConnection, NativeForks) {
    let (forks, commands) = NativeForks::channel();
    let connection = start(
        reader,
        writer,
        cwd,
        additions,
        (echo, status),
        native::Native::enabled(agent),
        commands,
    );
    (connection, forks)
}

fn start(
    reader: impl AsyncRead + Unpin + Send + 'static,
    mut writer: impl AsyncWrite + Unpin + Send + 'static,
    cwd: PathBuf,
    additions: ManagedSessionAdditions,
    (echo, status): (bool, bool),
    native: native::Native,
    mut commands: mpsc::Receiver<native::Command>,
) -> StandardAcpConnection {
    let (input, mut client) = mpsc::channel(64);
    let (output, incoming) = mpsc::channel(64);
    let finished = tokio::spawn(async move {
        let (to_agent, mut writes) = mpsc::channel::<Value>(64);
        let write = async move {
            while let Some(message) = writes.recv().await {
                let mut bytes = serde_json::to_vec(&message)?;
                bytes.push(b'\n');
                writer.write_all(&bytes).await?;
                writer.flush().await?;
            }
            Ok(())
        };
        let relay = async {
            let mut state = Relay {
                cwd,
                native,
                additions,
                echo,
                status,
                output,
                initialize: None,
                requests: HashMap::new(),
                agent_requests: HashSet::new(),
            };
            let mut lines =
                FramedRead::new(reader, LinesCodec::new_with_max_length(16 * 1024 * 1024));
            loop {
                tokio::select! {
                    permit = to_agent.reserve(), if state.native.deferred() => {
                        state.native.flush(permit?);
                    },
                    message = client.recv(), if !state.native.deferred() => match message {
                        Some(message) => state.client(message, &to_agent).await?,
                        None => break,
                    },
                    Some(cmd) = commands.recv(), if !state.native.deferred() => {
                        state.native.command(cmd, state.initialize.as_ref().unwrap_or(&Value::Null), &state.cwd, &state.requests, &to_agent)?;
                    },
                    line = lines.next() => match line {
                        Some(line) => state.agent(serde_json::from_str(&line?)?, &to_agent).await?,
                        None => break,
                    }
                }
            }
            Ok(())
        };
        tokio::select! { result = write => result, result = relay => result }
    });
    StandardAcpConnection {
        input,
        output: incoming,
        finished,
    }
}

#[cfg(test)]
mod native_tests;
