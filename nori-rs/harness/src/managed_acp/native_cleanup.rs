//! Track adapter ownership separately from the lifetime of a fork's client endpoint.
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use uuid::Uuid;

use super::Closing;
use super::Endpoint;
use super::Native;
use super::Pending;
use super::cancelled_permission;
use super::error_response;

impl Native {
    pub(super) fn drain(
        &mut self,
        parent: String,
        answer: oneshot::Sender<Result<()>>,
        writer: &mpsc::Sender<Value>,
    ) -> Result<()> {
        let waiters = self.drains.entry(parent.clone()).or_default();
        let retry = !waiters.is_empty() && waiters.iter().all(oneshot::Sender::is_closed);
        waiters.retain(|answer| !answer.is_closed());
        waiters.push(answer);
        let children: Vec<_> = self
            .children
            .iter()
            .filter(|(_, c)| c.parent == parent)
            .map(|(key, _)| *key)
            .collect();
        for key in children {
            self.close_child(key, None, retry, writer)?;
        }
        self.finish_drain(&parent);
        Ok(())
    }

    pub(super) fn finish_drain(&mut self, parent: &str) {
        if !self.drains.contains_key(parent)
            || self
                .pending
                .values()
                .any(|p| matches!(p, Pending::LateFork { parent: p } if p == parent))
        {
            return;
        }
        let mut refused = false;
        for child in self
            .children
            .values()
            .filter(|child| child.parent == parent)
        {
            if child.closing.is_some() {
                return;
            }
            refused = true;
        }
        for answer in self.drains.remove(parent).unwrap_or_default() {
            let result = if refused {
                Err(anyhow!("adapter declined to close one or more forks"))
            } else {
                Ok(())
            };
            let _ = answer.send(result);
        }
    }

    pub(super) fn close_child(
        &mut self,
        key: Uuid,
        client_id: Option<Value>,
        retry: bool,
        writer: &mpsc::Sender<Value>,
    ) -> Result<()> {
        let Some(child) = self.children.get_mut(&key) else {
            return Ok(());
        };
        let mut client_ids: Vec<Value> = client_id.into_iter().collect();
        if let Some(closing) = &mut child.closing {
            closing.client_ids.append(&mut client_ids);
            if !retry {
                return Ok(());
            }
            client_ids = std::mem::take(&mut closing.client_ids);
        }
        let params = json!({"sessionId":child.sid});
        let wire_id = self.request(writer, "session/close", params, Pending::Close(key))?;
        self.children
            .get_mut(&key)
            .context("fork missing during close")?
            .closing = Some(Closing {
            wire_id,
            client_ids,
        });
        Ok(())
    }

    pub(super) fn closed(
        &mut self,
        key: Uuid,
        msg: &mut Value,
        writer: &mpsc::Sender<Value>,
    ) -> Result<()> {
        let Some(child) = self.children.get_mut(&key) else {
            return Ok(());
        };
        let parent = child.parent.clone();
        super::super::normalize_close(msg);
        let confirmed = msg["result"].is_object();
        if !confirmed
            && child
                .closing
                .as_ref()
                .is_none_or(|closing| closing.wire_id != msg["id"])
        {
            return Ok(());
        }
        let mut delivery_failed = false;
        if let Some(closing) = child.closing.take()
            && let Some(endpoint) = &child.endpoint
        {
            for id in closing.client_ids {
                msg["id"] = id;
                delivery_failed |= endpoint.output.try_send(msg.clone()).is_err();
            }
        }
        if confirmed {
            self.detach(key);
            self.children.remove(&key);
        } else if delivery_failed {
            self.retire(key, writer)?;
        }
        self.finish_drain(&parent);
        Ok(())
    }

    pub(super) fn retire(&mut self, key: Uuid, writer: &mpsc::Sender<Value>) -> Result<()> {
        if let Some(endpoint) = self.detach(key) {
            for id in endpoint.permissions {
                if !self.ambiguous(&id) {
                    self.write(writer, cancelled_permission(&id))?;
                }
            }
            self.close_child(key, None, false, writer)?;
        }
        Ok(())
    }

    fn detach(&mut self, key: Uuid) -> Option<Endpoint> {
        let endpoint = self.children.get_mut(&key)?.endpoint.take()?;
        for pending in self.pending.values_mut() {
            if !matches!(pending, Pending::Forward {key: owner, ..} | Pending::Mode {key: owner, ..} if *owner == key)
            {
                continue;
            }
            match std::mem::replace(pending, Pending::Discard) {
                Pending::Forward { client_id, .. } => {
                    let _ = endpoint.output.try_send(error_response(
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
        Some(endpoint)
    }
}
