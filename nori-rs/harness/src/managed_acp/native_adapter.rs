//! Dispatch side-session messages from the shared adapter before the parent relay.
use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Result;
use anyhow::anyhow;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::Child;
use super::Native;
use super::Opening;
use super::Pending;
use super::Request;
use super::error_response;

impl Native {
    /// Consume responses and callbacks addressed to private endpoints before parent services see them.
    pub(in crate::managed_acp) fn agent(
        &mut self,
        msg: &mut Value,
        requests: &HashMap<Value, Request>,
        parent_permissions: &HashSet<Value>,
        writer: &mpsc::Sender<Value>,
    ) -> Result<bool> {
        if msg["method"].is_string() {
            let Some(sid) = msg["params"]["sessionId"].as_str() else {
                return Ok(false);
            };
            let private = self.private_ids.get(sid).copied();
            let opening_regular = requests.values().any(|r| {
                matches!(r.method.as_str(), "session/load" | "session/resume")
                    && r.sid.as_deref() == Some(sid)
            });
            let unclaimed = !opening_regular && self.inaccessible(sid);
            if let Some(id) = msg.get("id") {
                let previous = self
                    .children
                    .iter()
                    .find(|(_, child)| child.permissions.contains(id))
                    .map(|(key, _)| *key);
                if self.ambiguous(id)
                    || previous.is_some()
                    || ((private.is_some() || unclaimed) && parent_permissions.contains(id))
                {
                    self.ambiguous_permissions.insert(id.clone());
                    if let Some(key) = previous {
                        self.retire(key, writer)?;
                    }
                    if let Some(key) = private {
                        self.retire(key, writer)?;
                    }
                    return Ok(true);
                }
            }
            let Some(key) = private else {
                if unclaimed
                    && !msg["id"].is_null()
                    && !self.deferred()
                    && !parent_permissions.contains(&msg["id"])
                {
                    self.write(
                        writer,
                        error_response(&msg["id"], -32001, "fork is not ready"),
                    )?;
                }
                return Ok(unclaimed);
            };
            let Some(child) = self.children.get_mut(&key) else {
                return Ok(true);
            };
            let changed = &msg["params"]["update"];
            if changed["sessionUpdate"] == "current_mode_update"
                && changed["currentModeId"] != child.mode
            {
                self.retire(key, writer)?;
            } else if !child.ready {
                if !msg["id"].is_null() && !self.deferred() {
                    self.write(
                        writer,
                        error_response(&msg["id"], -32001, "fork is not ready"),
                    )?;
                }
            } else if !child.closing {
                if msg["method"] == "session/request_permission" && !msg["id"].is_null() {
                    child.permissions.insert(msg["id"].clone());
                } else if msg["method"] != "session/update" || !msg["id"].is_null() {
                    if !msg["id"].is_null() && !self.deferred() {
                        self.write(
                            writer,
                            error_response(
                                &msg["id"],
                                -32601,
                                "client operation unavailable on read-only fork",
                            ),
                        )?;
                    }
                    return Ok(true);
                }
                if child.output.try_send(msg.take()).is_err() {
                    self.retire(key, writer)?;
                }
            }
            return Ok(true);
        }

        let Some(pending) = self.pending.remove(&msg["id"]) else {
            return Ok(false);
        };
        match pending {
            Pending::Fork(open) => self.forked(open, &msg["result"], requests, writer)?,
            Pending::LateFork { parent } => {
                if let Some(sid) = msg["result"]["sessionId"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    && self.fresh_id(sid, &parent, requests)
                {
                    self.private_ids.insert(sid.to_owned(), Uuid::new_v4());
                    self.request(
                        writer,
                        "session/close",
                        json!({"sessionId":sid}),
                        Pending::Discard,
                    )?;
                }
            }
            Pending::Mode {
                key,
                mut result,
                answer,
            } => {
                if !msg["result"].is_object()
                    || msg.get("error").is_some()
                    || !self.children.contains_key(&key)
                {
                    self.retire(key, writer)?;
                    let _ = answer.send(Err(anyhow!(
                        "adapter could not confirm the independent read-only mode"
                    )));
                } else if let Some(child) = self.children.get_mut(&key) {
                    child.ready = true;
                    result["modes"]["currentModeId"] = json!(child.mode);
                    if answer.send(Ok(result)).is_err() {
                        self.retire(key, writer)?;
                    }
                }
            }
            Pending::Forward {
                key,
                client_id,
                method,
            } => {
                if let Some(child) = self.children.get_mut(&key) {
                    let closed = method == "session/close"
                        && msg.get("result").is_some()
                        && msg.get("error").is_none();
                    if method == "session/close" && !closed {
                        child.closing = false;
                    }
                    msg["id"] = client_id;
                    let failed = child.output.try_send(msg.take()).is_err();
                    if closed || failed {
                        self.retire(key, writer)?;
                    }
                }
            }
            Pending::Discard => {}
        }
        Ok(true)
    }

    fn fresh_id(&self, sid: &str, parent: &str, requests: &HashMap<Value, Request>) -> bool {
        sid != parent
            && !self.parents.contains(sid)
            && !self.private(sid)
            && !requests.values().any(|r| r.sid.as_deref() == Some(sid))
    }

    fn forked(
        &mut self,
        open: Opening,
        result: &Value,
        requests: &HashMap<Value, Request>,
        writer: &mpsc::Sender<Value>,
    ) -> Result<()> {
        let Some(sid) = result["sessionId"].as_str().filter(|s| !s.is_empty()) else {
            let _ = open
                .answer
                .send(Err(anyhow!("adapter did not return a fork")));
            return Ok(());
        };
        if !self.fresh_id(sid, &open.parent, requests) {
            let _ = open
                .answer
                .send(Err(anyhow!("adapter returned an existing session ID")));
            return Ok(());
        }
        self.private_ids.insert(sid.to_owned(), open.key);
        self.children.insert(
            open.key,
            Child {
                parent: open.parent.clone(),
                sid: sid.to_owned(),
                mode: open.mode.clone(),
                output: open.output,
                permissions: HashSet::new(),
                closing: false,
                ready: false,
            },
        );
        let available = result["modes"]["availableModes"]
            .as_array()
            .is_some_and(|modes| modes.iter().any(|mode| mode["id"] == open.mode));
        if open.answer.is_closed() || !available {
            self.retire(open.key, writer)?;
            let _ = open.answer.send(Err(anyhow!(
                "fork did not offer the guaranteed read-only mode on a live parent"
            )));
            return Ok(());
        }
        self.request(
            writer,
            "session/set_mode",
            json!({"sessionId":sid,"modeId":open.mode}),
            Pending::Mode {
                key: open.key,
                result: result.clone(),
                answer: open.answer,
            },
        )
    }
}
