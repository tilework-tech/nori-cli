//! Per-connection lifecycle for the remote ACP transport.
//!
//! Enforces the RFD's initialize-first rule at the wire, then serves the
//! outward ACP Agent over the adapted frame streams. The Agent handlers call
//! the [`HostedAgent`] interface only — never the downstream `AcpConnection`
//! — so every remote mutation passes through Nori-owned harness policy.

use std::sync::Arc;
use std::sync::Mutex;

use agent_client_protocol::Agent;
use agent_client_protocol::Client;
use agent_client_protocol::ConnectionTo;
use agent_client_protocol::Responder;
use axum::extract::ws::CloseFrame;
use axum::extract::ws::Message;
use axum::extract::ws::WebSocket;
use futures::SinkExt;
use futures::StreamExt;
use nori_protocol::AcpEvent;
use nori_protocol::NoriEvent;
use nori_protocol::SessionEvent;
use nori_protocol::acp::v1 as acp;
use tokio_util::sync::CancellationToken;

use super::hosted_agent::HostedAgent;
use super::hosted_agent::HostedEventReceiver;
use super::wire;

/// WebSocket close code for a protocol error (RFC 6455), used when the first
/// JSON-RPC message on the socket is not an `initialize` request.
const CLOSE_PROTOCOL_ERROR: u16 = 1002;

/// Serve one accepted WebSocket connection until it disconnects, the hosted
/// session ends, or `cancel` fires (last-connect-wins replacement).
pub(super) async fn serve_connection<H: HostedAgent>(
    socket: WebSocket,
    hosted: Arc<H>,
    subscription: super::hosted_agent::HostedSubscription,
    cancel: CancellationToken,
) {
    let subscription_id = subscription.id;
    serve_gated_connection(socket, hosted.clone(), subscription.events, cancel).await;
    hosted.detach(subscription_id).await;
}

