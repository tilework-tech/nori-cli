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
        let (socket, _) = tokio::time::timeout(Duration::from_secs(10), connect_async(request))
            .await
            .map_err(|_| anyhow::anyhow!("host connection timed out"))??;
        Ok(Self { socket, next_id: 1 })
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
