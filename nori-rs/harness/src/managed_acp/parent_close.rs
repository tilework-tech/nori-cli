//! Join adapter cleanup and side-worker shutdown before answering a parent close.
use std::collections::hash_map::Entry;

use anyhow::Result;
use futures::future::BoxFuture;
use serde_json::Value;
use serde_json::json;

use super::Router;
use super::invalid;

pub(super) type Cleanup = BoxFuture<'static, (String, Result<()>)>;

pub(super) struct ClosingParent {
    response: Value,
    pub request_id: Option<Value>,
    pub outcome: Option<Result<()>>,
}

impl Router {
    pub(super) fn parent_close_pending(&self, parent: &str) -> bool {
        self.requests.values().any(|request| {
            request.method == "session/close" && request.sid.as_deref() == Some(parent)
        })
    }

    pub(super) fn begin_parent_cleanup(&mut self, parent: &str, response: Value) {
        self.parents.remove(parent);
        for side in self.sides.values_mut() {
            if side.parent.session_id == parent && !side.native {
                side.input = None;
            }
        }
        self.closing.insert(
            parent.into(),
            ClosingParent {
                request_id: Some(response["id"].clone()),
                response,
                outcome: None,
            },
        );
        self.close_forks(parent);
    }

    fn close_forks(&mut self, parent: &str) {
        let forks = self.forks.clone();
        let parent = parent.to_owned();
        self.cleanup.push(Box::pin(async move {
            let result = forks.close_children(&parent).await;
            (parent, result)
        }));
    }

    pub(super) async fn route_closing_parent(
        &mut self,
        parent: &str,
        method: &str,
        id: Option<&Value>,
    ) -> Result<bool> {
        if let Some(state) = self.closing.get_mut(parent) {
            if let Some(id) = id {
                if method == "session/close" && state.request_id.is_none() {
                    state.request_id = Some(id.clone());
                    self.close_forks(parent);
                } else {
                    let code = if method == "session/close" {
                        -32015
                    } else {
                        -32002
                    };
                    self.output
                        .send(invalid(
                            id,
                            code,
                            "parent is closed and side cleanup is pending",
                        ))
                        .await?;
                }
            }
            return Ok(true);
        }
        let conflicts = match method {
            "session/close" => self.requests.values().any(|request| {
                request.sid.as_deref() == Some(parent)
                    && matches!(
                        request.method.as_str(),
                        "session/close" | "session/resume" | "session/load"
                    )
            }),
            "session/resume" | "session/load" => self.parent_close_pending(parent),
            _ => false,
        };
        if conflicts {
            if let Some(id) = id {
                self.output
                    .send(invalid(
                        id,
                        -32015,
                        "session lifecycle request already pending",
                    ))
                    .await?;
            }
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) async fn finish_parent_cleanup(&mut self, parent: &str) -> Result<()> {
        let Entry::Occupied(mut entry) = self.closing.entry(parent.to_owned()) else {
            return Ok(());
        };
        let state = entry.get_mut();
        let Some(outcome) = &state.outcome else {
            return Ok(());
        };
        let mut children = self
            .sides
            .values()
            .filter(|side| side.parent.session_id == parent);
        let exit_error = children.clone().find_map(|side| side.exit_error.as_ref());
        if exit_error.is_none() && children.any(|side| !side.native || outcome.is_ok()) {
            return Ok(());
        }
        let Some(id) = state.request_id.take() else {
            return Ok(());
        };
        let response = match (outcome, exit_error) {
            (Ok(()), Some(reason)) => {
                state.outcome = None;
                invalid(&id, -32000, reason)
            }
            (Ok(()), None) => {
                let mut response = entry.remove().response;
                response["id"] = id;
                response
            }
            (Err(error), _) => {
                let mut reply = invalid(
                    &id,
                    -32000,
                    &format!("parent closed; side cleanup pending: {error}"),
                );
                state.outcome = None;
                reply["error"]["data"] = json!({"parentClosed":true,"retry":true});
                reply
            }
        };
        self.output.send(response).await?;
        Ok(())
    }
}
