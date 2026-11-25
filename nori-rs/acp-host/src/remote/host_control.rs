//! Connection ownership and serialized mutation admission for managed hosts.

use std::collections::HashMap;

use serde_json::Value;
use serde_json::json;
use tokio::sync::watch;

use super::managed::ManagedReply;

pub(super) enum HostRequest {
    Reply(ManagedReply),
    Forward(Value),
}

#[derive(Default)]
pub(super) struct HostControl {
    controller: Option<i64>,
    clients: HashMap<i64, watch::Sender<&'static str>>,
}

impl HostControl {
    pub(super) fn connected(&self) -> bool {
        self.controller.is_some()
    }

    pub(super) fn remove(&mut self, id: i64) {
        self.clients.remove(&id);
        if self.controller == Some(id) {
            self.controller = None;
        }
    }

    fn role(&self, id: i64) -> &'static str {
        if self.controller == Some(id) {
            "controller"
        } else {
            "watcher"
        }
    }

    fn status(&self, id: i64) -> Value {
        json!({"role":self.role(id), "controller_connected":self.connected()})
    }

    /// Register only after the host has successfully initialized this client.
    pub(super) fn register(
        &mut self,
        client: i64,
        changes: &watch::Sender<&'static str>,
        observe: bool,
    ) -> Value {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.clients.entry(client) {
            entry.insert(changes.clone());
            if !observe && self.controller.is_none() {
                self.controller = Some(client);
                changes.send_replace("controller");
            }
        }
        self.status(client)
    }

    /// Decide ownership at admission. The caller releases the mutex before
    /// dispatching forwarded work; admitted requests may finish after handoff.
    pub(super) fn admit(
        &mut self,
        client: i64,
        changes: &watch::Sender<&'static str>,
        request: Value,
    ) -> HostRequest {
        let id = request["id"].clone();
        let method = request["method"].as_str().unwrap_or_default();
        match method {
            "_nori/control/status" => HostRequest::Reply(success(id, self.status(client))),
            "_nori/control/take" => {
                if self.controller != Some(client) {
                    let previous = self.controller.replace(client);
                    if let Some(previous) = previous
                        && let Some(changes) = self.clients.get(&previous)
                    {
                        changes.send_replace("watcher");
                    }
                    changes.send_replace("controller");
                }
                HostRequest::Reply(success(id, self.status(client)))
            }
            // Starting a session leaves every existing one alone, so any
            // authenticated client may do it, as with stopping the host.
            // Finding paths only reads the host's filesystem.
            "_nori/session/list"
            | "_nori/session/find_paths"
            | "_nori/session/ensure"
            | "_nori/session/status"
            | "_nori/session/subscribe"
            | "_nori/session/new"
            | "_nori/host/prepare_agent"
            | "_nori/host/discard_agent"
            | "_nori/host/stop"
            | "session/new"
            | "session/list"
            | "session/load"
            | "session/resume" => HostRequest::Forward(request),
            _ if self.controller == Some(client) => HostRequest::Forward(request),
            _ => HostRequest::Reply(failure(
                id,
                -32016,
                "watcher is read-only; take control before changing a session",
            )),
        }
    }
}

fn success(id: Value, result: Value) -> ManagedReply {
    ManagedReply {
        delivery_guard: None,
        response: json!({"jsonrpc":"2.0","id":id,"result":result}),
        subscription: None,
    }
}

pub(super) fn failure(id: Value, code: i32, message: &str) -> ManagedReply {
    ManagedReply {
        delivery_guard: None,
        response: json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message}}),
        subscription: None,
    }
}
