//! Multiplex downstream ACP connections over the authenticated machine socket.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use anyhow::ensure;
use axum::extract::ws::Message;
use axum::extract::ws::WebSocket;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tokio_util::sync::DropGuard;

use super::host_control::failure;
use super::managed::ManagedAgent;
use super::managed_wire::send_frame;
use crate::recovery::Scope;
use crate::recovery::SharedRecovery;
use crate::recovery::StorageFailure;
use crate::recovery::journal::Record;
use crate::recovery::journal::Source;

const MAX_LANES: usize = 128;
const QUEUE_DEPTH: usize = 32;

#[derive(Debug)]
struct Input {
    message: Value,
    command: Option<String>,
}

enum Operation {
    Acp(Input),
    Attach(Vec<Source>),
}

impl Operation {
    fn error_id(&self) -> Option<Value> {
        match self {
            Self::Attach(_) => Some(Value::Null),
            Self::Acp(input)
                if input.message["method"].is_string() && !input.message["id"].is_null() =>
            {
                Some(reply_id(&input.message))
            }
            _ => None,
        }
    }
}

struct Outgoing {
    scope: Scope,
    message: Option<Value>,
    source: Option<Source>,
}

impl Outgoing {
    fn new(scope: &Scope, message: Option<Value>, source: Option<Source>) -> Self {
        Self {
            scope: scope.clone(),
            message,
            source,
        }
    }
    fn into_frame(self) -> Message {
        let mut envelope =
            json!({"bindingId": self.scope.binding_id, "generation": self.scope.generation});
        if let Some(message) = self.message {
            envelope["message"] = message;
        } else {
            envelope["closed"] = Value::Bool(true);
        }
        if let Some(source) = self.source {
            envelope["source"] = json!(source);
        }
        Message::Text(envelope.to_string().into())
    }
}

fn reply_id(message: &Value) -> Value {
    message
        .get("id")
        .filter(|id| id.is_string() || id.is_i64())
        .cloned()
        .unwrap_or_default()
}

fn unscoped_error(code: i32, description: &str) -> Message {
    Message::Text(
        failure(Value::Null, code, description)
            .response
            .to_string()
            .into(),
    )
}

fn decode(text: &str) -> Result<(Scope, Operation), Message> {
    let mut envelope: Value =
        serde_json::from_str(text).map_err(|_| unscoped_error(-32700, "invalid JSON"))?;
    let scope = Scope::deserialize(&envelope)
        .map_err(|_| unscoped_error(-32600, "expected bindingId and generation"))?;
    if let Some(from) = envelope.get("attachFrom") {
        if envelope.get("message").is_none()
            && envelope.get("commandId").is_none()
            && let Ok(cursors) = Vec::<Source>::deserialize(from)
        {
            return Ok((scope, Operation::Attach(cursors)));
        }
        let reply = failure(
            Value::Null,
            -32600,
            "expected standalone attachFrom cursor array",
        )
        .response;
        return Err(Outgoing::new(&scope, Some(reply), None).into_frame());
    }
    let command = envelope
        .get("commandId")
        .map(|v| v.as_str().map(str::to_owned));
    let message = &envelope["message"];
    if message.get("method").is_none()
        && message.get("result").is_none()
        && message.get("error").is_none()
    {
        let response = failure(
            reply_id(message),
            -32600,
            "expected a JSON-RPC request or response",
        )
        .response;
        return Err(Outgoing::new(&scope, Some(response), None).into_frame());
    }
    if command == Some(None) {
        let response = failure(
            reply_id(message),
            -32600,
            "expected optional string commandId",
        )
        .response;
        return Err(Outgoing::new(&scope, Some(response), None).into_frame());
    }
    Ok((
        scope,
        Operation::Acp(Input {
            message: envelope["message"].take(),
            command: command.flatten(),
        }),
    ))
}

#[derive(Clone)]
struct Client {
    frames: mpsc::Sender<Outgoing>,
    close: CancellationToken,
}

impl Client {
    fn send_durable(&self, frame: Outgoing) {
        if !self.close.is_cancelled() && self.frames.try_send(frame).is_err() {
            self.close.cancel();
        }
    }
}

#[derive(Clone)]
struct Lane {
    inbox: mpsc::Sender<Input>,
    client: Arc<Mutex<Client>>,
    cancel: CancellationToken,
}

