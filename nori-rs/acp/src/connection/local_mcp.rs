use std::collections::HashMap;
use std::sync::Arc;

use futures::SinkExt;
use futures::StreamExt;
use futures::channel::mpsc;
use sacp::Agent;
use sacp::Channel;
use sacp::ConnectTo;
use sacp::ConnectionTo;
use sacp::Dispatch;
use sacp::DynConnectTo;
use sacp::HandleDispatchFrom;
use sacp::Handled;
use sacp::Responder;
use sacp::Role;
use sacp::UntypedMessage;
use sacp::role;
use sacp::role::HasPeer;
use sacp::schema::McpConnectRequest;
use sacp::schema::McpConnectResponse;
use sacp::schema::McpDisconnectNotification;
use sacp::schema::McpOverAcpMessage;
use sacp::util::MatchDispatchFrom;

pub(crate) trait LocalMcpServer<Counterpart: Role>: Send + Sync + 'static {
    fn name(&self) -> String;

    fn connect(
        &self,
        acp_url: String,
        connection: ConnectionTo<Counterpart>,
    ) -> DynConnectTo<role::mcp::Client>;
}

pub(super) struct LocalMcpSession<Counterpart: Role> {
    acp_url: String,
    mcp_connect: Arc<dyn LocalMcpServer<Counterpart>>,
    connections: HashMap<String, mpsc::Sender<Dispatch>>,
}

impl<Counterpart: Role> LocalMcpSession<Counterpart>
where
    Counterpart: HasPeer<Agent>,
{
    pub(super) fn new(acp_url: String, mcp_connect: Arc<dyn LocalMcpServer<Counterpart>>) -> Self {
        Self {
            acp_url,
            mcp_connect,
            connections: HashMap::new(),
        }
    }

    async fn handle_connect_request(
        &mut self,
        request: McpConnectRequest,
        responder: Responder<McpConnectResponse>,
        acp_connection: &ConnectionTo<Counterpart>,
    ) -> Result<Handled<(McpConnectRequest, Responder<McpConnectResponse>)>, sacp::Error> {
        if request.acp_url != self.acp_url {
            return Ok(Handled::No {
                message: (request, responder),
                retry: false,
            });
        }

        let connection_id = format!("mcp-over-acp-connection:{}", uuid::Uuid::new_v4());
        let (mcp_server_tx, mut mcp_server_rx) = mpsc::channel(128);
        self.connections
            .insert(connection_id.clone(), mcp_server_tx);

        let (client_channel, server_channel) = Channel::duplex();
        let client_component = {
            let connection_id = connection_id.clone();
            let acp_connection = acp_connection.clone();

            role::mcp::Client
                .builder()
                .on_receive_dispatch(
                    async move |message: Dispatch, _mcp_connection| {
                        let wrapped = message.map(
                            |request, responder| {
                                (
                                    McpOverAcpMessage {
                                        connection_id: connection_id.clone(),
                                        message: request,
                                        meta: None,
                                    },
                                    responder,
                                )
                            },
                            |notification| McpOverAcpMessage {
                                connection_id: connection_id.clone(),
                                message: notification,
                                meta: None,
                            },
                        );
                        acp_connection.send_proxied_message_to(Agent, wrapped)
                    },
                    sacp::on_receive_dispatch!(),
                )
                .with_spawned(move |mcp_connection| async move {
                    while let Some(msg) = mcp_server_rx.next().await {
                        mcp_connection.send_proxied_message_to(role::mcp::Server, msg)?;
                    }
                    Ok(())
                })
        };

        let spawned_server = self
            .mcp_connect
            .connect(request.acp_url, acp_connection.clone());

        let spawn_results = acp_connection
            .spawn(async move { client_component.connect_to(client_channel).await })
            .and_then(|()| {
                acp_connection.spawn(async move { spawned_server.connect_to(server_channel).await })
            });

        match spawn_results {
            Ok(()) => {
                responder.respond(McpConnectResponse {
                    connection_id,
                    meta: None,
                })?;
                Ok(Handled::Yes)
            }
            Err(err) => {
                responder.respond_with_error(err)?;
                Ok(Handled::Yes)
            }
        }
    }

    async fn handle_mcp_over_acp_request(
        &mut self,
        request: McpOverAcpMessage<UntypedMessage>,
        responder: Responder<serde_json::Value>,
    ) -> Result<
        Handled<(
            McpOverAcpMessage<UntypedMessage>,
            Responder<serde_json::Value>,
        )>,
        sacp::Error,
    > {
        let Some(mcp_server_tx) = self.connections.get_mut(&request.connection_id) else {
            return Ok(Handled::No {
                message: (request, responder),
                retry: false,
            });
        };

        mcp_server_tx
            .send(Dispatch::Request(request.message, responder))
            .await
            .map_err(sacp::Error::into_internal_error)?;

        Ok(Handled::Yes)
    }

    async fn handle_mcp_over_acp_notification(
        &mut self,
        notification: McpOverAcpMessage<UntypedMessage>,
    ) -> Result<Handled<McpOverAcpMessage<UntypedMessage>>, sacp::Error> {
        let Some(mcp_server_tx) = self.connections.get_mut(&notification.connection_id) else {
            return Ok(Handled::No {
                message: notification,
                retry: false,
            });
        };

        mcp_server_tx
            .send(Dispatch::Notification(notification.message))
            .await
            .map_err(sacp::Error::into_internal_error)?;

        Ok(Handled::Yes)
    }

    async fn handle_mcp_disconnect_notification(
        &mut self,
        notification: McpDisconnectNotification,
    ) -> Result<Handled<McpDisconnectNotification>, sacp::Error> {
        if self
            .connections
            .remove(&notification.connection_id)
            .is_some()
        {
            Ok(Handled::Yes)
        } else {
            Ok(Handled::No {
                message: notification,
                retry: false,
            })
        }
    }
}

