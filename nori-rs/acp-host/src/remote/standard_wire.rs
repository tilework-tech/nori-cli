//! Multiplex downstream ACP connections over the authenticated machine socket.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::Message;
use axum::extract::ws::WebSocket;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::host_control::failure;
use super::managed::ManagedAgent;
use super::managed_wire::send_frame;

const MAX_LANES: usize = 128;
const QUEUE_DEPTH: usize = 32;

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Scope {
    binding_id: String,
    generation: i64,
}

struct Outgoing {
    scope: Scope,
    message: Option<Value>,
}

impl Outgoing {
    fn into_frame(self) -> Message {
        let mut envelope =
            json!({"bindingId": self.scope.binding_id, "generation": self.scope.generation});
        if let Some(message) = self.message {
            envelope["message"] = message;
        } else {
            envelope["closed"] = Value::Bool(true);
        }
        Message::Text(envelope.to_string().into())
    }
}

fn unscoped_error(code: i32, description: &str) -> Message {
    let error = failure(Value::Null, code, description);
    Message::Text(error.response.to_string().into())
}

fn decode(text: &str) -> Result<(Scope, Value), Message> {
    let mut envelope: Value =
        serde_json::from_str(text).map_err(|_| unscoped_error(-32700, "invalid JSON"))?;
    let scope = Scope::deserialize(&envelope)
        .map_err(|_| unscoped_error(-32600, "expected bindingId and generation"))?;
    let message = &envelope["message"];
    if message.get("method").is_none()
        && message.get("result").is_none()
        && message.get("error").is_none()
    {
        let id = &message["id"];
        let id = if id.is_string() || id.is_i64() {
            id.clone()
        } else {
            Value::Null
        };
        let response = failure(id, -32600, "expected a JSON-RPC request or response").response;
        return Err(Outgoing {
            scope,
            message: Some(response),
        }
        .into_frame());
    }
    Ok((scope, envelope["message"].take()))
}

struct Lane {
    inbox: mpsc::Sender<Value>,
    cancel: CancellationToken,
}

pub(super) async fn serve<M: ManagedAgent>(socket: WebSocket, host: Arc<M>) -> anyhow::Result<()> {
    let (mut sink, mut incoming) = socket.split();
    let (outbox, mut outgoing) = mpsc::channel::<Outgoing>(QUEUE_DEPTH);
    let mut lanes = HashMap::<Scope, Lane>::new();
    let mut workers = FuturesUnordered::new();
    loop {
        tokio::select! {
            Some(()) = workers.next(), if !workers.is_empty() => {},
            Some(frame) = outgoing.recv() => {
                if frame.message.is_none() {
                    lanes.remove(&frame.scope);
                }
                send_frame(&mut sink, frame.into_frame()).await?;
            }
            frame = incoming.next() => {
                let Some(frame) = frame else { break; };
                let text = match frame? {
                    Message::Text(text) => text,
                    Message::Close(_) => break,
                    Message::Ping(bytes) => {
                        send_frame(&mut sink, Message::Pong(bytes)).await?;
                        continue;
                    }
                    Message::Pong(_) => continue,
                    Message::Binary(_) => {
                        send_frame(&mut sink, unscoped_error(-32600, "expected a text machine envelope")).await?;
                        continue;
                    }
                };
                let (scope, message) = match decode(&text) {
                    Ok(decoded) => decoded,
                    Err(error) => {
                        send_frame(&mut sink, error).await?;
                        continue;
                    }
                };
                if let Some(lane) = lanes.get(&scope) {
                    if lane.inbox.try_send(message).is_err() {
                        lane.cancel.cancel();
                    }
                    continue;
                }

                let id = &message["id"];
                let initialize = message["jsonrpc"] == "2.0"
                    && message["method"] == "initialize"
                    && (id.is_string() || id.is_i64())
                    && message["params"]["protocolVersion"].is_i64();
                let error = if !initialize {
                    Some("first scoped message must be an initialize request")
                } else if lanes.len() >= MAX_LANES {
                    Some("machine connection has reached the logical connection limit")
                } else {
                    None
                };
                if let Some(error) = error {
                    if message["method"].is_string() && !id.is_null() {
                        let reply = failure(id.clone(), -32001, error).response;
                        send_frame(&mut sink, Outgoing { scope: scope.clone(), message: Some(reply) }.into_frame()).await?;
                    }
                    send_frame(&mut sink, Outgoing { scope, message: None }.into_frame()).await?;
                    continue;
                }
                let (inbox, pending) = mpsc::channel(QUEUE_DEPTH);
                let cancel = CancellationToken::new();
                lanes.insert(scope.clone(), Lane { inbox, cancel: cancel.clone() });
                workers.push(serve_lane(host.clone(), scope, message, pending, outbox.clone(), cancel));
            }
        }
    }
    Ok(())
}

async fn serve_lane<M: ManagedAgent>(
    host: Arc<M>,
    scope: Scope,
    initialize: Value,
    mut pending: mpsc::Receiver<Value>,
    outbox: mpsc::Sender<Outgoing>,
    cancel: CancellationToken,
) {
    let relay = async {
        let mut connection = match host
            .open_standard_acp(&scope.binding_id, scope.generation)
            .await
        {
            Ok(connection) => connection,
            Err(error) => {
                let reply = failure(initialize["id"].clone(), -32001, &error.to_string()).response;
                let _ = outbox
                    .send(Outgoing {
                        scope: scope.clone(),
                        message: Some(reply),
                    })
                    .await;
                return;
            }
        };
        let to_agent = async {
            if connection.input.send(initialize).await.is_err() {
                return;
            }
            while let Some(message) = pending.recv().await {
                if connection.input.send(message).await.is_err() {
                    break;
                }
            }
        };
        let from_agent = async {
            while let Some(message) = connection.output.recv().await {
                if outbox
                    .send(Outgoing {
                        scope: scope.clone(),
                        message: Some(message),
                    })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        };
        tokio::select! {
            _ = to_agent => {},
            _ = from_agent => {},
        }
    };
    tokio::select! {
        _ = cancel.cancelled() => {},
        _ = relay => {},
    }
    let _ = outbox
        .send(Outgoing {
            scope,
            message: None,
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn malformed_json_returns_an_unscoped_parse_error() -> anyhow::Result<()> {
        let Err(error) = decode(r#"{"bindingId":"synthetic","generation":1,"message":"#) else {
            panic!("malformed JSON was accepted");
        };
        let Message::Text(text) = error else {
            panic!("expected JSON-RPC error frame");
        };
        let response: Value = serde_json::from_str(&text)?;
        assert_eq!(
            response,
            json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"invalid JSON"}})
        );
        Ok(())
    }
}