impl Lane {
    async fn deliver(&self, frame: Outgoing, durable: bool) -> bool {
        let client = self
            .client
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if durable {
            client.send_durable(frame);
            true
        } else {
            client.frames.send(frame).await.is_ok()
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct Lanes(Arc<Mutex<HashMap<Scope, Lane>>>);

struct Attachment {
    lane: Lane,
    close_on_disconnect: Option<DropGuard>,
}

enum Open {
    Attached(Attachment, Vec<Record>),
    Replied(Value),
}

impl Lanes {
    async fn open<M: ManagedAgent>(
        &self,
        host: &Arc<M>,
        scope: &Scope,
        operation: &Operation,
        client: &Client,
        shutdown: &CancellationToken,
    ) -> Result<Open> {
        let existing = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(scope)
            .cloned();
        if let Some(lane) = existing {
            let cursors = match operation {
                Operation::Attach(from) => from.clone(),
                Operation::Acp(_) => vec![],
            };
            let scope = scope.clone();
            let live = lane.clone();
            let client = client.clone();
            let replay = host
                .recovery()
                .context("binding has not negotiated durable recovery")?
                .run(move |r| {
                    let mut previous = live.client.lock().unwrap_or_else(PoisonError::into_inner);
                    ensure!(!live.inbox.is_closed(), "binding has no live ACP relay");
                    ensure!(
                        previous.frames.is_closed(),
                        "logical connection already attached"
                    );
                    let records =
                        r.journal
                            .replay_from(&scope.binding_id, scope.generation, &cursors)?;
                    *previous = client;
                    Ok(records)
                })
                .await?;
            return Ok(Open::Attached(
                Attachment {
                    lane,
                    close_on_disconnect: None,
                },
                replay,
            ));
        }
        let Operation::Acp(input) = operation else {
            bail!("binding has no live ACP relay");
        };
        let recovery = if let Some(recovery) = host.recovery() {
            let binding = scope.binding_id.clone();
            recovery
                .run(move |r| r.journal.contains(&binding))
                .await?
                .then_some(recovery)
        } else {
            None
        };
        let message = &input.message;
        let initialize = message["jsonrpc"] == "2.0"
            && message["method"] == "initialize"
            && (message["id"].is_string() || message["id"].is_i64())
            && message["params"]["protocolVersion"].is_i64();
        if !initialize {
            if let (Some(recovery), Some(command)) = (&recovery, &input.command) {
                let scope = scope.clone();
                let command = command.clone();
                let message = message.clone();
                let reply = recovery
                    .run(move |r| {
                        if let Err(error) =
                            r.journal
                                .command(&scope.binding_id, scope.generation, &command)
                        {
                            r.check_admission_storage(&scope)?;
                            return Err(error);
                        }
                        r.incoming(&scope, &message, Some(&command))?
                            .context("command cannot be reconciled")
                    })
                    .await?;
                return Ok(Open::Replied(reply));
            }
            bail!("first scoped message must be an initialize request");
        }
        ensure!(
            input.command.is_none(),
            "commandId is reserved for session/prompt"
        );
        let (inbox, pending) = mpsc::channel(QUEUE_DEPTH);
        let lane = Lane {
            inbox,
            client: Arc::new(Mutex::new(client.clone())),
            cancel: shutdown.child_token(),
        };
        let durable = recovery.is_some();
        let producer = if let Some(recovery) = &recovery {
            let producer = recovery.start_producer(scope.clone()).await?;
            let mut lanes = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            lanes.insert(scope.clone(), lane.clone());
            Some(producer)
        } else {
            None
        };
        let routes = self.clone();
        let worker = lane.clone();
        let host = host.clone();
        let scope = scope.clone();
        let id = message["id"].clone();
        tokio::spawn(async move {
            let run = async {
                match host
                    .open_standard_acp(&scope.binding_id, scope.generation)
                    .await
                {
                    Ok(connection) => {
                        relay(connection, &scope, &worker, pending, recovery.as_ref()).await
                    }
                    Err(error) => {
                        let reply = failure(
                            id,
                            StorageFailure::rpc_code(&error, -32001),
                            &error.to_string(),
                        )
                        .response;
                        worker
                            .deliver(Outgoing::new(&scope, Some(reply), None), durable)
                            .await;
                        Err(error)
                    }
                }
            };
            let finished = tokio::select! {
                _ = worker.cancel.cancelled() => Err(anyhow::anyhow!("ACP relay cancelled")),
                result = run => result,
            };
            if let Err(error) = &finished {
                tracing::debug!(%error, "scoped ACP relay ended");
            }
            if let Some(producer) = producer {
                if let Err(error) = producer
                    .finish(finished.err().map(|error| error.to_string()))
                    .await
                {
                    tracing::warn!(%error, "failed to retire ACP producer");
                }
                routes
                    .0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&scope);
            }
            worker
                .deliver(Outgoing::new(&scope, None, None), durable)
                .await;
        });
        Ok(Open::Attached(
            Attachment {
                close_on_disconnect: (!durable).then(|| lane.cancel.clone().drop_guard()),
                lane,
            },
            vec![],
        ))
    }
}

pub(super) async fn serve<M: ManagedAgent>(
    socket: WebSocket,
    host: Arc<M>,
    routes: Lanes,
    shutdown: CancellationToken,
) -> Result<()> {
    let (mut sink, mut incoming) = socket.split();
    let (frames, mut outgoing) = mpsc::channel::<Outgoing>(QUEUE_DEPTH);
    let client = Client {
        frames,
        close: shutdown.child_token(),
    };
    let _disconnect = client.close.clone().drop_guard();
    let mut lanes = HashMap::<Scope, Attachment>::new();
    loop {
        tokio::select! {
            _ = client.close.cancelled() => break,
            Some(frame) = outgoing.recv() => {
                if frame.message.is_none() { lanes.remove(&frame.scope); }
                send_frame(&mut sink, frame.into_frame()).await?;
            }
            frame = incoming.next() => {
                let Some(frame) = frame else { break; };
                let text = match frame? {
                    Message::Text(text) => text,
                    Message::Close(_) => break,
                    Message::Ping(bytes) => { send_frame(&mut sink, Message::Pong(bytes)).await?; continue; }
                    Message::Pong(_) => continue,
                    Message::Binary(_) => {
                        send_frame(&mut sink, unscoped_error(-32600, "expected a text machine envelope")).await?;
                        continue;
                    }
                };
                let (scope, operation) = match decode(&text) {
                    Ok(decoded) => decoded,
                    Err(error) => { send_frame(&mut sink, error).await?; continue; }
                };
                if !lanes.contains_key(&scope) {
                    let opened = if lanes.len() >= MAX_LANES {
                        Err(anyhow::anyhow!("machine connection has reached the logical connection limit"))
                    } else {
                        routes.open(&host, &scope, &operation, &client, &shutdown).await
                    };
                    match opened {
                        Ok(Open::Attached(attached, replay)) => {
                            lanes.insert(scope.clone(), attached);
                            for record in replay {
                                send_frame(&mut sink, Outgoing::new(&scope, Some(record.message), Some(record.source)).into_frame()).await?;
                            }
                            if matches!(operation, Operation::Attach(_)) {
                                let ready = json!({"bindingId":scope.binding_id,"generation":scope.generation,"attached":true});
                                send_frame(&mut sink, Message::Text(ready.to_string().into())).await?;
                                continue;
                            }
                        }
                        Ok(Open::Replied(reply)) => {
                            send_frame(&mut sink, Outgoing::new(&scope, Some(reply), None).into_frame()).await?;
                            continue;
                        }
                        Err(error) => {
                            if let Some(id) = operation.error_id() {
                                let reply = failure(id, StorageFailure::rpc_code(&error, -32001), &error.to_string()).response;
                                send_frame(&mut sink, Outgoing::new(&scope, Some(reply), None).into_frame()).await?;
                            }
                            if !matches!(operation, Operation::Attach(_)) {
                                send_frame(&mut sink, Outgoing::new(&scope, None, None).into_frame()).await?;
                            }
                            continue;
                        }
                    }
                }
                let Operation::Acp(input) = operation else {
                    let reply = failure(Value::Null, -32001, "logical connection already attached").response;
                    send_frame(&mut sink, Outgoing::new(&scope, Some(reply), None).into_frame()).await?;
                    continue;
                };
                if let Some(attached) = lanes.get(&scope)
                    && attached.lane.inbox.try_send(input).is_err() {
                    if attached.close_on_disconnect.is_some() { attached.lane.cancel.cancel(); }
                    else { break; }
                }
            }
        }
    }
    Ok(())
}

async fn relay(
    mut connection: super::managed::StandardAcpConnection,
    scope: &Scope,
    lane: &Lane,
    mut pending: mpsc::Receiver<Input>,
    recovery: Option<&SharedRecovery>,
) -> Result<()> {
    let from_agent = async {
        while let Some(message) = connection.output.recv().await {
            if let Some(recovery) = recovery {
                let scope = scope.clone();
                let client = lane.client.clone();
                recovery
                    .run(move |r| {
                        let source = r.outgoing(&scope, &message)?;
                        client
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .send_durable(Outgoing::new(&scope, Some(message), source));
                        Ok(())
                    })
                    .await?;
            } else if !lane
                .deliver(Outgoing::new(scope, Some(message), None), false)
                .await
            {
                break;
            }
        }
        connection.finished.await??;
        Ok(())
    };
    let to_agent = async {
        while let Some(input) = pending.recv().await {
            let message = input.message;
            let admission = if let Some(r) = recovery {
                r.incoming(scope.clone(), message.clone(), input.command)
                    .await
            } else if input.command.is_some() {
                Err(anyhow::anyhow!(
                    "binding has not negotiated durable recovery"
                ))
            } else {
                Ok(None)
            };
            let reply = match admission {
                Ok(None) => {
                    connection.input.send(message).await?;
                    continue;
                }
                Ok(Some(reply)) => reply,
                Err(error) => {
                    failure(
                        reply_id(&message),
                        StorageFailure::rpc_code(&error, -32001),
                        &error.to_string(),
                    )
                    .response
                }
            };
            if !lane
                .deliver(Outgoing::new(scope, Some(reply), None), recovery.is_some())
                .await
            {
                break;
            }
        }
        Ok(())
    };
    tokio::pin!(from_agent);
    tokio::select! {
        result = &mut from_agent => result,
        result = to_agent => {
            // Adapter stdin can close while its final stdout messages are still queued.
            let output = from_agent.await;
            result.and(output)
        }
    }
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
