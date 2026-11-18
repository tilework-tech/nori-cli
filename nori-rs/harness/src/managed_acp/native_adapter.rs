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
use super::Endpoint;
use super::Native;
use super::Opening;
use super::Pending;
use super::Request;
use super::cancelled_permission;
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
            if private.is_some_and(|key| !self.children.contains_key(&key)) {
                return Ok(true);
            }
            let opening_regular = requests.values().any(|r| {
                matches!(r.method.as_str(), "session/load" | "session/resume")
                    && r.sid.as_deref() == Some(sid)
            });
            let unclaimed = !opening_regular
                && (self.inaccessible(sid)
                    || (!self.parents.contains(sid)
                        && self
                            .pending
                            .values()
                            .any(|pending| matches!(pending, Pending::LateFork { .. }))));

            if let Some(id) = msg.get("id") {
                let previous = self
                    .children
                    .iter()
                    .find(|(_, child)| {
                        child
                            .endpoint
                            .as_ref()
                            .is_some_and(|e| e.permissions.contains(id))
                    })
                    .map(|(key, _)| *key);
                if private.is_some_and(|key| self.children[&key].endpoint.is_none()) {
                    if !self.ambiguous(id) && previous.is_none() && !parent_permissions.contains(id)
                    {
                        let reply = if msg["method"] == "session/request_permission" {
                            cancelled_permission(id)
                        } else {
                            error_response(id, -32002, "fork endpoint has ended")
                        };
                        self.write(writer, reply)?;
                    }
                    return Ok(true);
                }
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
            let Some(endpoint) = self
                .children
                .get_mut(&key)
                .and_then(|c| c.endpoint.as_mut())
            else {
                return Ok(true);
            };
            let changed = &msg["params"]["update"];
            if changed["sessionUpdate"] == "current_mode_update"
                && changed["currentModeId"] != endpoint.mode
            {
                self.retire(key, writer)?;
            } else if !endpoint.ready {
                if !msg["id"].is_null() && !self.deferred() {
                    self.write(
                        writer,
                        error_response(&msg["id"], -32001, "fork is not ready"),
                    )?;
                }
            } else {
                if msg["method"] == "session/request_permission" && !msg["id"].is_null() {
                    endpoint.permissions.insert(msg["id"].clone());
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
                if endpoint.output.try_send(msg.take()).is_err() {
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
                    let key = Uuid::new_v4();
                    self.private_ids.insert(sid.to_owned(), key);
                    self.children.insert(
                        key,
                        Child {
                            sid: sid.to_owned(),
                            parent: parent.clone(),
                            endpoint: None,
                            closing: None,
                        },
                    );
                    self.close_child(key, None, false, writer)?;
                }
                self.finish_drain(&parent);
            }
            Pending::Mode {
                key,
                mut result,
                answer,
            } => {
                if !msg["result"].is_object()
                    || msg.get("error").is_some()
                    || self.children.get(&key).is_none_or(|c| c.endpoint.is_none())
                {
                    self.retire(key, writer)?;
                    let _ = answer.send(Err(anyhow!(
                        "adapter could not confirm the independent read-only mode"
                    )));
                } else if let Some(endpoint) = self
                    .children
                    .get_mut(&key)
                    .and_then(|c| c.endpoint.as_mut())
                {
                    endpoint.ready = true;
                    result["modes"]["currentModeId"] = json!(endpoint.mode);
                    if answer.send(Ok(result)).is_err() {
                        self.retire(key, writer)?;
                    }
                }
            }
            Pending::Forward { key, client_id } => {
                if let Some(endpoint) = self.children.get(&key).and_then(|c| c.endpoint.as_ref()) {
                    msg["id"] = client_id;
                    if endpoint.output.try_send(msg.take()).is_err() {
                        self.retire(key, writer)?;
                    }
                }
            }
            Pending::Close(key) => self.closed(key, msg, writer)?,
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
                endpoint: Some(Endpoint {
                    mode: open.mode.clone(),
                    output: open.output,
                    permissions: HashSet::new(),
                    ready: false,
                }),
                closing: None,
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
        )?;
        Ok(())
    }
}
