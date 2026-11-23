//! Shared WebSocket frame helpers for the remote ACP transport.

use axum::extract::ws::Message;
use futures::SinkExt;
pub(super) async fn send_frame<S>(sink: &mut S, frame: Message) -> anyhow::Result<()>
where
    S: futures::Sink<Message> + Unpin,
    S::Error: std::error::Error + Send + Sync + 'static,
{
    tokio::time::timeout(std::time::Duration::from_secs(30), sink.send(frame))
        .await
        .map_err(|_| anyhow::anyhow!("managed remote consumer stalled"))??;
    Ok(())
}