impl<Counterpart: Role> HandleDispatchFrom<Counterpart> for LocalMcpSession<Counterpart>
where
    Counterpart: HasPeer<Agent>,
{
    async fn handle_dispatch_from(
        &mut self,
        message: Dispatch,
        connection: ConnectionTo<Counterpart>,
    ) -> Result<Handled<Dispatch>, sacp::Error> {
        MatchDispatchFrom::new(message, &connection)
            .if_request_from(Agent, async |request, responder| {
                self.handle_connect_request(request, responder, &connection)
                    .await
            })
            .await
            .if_request_from(Agent, async |request, responder| {
                self.handle_mcp_over_acp_request(request, responder).await
            })
            .await
            .if_notification_from(Agent, async |notification| {
                self.handle_mcp_over_acp_notification(notification).await
            })
            .await
            .if_notification_from(Agent, async |notification| {
                self.handle_mcp_disconnect_notification(notification).await
            })
            .await
            .done()
    }

    fn describe_chain(&self) -> impl std::fmt::Debug {
        format!("LocalMcpSession({})", self.mcp_connect.name())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use sacp::Client;
    use sacp::JsonRpcResponse;
    use sacp::SentRequest;
    use serde_json::json;
    use tokio::sync::oneshot;

    use super::*;

    async fn recv<T: JsonRpcResponse + Send>(response: SentRequest<T>) -> Result<T, sacp::Error> {
        let (tx, rx) = oneshot::channel();
        response.on_receiving_result(async move |result| {
            tx.send(result).map_err(|_| sacp::Error::internal_error())
        })?;
        rx.await.map_err(|_| sacp::Error::internal_error())?
    }

    struct EchoMcpServer;

    impl LocalMcpServer<Agent> for EchoMcpServer {
        fn name(&self) -> String {
            "echo".to_string()
        }

        fn connect(
            &self,
            _acp_url: String,
            _connection: ConnectionTo<Agent>,
        ) -> DynConnectTo<role::mcp::Client> {
            DynConnectTo::new(EchoMcpComponent)
        }
    }

    struct EchoMcpComponent;

    impl ConnectTo<role::mcp::Client> for EchoMcpComponent {
        async fn connect_to(
            self,
            client: impl ConnectTo<role::mcp::Server>,
        ) -> Result<(), sacp::Error> {
            role::mcp::Server
                .builder()
                .on_receive_dispatch(
                    async move |message: Dispatch, _connection| {
                        if let Dispatch::Request(request, responder) = message {
                            responder.respond(json!({
                                "method": request.method,
                                "params": request.params,
                            }))?;
                        }
                        Ok(())
                    },
                    sacp::on_receive_dispatch!(),
                )
                .connect_to(client)
                .await
        }
    }

    #[tokio::test]
    async fn local_mcp_session_routes_connect_and_message_requests() {
        let acp_url = "acp:test-local-mcp".to_string();
        let (client_transport, agent_transport) = Channel::duplex();
        let (ready_tx, ready_rx) = oneshot::channel();
        let client_acp_url = acp_url.clone();

        let client_task = tokio::spawn(async move {
            Client
                .builder()
                .connect_with(client_transport, async move |connection| {
                    let registration = connection.add_dynamic_handler(LocalMcpSession::new(
                        client_acp_url,
                        Arc::new(EchoMcpServer),
                    ))?;
                    ready_tx
                        .send(())
                        .map_err(|_| sacp::Error::internal_error())?;
                    let _registration = registration;
                    futures::future::pending::<Result<(), sacp::Error>>().await
                })
                .await
        });

        let agent_result = Agent
            .builder()
            .connect_with(agent_transport, async move |connection| {
                ready_rx.await.map_err(|_| sacp::Error::internal_error())?;
                let connect_response = recv(connection.send_request_to(
                    Client,
                    McpConnectRequest {
                        acp_url,
                        meta: None,
                    },
                ))
                .await?;
                let response = recv(connection.send_request_to(
                    Client,
                    McpOverAcpMessage {
                        connection_id: connect_response.connection_id.clone(),
                        message: UntypedMessage::new("tools/list", json!({ "cursor": null }))?,
                        meta: None,
                    },
                ))
                .await?;

                assert_eq!(response["method"], "tools/list");
                assert_eq!(response["params"], json!({ "cursor": null }));

                connection.send_notification_to(
                    Client,
                    McpDisconnectNotification {
                        connection_id: connect_response.connection_id,
                        meta: None,
                    },
                )?;

                Ok(())
            })
            .await;

        client_task.abort();
        assert!(agent_result.is_ok(), "agent side failed: {agent_result:?}");
    }
}