async fn serve_gated_connection<H: HostedAgent>(
    socket: WebSocket,
    hosted: Arc<H>,
    events: HostedEventReceiver,
    cancel: CancellationToken,
) {
    let (ws_sink, ws_stream) = socket.split();
    let mut ws_sink = ws_sink;
    let mut incoming = Box::pin(wire::incoming_lines(ws_stream, cancel.clone()));

    // RFD: `initialize` must be the first JSON-RPC message on the socket.
    // Unparseable frames get a JSON-RPC parse error and the gate keeps
    // waiting; the first valid message decides. The params are validated
    // here too, so a connection whose initialize the SDK would reject never
    // idles without a forward loop.
    let first_line = loop {
        let Some(line) = incoming.next().await else {
            return;
        };
        let Ok(line) = line else {
            return;
        };
        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(message) => {
                let is_initialize_request = message.get("method")
                    == Some(&serde_json::Value::String("initialize".to_owned()))
                    && message.get("id").is_some_and(|id| !id.is_null())
                    && message.get("params").is_some_and(|params| {
                        serde_json::from_value::<acp::InitializeRequest>(params.clone()).is_ok()
                    });
                if !is_initialize_request {
                    let close = CloseFrame {
                        code: CLOSE_PROTOCOL_ERROR,
                        reason: "First message must be a valid initialize request".into(),
                    };
                    let _ = ws_sink.send(Message::Close(Some(close))).await;
                    return;
                }
                break line;
            }
            Err(_) => {
                let error = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": "Parse error" },
                });
                if ws_sink
                    .send(Message::Text(error.to_string().into()))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
    };

    let events_cell: Arc<Mutex<Option<HostedEventReceiver>>> = Arc::new(Mutex::new(Some(events)));
    // Harness requests this connection issued, awaiting their outcome from
    // the event stream. The lock is held across submit-and-register so the
    // forward loop cannot observe a response before its responder exists.
    let pending_prompts: PendingPrompts = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let outgoing = wire::outgoing_lines(ws_sink, cancel.clone());
    let incoming = futures::stream::once(std::future::ready(Ok(first_line))).chain(incoming);
    let transport = agent_client_protocol::Lines::new(outgoing, incoming);

    let connection_result = Agent
        .builder()
        .name("nori-remote")
        .on_receive_request(
            {
                let hosted = hosted.clone();
                let cancel = cancel.clone();
                let events_cell = events_cell.clone();
                let pending_prompts = pending_prompts.clone();
                async move |request: acp::InitializeRequest,
                            responder: Responder<acp::InitializeResponse>,
                            cx: ConnectionTo<Client>| {
                    let events = events_cell.lock().ok().and_then(|mut cell| cell.take());
                    if let Some(events) = events {
                        let hosted = hosted.clone();
                        let cancel = cancel.clone();
                        let client = cx.clone();
                        let pending_prompts = pending_prompts.clone();
                        cx.spawn(async move {
                            forward_events(events, hosted, client, pending_prompts, cancel).await;
                            Ok(())
                        })?;
                    }
                    responder.respond(
                        acp::InitializeResponse::new(request.protocol_version)
                            .agent_capabilities(remote_capabilities())
                            .agent_info(
                                acp::Implementation::new("nori", env!("CARGO_PKG_VERSION"))
                                    .title("Nori CLI"),
                            ),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_request: acp::NewSessionRequest,
                        responder: Responder<acp::NewSessionResponse>,
                        _cx: ConnectionTo<Client>| {
                responder.respond_with_error(acp::Error::new(
                    -32600,
                    "This remote surface exposes the running Nori session; discover it with \
                     session/list and attach with session/load",
                ))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hosted = hosted.clone();
                async move |_request: acp::ListSessionsRequest,
                            responder: Responder<acp::ListSessionsResponse>,
                            _cx: ConnectionTo<Client>| {
                    match hosted.list_sessions().await {
                        Ok(sessions) => responder.respond(acp::ListSessionsResponse::new(sessions)),
                        Err(error) => responder.respond_with_error(error),
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hosted = hosted.clone();
                async move |request: acp::LoadSessionRequest,
                            responder: Responder<acp::LoadSessionResponse>,
                            cx: ConnectionTo<Client>| {
                    match hosted.load_session(&request.session_id).await {
                        Ok(loaded) => {
                            // Replay precedes the response in the outgoing
                            // queue, matching `session/load` semantics.
                            for notification in loaded.replay {
                                cx.send_notification(notification)?;
                            }
                            responder.respond(acp::LoadSessionResponse::new())
                        }
                        Err(error) => responder.respond_with_error(error),
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hosted = hosted.clone();
                async move |request: acp::ResumeSessionRequest,
                            responder: Responder<acp::ResumeSessionResponse>,
                            _cx: ConnectionTo<Client>| {
                    match hosted.resume_session(&request.session_id).await {
                        Ok(()) => responder.respond(acp::ResumeSessionResponse::new()),
                        Err(error) => responder.respond_with_error(error),
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hosted = hosted.clone();
                let pending_prompts = pending_prompts.clone();
                async move |request: acp::PromptRequest,
                            responder: Responder<acp::PromptResponse>,
                            cx: ConnectionTo<Client>| {
                    // Spawned: a prompt queued behind an active turn resolves
                    // only when it is issued, and other incoming messages
                    // (notably session/cancel) must keep dispatching.
                    let hosted = hosted.clone();
                    let pending_prompts = pending_prompts.clone();
                    cx.spawn(async move {
                        let mut pending = pending_prompts.lock().await;
                        match hosted
                            .prompt(&request.session_id, request.prompt, request.meta)
                            .await
                        {
                            Ok(request_id) => {
                                pending.push((request_id, responder));
                                Ok(())
                            }
                            Err(error) => responder.respond_with_error(error),
                        }
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let hosted = hosted.clone();
                async move |request: acp::CloseSessionRequest,
                            responder: Responder<acp::CloseSessionResponse>,
                            _cx: ConnectionTo<Client>| {
                    match hosted.close_session(&request.session_id).await {
                        Ok(()) => responder.respond(acp::CloseSessionResponse::new()),
                        Err(error) => responder.respond_with_error(error),
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let hosted = hosted.clone();
                async move |notification: acp::CancelNotification, _cx: ConnectionTo<Client>| {
                    hosted.cancel(&notification.session_id).await?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(transport)
        .await;

    if let Err(error) = connection_result {
        tracing::debug!("remote ACP connection ended: {error}");
    }
}

/// Capabilities the remote surface advertises. `loadSession` is required by
/// the spec: `session/load` is the recovery path after a reconnect.
fn remote_capabilities() -> acp::AgentCapabilities {
    acp::AgentCapabilities::new()
        .load_session(true)
        .session_capabilities(
            acp::SessionCapabilities::new()
                .list(acp::SessionListCapabilities::new())
                .resume(acp::SessionResumeCapabilities::new())
                .close(acp::SessionCloseCapabilities::new()),
        )
}

/// Harness requests issued by this connection, keyed by the harness-side
/// request id, each holding the remote client's own responder — the boundary
/// correlation from the spec.
type PendingPrompts =
    Arc<tokio::sync::Mutex<Vec<(acp::RequestId, Responder<acp::PromptResponse>)>>>;

/// Resolve the pending responder registered for `request_id`, if any.
async fn take_pending(
    pending_prompts: &PendingPrompts,
    request_id: &acp::RequestId,
) -> Option<Responder<acp::PromptResponse>> {
    let mut pending = pending_prompts.lock().await;
    let position = pending.iter().position(|(id, _)| id == request_id)?;
    Some(pending.remove(position).1)
}

/// Forward the hosted post-harness stream to the remote controller: pass
/// `session/update` notifications through, answer this connection's harness
/// requests in stream order, round-trip delegated permission requests, and
/// close the connection when the hosted session ends or the host drops this
/// consumer.
async fn forward_events<H: HostedAgent>(
    mut events: HostedEventReceiver,
    hosted: Arc<H>,
    client: ConnectionTo<Client>,
    pending_prompts: PendingPrompts,
    cancel: CancellationToken,
) {
    while let Some(event) = events.recv().await {
        match event {
            SessionEvent::Acp(AcpEvent::Notification(notification)) => {
                if let acp::AgentNotification::SessionNotification(notification) = notification
                    && client.send_notification(notification).is_err()
                {
                    break;
                }
            }
            SessionEvent::Acp(AcpEvent::Request {
                request_id,
                request,
            }) => match request {
                acp::AgentRequest::RequestPermissionRequest(request) => {
                    let response = client.send_request(request).block_task().await;
                    let answer = match response {
                        Ok(response) => response,
                        Err(_) => acp::RequestPermissionResponse::new(
                            acp::RequestPermissionOutcome::Cancelled,
                        ),
                    };
                    let _ = hosted
                        .respond(
                            request_id,
                            Ok(acp::ClientResponse::RequestPermissionResponse(answer)),
                        )
                        .await;
                }
                _ => {
                    let _ = hosted
                        .respond(request_id, Err(acp::Error::method_not_found()))
                        .await;
                }
            },
            SessionEvent::Acp(AcpEvent::Response {
                request_id,
                response,
            }) => {
                if let Some(responder) = take_pending(&pending_prompts, &request_id).await {
                    let _ = match response {
                        Ok(acp::AgentResponse::PromptResponse(response)) => {
                            responder.respond(response)
                        }
                        Ok(_) => responder.respond_with_error(acp::Error::internal_error()),
                        Err(error) => responder.respond_with_error(error),
                    };
                }
            }
            SessionEvent::Nori(NoriEvent::RequestFailed(failure)) => {
                if let Some(request_id) = &failure.request_id
                    && let Some(responder) = take_pending(&pending_prompts, request_id).await
                {
                    let _ = responder
                        .respond_with_error(acp::Error::new(-32000, failure.message.clone()));
                }
            }
            SessionEvent::Nori(NoriEvent::SessionEnded(ended)) => {
                let message = ended
                    .message
                    .unwrap_or_else(|| format!("session ended: {:?}", ended.reason));
                for (_, responder) in pending_prompts.lock().await.drain(..) {
                    let _ = responder.respond_with_error(acp::Error::new(-32000, message.clone()));
                }
                break;
            }
            SessionEvent::Nori(_) => {}
        }
    }
    cancel.cancel();
}
