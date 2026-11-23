//! Client connection to an authenticated managed host.

use std::time::Duration;

use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// A managed WebSocket connection. Streaming frontends use `send` and `recv`
/// to correlate replies while processing every event notification.
pub struct ManagedClient {
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
    next_id: i64,
    host_instance: Option<uuid::Uuid>,
}

impl ManagedClient {
    /// Authenticate the WebSocket upgrade. The caller sends initialization.
    pub async fn connect(url: &str, token: &str) -> anyhow::Result<Self> {
        anyhow::ensure!(!token.trim().is_empty(), "host token must not be empty");
        let mut request = url.into_client_request()?;
        let mut authorization = format!("Bearer {token}")
            .parse::<tokio_tungstenite::tungstenite::http::HeaderValue>()?;
        authorization.set_sensitive(true);
        request.headers_mut().insert("Authorization", authorization);
        let (socket, response) =
            match tokio::time::timeout(Duration::from_secs(10), connect_async(request))
                .await
                .map_err(|_| anyhow::anyhow!("host connection timed out"))?
            {
                Ok(connected) => connected,
                Err(tokio_tungstenite::tungstenite::Error::Http(refusal)) => {
                    let status = refusal.status();
                    let reason = refusal
                        .body()
                        .as_deref()
                        .map(String::from_utf8_lossy)
                        .unwrap_or_default()
                        .trim()
                        .to_owned();
                    return Err(
                        anyhow::Error::from(tokio_tungstenite::tungstenite::Error::Http(refusal))
                            .context(format!(
                                "the host refused the connection ({status}): {reason}"
                            )),
                    );
                }
                Err(error) => return Err(error.into()),
            };
        let host_instance = response
            .headers()
            .get("x-nori-host-instance")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok());
        Ok(Self {
            socket,
            next_id: 1,
            host_instance,
        })
    }

    /// Continuity metadata from the authenticated upgrade, independent of credentials.
    pub fn host_instance(&self) -> Option<uuid::Uuid> {
        self.host_instance
    }

    /// Authentication rejection is terminal unless local credentials can be refreshed.
    pub fn is_authentication_error(error: &anyhow::Error) -> bool {
        matches!(error.downcast_ref::<tokio_tungstenite::tungstenite::Error>(),
            Some(tokio_tungstenite::tungstenite::Error::Http(response))
                if matches!(response.status().as_u16(), 401 | 403))
    }

    pub async fn send(&mut self, message: Value) -> anyhow::Result<()> {
        tokio::time::timeout(
            Duration::from_secs(30),
            self.socket.send(Message::Text(message.to_string().into())),
        )
        .await
        .map_err(|_| anyhow::anyhow!("host write timed out"))??;
        Ok(())
    }

    /// Send one text frame as is, for relays that must not reinterpret it.
    pub async fn send_text(&mut self, text: String) -> anyhow::Result<()> {
        tokio::time::timeout(
            Duration::from_secs(30),
            self.socket.send(Message::Text(text.into())),
        )
        .await
        .map_err(|_| anyhow::anyhow!("host write timed out"))??;
        Ok(())
    }

    /// The next text frame as is, or `None` once the host closes the
    /// connection.
    pub async fn recv_text(&mut self) -> anyhow::Result<Option<String>> {
        loop {
            let Some(message) = self.socket.next().await else {
                return Ok(None);
            };
            match message? {
                Message::Text(text) => return Ok(Some(text.as_str().to_owned())),
                Message::Ping(bytes) => self.socket.send(Message::Pong(bytes)).await?,
                Message::Pong(_) | Message::Frame(_) | Message::Binary(_) => {}
                Message::Close(_) => return Ok(None),
            }
        }
    }

    /// Close the connection, telling the host this client is leaving.
    pub async fn close(mut self) {
        let _ = self.socket.close(None).await;
    }

    pub async fn recv(&mut self) -> anyhow::Result<Value> {
        loop {
            let message = self
                .socket
                .next()
                .await
                .ok_or_else(|| anyhow::anyhow!("host connection closed"))??;
            match message {
                Message::Text(text) => return Ok(serde_json::from_str(&text)?),
                Message::Ping(bytes) => self.socket.send(Message::Pong(bytes)).await?,
                Message::Pong(_) | Message::Frame(_) => {}
                Message::Close(_) => anyhow::bail!("host connection closed"),
                Message::Binary(_) => anyhow::bail!("host sent a binary frame instead of JSON"),
            }
        }
    }

    /// Execute one sequential request, ignoring intervening notifications.
    /// Use `send`/`recv` instead when notifications must be retained.
    pub async fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = self.next_id;
        self.next_id = id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("host request IDs exhausted"))?;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await?;
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let response = self.recv().await?;
                if response.get("id") != Some(&json!(id)) {
                    continue;
                }
                if let Some(error) = response.get("error") {
                    anyhow::bail!(
                        "host error {}: {}",
                        error["code"],
                        error["message"].as_str().unwrap_or("request failed")
                    );
                }
                return response
                    .get("result")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("host response has no result"));
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("host request timed out"))?
    }
}
