//! Private ACP endpoints sharing the admitted adapter's stdio stream.
#[path = "native_adapter.rs"]
mod adapter;

use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::Request;
use super::StandardAcpConnection;
use super::side_sessions::SIDE_META;

#[derive(Clone)]
pub struct NativeForks {
    commands: mpsc::Sender<Command>,
}

fn error_response(id: &Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

impl NativeForks {
    pub(super) fn channel() -> (Self, mpsc::Receiver<Command>) {
        let (commands, receiver) = mpsc::channel(64);
        (Self { commands }, receiver)
    }

    /// Open a private endpoint after the adapter has acknowledged its read-only mode.
    pub async fn open(
        &self,
        parent: &str,
        params: Value,
    ) -> Result<(StandardAcpConnection, Value)> {
        let (output, incoming) = mpsc::channel(64);
        let (answer, ready) = oneshot::channel();
        let (input, mut outgoing) = mpsc::channel(64);
        let key = Uuid::new_v4();
        self.commands
            .send(Command::Open {
                key,
                parent: parent.to_owned(),
                params,
                output,
                answer,
            })
            .await
            .map_err(|_| anyhow!("parent adapter connection ended"))?;
        let commands = self.commands.clone();
        tokio::spawn(async move {
            while let Some(message) = outgoing.recv().await {
                if commands.send(Command::Frame(key, message)).await.is_err() {
                    return;
                }
            }
            let _ = commands.send(Command::Dropped(key)).await;
        });
        let result = tokio::time::timeout(Duration::from_secs(10), ready)
            .await
            .context("adapter timed out opening a read-only fork")?
            .context("parent adapter connection ended")??;
        Ok((
            StandardAcpConnection {
                input,
                output: incoming,
            },
            result,
        ))
    }
}

pub(super) enum Command {
    Open {
        key: Uuid,
        parent: String,
        params: Value,
        output: mpsc::Sender<Value>,
        answer: oneshot::Sender<Result<Value>>,
    },
    Frame(Uuid, Value),
    Dropped(Uuid),
}

struct Opening {
    key: Uuid,
    parent: String,
    mode: String,
    output: mpsc::Sender<Value>,
    answer: oneshot::Sender<Result<Value>>,
}

struct Child {
    parent: String,
    sid: String,
    mode: String,
    output: mpsc::Sender<Value>,
    permissions: HashSet<Value>,
    closing: bool,
    ready: bool,
}

enum Pending {
    Fork(Opening),
    LateFork {
        parent: String,
    },
    Mode {
        key: Uuid,
        result: Value,
        answer: oneshot::Sender<Result<Value>>,
    },
    Forward {
        key: Uuid,
        client_id: Value,
        method: String,
    },
    Discard,
}

#[derive(Default)]
pub(super) struct Native {
    children: HashMap<Uuid, Child>,
    private_ids: HashMap<String, Uuid>,
    parents: HashSet<String>,
    pending: HashMap<Value, Pending>,
    agent: Option<String>,
    ambiguous_permissions: HashSet<Value>,
    deferred: VecDeque<Value>,
}

impl Native {
    pub(super) fn enabled(agent: String) -> Self {
        Self {
            agent: Some(agent),
            ..Self::default()
        }
    }

    pub(super) fn private(&self, sid: &str) -> bool {
        self.private_ids.contains_key(sid)
    }

    pub(super) fn deferred(&self) -> bool {
        !self.deferred.is_empty()
    }

    pub(super) fn flush(&mut self, permit: mpsc::Permit<'_, Value>) {
        if let Some(frame) = self.deferred.pop_front() {
            permit.send(frame);
        }
    }

    pub(super) fn write(&mut self, writer: &mpsc::Sender<Value>, frame: Value) -> Result<()> {
        if self.agent.is_none() {
            writer.try_send(frame)?;
        } else if self.deferred() {
            self.deferred.push_back(frame);
        } else if let Err(error) = writer.try_send(frame) {
            match error {
                mpsc::error::TrySendError::Full(frame) => self.deferred.push_back(frame),
                mpsc::error::TrySendError::Closed(_) => {
                    anyhow::bail!("adapter stopped accepting input")
                }
            }
        }
        Ok(())
    }

    pub(super) fn ambiguous(&self, id: &Value) -> bool {
        self.ambiguous_permissions.contains(id)
    }

    pub(super) fn list_visible(&self, sid: &str) -> bool {
        !self.private(sid)
            && (self.parents.contains(sid)
                || !self
                    .pending
                    .values()
                    .any(|p| matches!(p, Pending::Fork(_) | Pending::LateFork { .. })))
    }

    pub(super) fn inaccessible(&self, sid: &str) -> bool {
        self.private(sid)
            || (!self.parents.contains(sid)
                && self
                    .pending
                    .values()
                    .any(|p| matches!(p, Pending::Fork(open) if !open.answer.is_closed())))
    }

    pub(super) fn reserved(&self, id: &Value) -> bool {
        self.pending.contains_key(id)
    }

    pub(super) fn observed(&mut self, sid: &str) {
        if self.agent.is_some() {
            self.parents.insert(sid.to_owned());
        }
    }

    pub(super) fn parent_closed(
        &mut self,
        parent: &str,
        writer: &mpsc::Sender<Value>,
    ) -> Result<()> {
        self.parents.remove(parent);
        for pending in self.pending.values_mut() {
            if let Pending::Fork(open) = pending
                && open.parent == parent
            {
                *pending = Pending::LateFork {
                    parent: open.parent.clone(),
                };
            }
        }
        let keys: Vec<_> = self
            .children
            .iter()
            .filter(|(_, c)| c.parent == parent)
            .map(|(key, _)| *key)
            .collect();
        for key in keys {
            self.retire(key, writer)?;
        }
        Ok(())
    }

    fn mode<'a>(&self, init: &'a Value) -> Option<&'a str> {
        let caps = &init["agentCapabilities"];
        let promise = &caps["_meta"]["nori.dev/readOnlyFork"];
        let agent = self.agent.as_deref()?;
        if agent.eq_ignore_ascii_case("codex")
            || agent.eq_ignore_ascii_case("codex-acp")
            || !caps["sessionCapabilities"]["fork"].is_object()
            || !caps["sessionCapabilities"]["close"].is_object()
            || promise["version"].as_i64() != Some(1)
        {
            return None;
        }
        promise["modeId"].as_str().filter(|id| {
            let mode = id.trim();
            !mode.is_empty()
                && !["bypassPermissions", "bypass", "danger-full-access"]
                    .iter()
                    .any(|unsafe_mode| mode.eq_ignore_ascii_case(unsafe_mode))
        })
    }

    fn request(
        &mut self,
        writer: &mpsc::Sender<Value>,
        method: &str,
        params: Value,
        pending: Pending,
    ) -> Result<()> {
        let id = json!(Uuid::new_v4().to_string());
        self.write(
            writer,
            json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )?;
        self.pending.insert(id, pending);
        Ok(())
    }

    pub(super) fn command(
        &mut self,
        cmd: Command,
        init: &Value,
        cwd: &Path,
        requests: &HashMap<Value, Request>,
        writer: &mpsc::Sender<Value>,
    ) -> Result<()> {
        match cmd {
            Command::Open {
                key,
                parent,
                params,
                output,
                answer,
            } => {
                let Some(mode) = self.mode(init).map(str::to_owned) else {
                    let _ = answer.send(Err(anyhow!(
                        "adapter does not guarantee independent read-only forks during a turn"
                    )));
                    return Ok(());
                };
                if requests.values().any(|r| {
                    r.method == "session/close" && r.sid.as_deref() == Some(parent.as_str())
                }) || !self.parents.contains(&parent)
                    || params["sessionId"] != parent
                    || params["cwd"].as_str().map(Path::new) != Some(cwd)
                    || params
                        .get("mcpServers")
                        .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty))
                    || params
                        .get("additionalDirectories")
                        .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty))
                {
                    let _ = answer.send(Err(anyhow!(
                        "fork requires a live parent and its admitted workspace"
                    )));
                    return Ok(());
                }
                let mut fork_params = json!({"sessionId":parent, "cwd":cwd, "mcpServers":[]});
                if let Some(marker) = params["_meta"].get(SIDE_META) {
                    fork_params["_meta"] = json!({(SIDE_META): marker});
                }
                let open = Opening {
                    key,
                    parent,
                    mode,
                    output,
                    answer,
                };
                self.request(writer, "session/fork", fork_params, Pending::Fork(open))?;
            }
            Command::Frame(key, message) => self.client(key, message, writer)?,
            Command::Dropped(key) => {
                for pending in self.pending.values_mut() {
                    if let Pending::Fork(open) = pending
                        && open.key == key
                    {
                        *pending = Pending::LateFork {
                            parent: open.parent.clone(),
                        };
                    }
                }
                self.retire(key, writer)?;
            }
        }
        Ok(())
    }

    fn client(&mut self, key: Uuid, mut msg: Value, writer: &mpsc::Sender<Value>) -> Result<()> {
        let Some(child) = self.children.get_mut(&key) else {
            return Ok(());
        };
        let id = msg["id"].clone();
        let Some(method) = msg["method"].as_str().map(str::to_owned) else {
            if (msg.get("result").is_some() != msg.get("error").is_some())
                && child.permissions.remove(&id)
                && !self.ambiguous(&id)
            {
                self.write(writer, msg)?;
            }
            return Ok(());
        };
        if child.closing || !child.ready {
            return Ok(());
        }
        let allowed = matches!(
            method.as_str(),
            "session/prompt" | "session/close" | "session/cancel"
        );
        if msg["params"]["sessionId"] != child.sid
            || !allowed
            || (method == "session/cancel") != id.is_null()
        {
            if !id.is_null() {
                let _ = child.output.try_send(error_response(
                    &id,
                    -32602,
                    "operation unavailable on read-only fork",
                ));
            }
            return Ok(());
        }
        if method == "session/cancel" {
            self.write(writer, msg)?;
        } else {
            if method == "session/close" {
                child.closing = true;
            }
            self.request(
                writer,
                &method,
                msg["params"].take(),
                Pending::Forward {
                    key,
                    client_id: id,
                    method: method.clone(),
                },
            )?;
        }
        Ok(())
    }

    fn retire(&mut self, key: Uuid, writer: &mpsc::Sender<Value>) -> Result<()> {
        let Some(child) = self.children.remove(&key) else {
            return Ok(());
        };
        for pending in self.pending.values_mut() {
            let owned = match pending {
                Pending::Forward { key: owner, .. } | Pending::Mode { key: owner, .. } => {
                    *owner == key
                }
                Pending::Fork(_) | Pending::LateFork { .. } | Pending::Discard => false,
            };
            if !owned {
                continue;
            }
            match std::mem::replace(pending, Pending::Discard) {
                Pending::Forward { client_id, .. } => {
                    let _ = child.output.try_send(error_response(
                        &client_id,
                        -32001,
                        "read-only fork closed",
                    ));
                }
                Pending::Mode { answer, .. } => {
                    let _ =
                        answer.send(Err(anyhow!("read-only fork closed during mode selection")));
                }
                _ => unreachable!(),
            }
        }
        if !child.closing {
            self.request(
                writer,
                "session/close",
                json!({"sessionId":child.sid}),
                Pending::Discard,
            )?;
        }
        Ok(())
    }
}
