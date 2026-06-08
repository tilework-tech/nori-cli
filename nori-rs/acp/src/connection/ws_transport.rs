use std::io;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use anyhow::Context as _;
use anyhow::Result;
use futures::Sink;
use futures::Stream;
use futures::StreamExt;
use futures::stream::SplitSink;
use futures::stream::SplitStream;
use tokio_tungstenite::MaybeTlsStream;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

pub struct WsReadStream<S> {
    inner: S,
    closed: bool,
}

impl<S> WsReadStream<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            closed: false,
        }
    }
}

impl<S> Stream for WsReadStream<S>
where
    S: Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    type Item = io::Result<String>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.closed {
            return Poll::Ready(None);
        }
        loop {
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Ready(Some(Ok(Message::Text(text)))) => {
                    return Poll::Ready(Some(Ok(text.to_string())));
                }
                Poll::Ready(Some(Ok(Message::Binary(data)))) => {
                    return Poll::Ready(Some(
                        String::from_utf8(data.to_vec())
                            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
                    ));
                }
                Poll::Ready(Some(Ok(Message::Close(_)))) => {
                    this.closed = true;
                    return Poll::Ready(None);
                }
                Poll::Ready(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {
                    continue;
                }
                Poll::Ready(Some(Ok(Message::Frame(_)))) => {
                    continue;
                }
                Poll::Ready(Some(Err(e))) => {
                    return Poll::Ready(Some(Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        e,
                    ))));
                }
            }
        }
    }
}

pub struct WsSink<S> {
    inner: S,
}

impl<S> WsSink<S> {
    pub fn new(inner: S) -> Self {
        Self { inner }
    }
}

fn ws_err_to_io(e: tokio_tungstenite::tungstenite::Error) -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, e)
}

impl<S> Sink<String> for WsSink<S>
where
    S: Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    type Error = io::Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner)
            .poll_ready(cx)
            .map_err(ws_err_to_io)
    }

    fn start_send(self: Pin<&mut Self>, item: String) -> Result<(), Self::Error> {
        Pin::new(&mut self.get_mut().inner)
            .start_send(Message::Text(item.into()))
            .map_err(ws_err_to_io)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner)
            .poll_flush(cx)
            .map_err(ws_err_to_io)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.get_mut().inner)
            .poll_close(cx)
            .map_err(ws_err_to_io)
    }
}

pub async fn connect_ws(
    url: &str,
    auth_token: &str,
) -> Result<(
    WsSink<SplitSink<WsStream, Message>>,
    WsReadStream<SplitStream<WsStream>>,
)> {
    let mut request = url
        .into_client_request()
        .context("Failed to build WebSocket request")?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {auth_token}")
            .parse()
            .context("Failed to parse auth token as header value")?,
    );

    let (ws_stream, _response) = tokio_tungstenite::connect_async(request)
        .await
        .context("WebSocket connection failed")?;

    let (write_half, read_half) = ws_stream.split();

    Ok((WsSink::new(write_half), WsReadStream::new(read_half)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::SinkExt;
    use futures::stream;
    use pretty_assertions::assert_eq;
    use tokio_tungstenite::tungstenite::Error as WsError;

    fn text_msg(s: &str) -> Result<Message, WsError> {
        Ok(Message::Text(s.to_string().into()))
    }

    fn close_msg() -> Result<Message, WsError> {
        Ok(Message::Close(None))
    }

    fn ping_msg() -> Result<Message, WsError> {
        Ok(Message::Ping(vec![].into()))
    }

    fn pong_msg() -> Result<Message, WsError> {
        Ok(Message::Pong(vec![].into()))
    }

    fn ws_error() -> Result<Message, WsError> {
        Err(WsError::ConnectionClosed)
    }

    #[tokio::test]
    async fn read_stream_yields_text_messages_as_strings() {
        let messages = stream::iter(vec![text_msg("hello"), text_msg("world")]);
        let mut reader = WsReadStream::new(messages);

        let first = reader.next().await.expect("should have first item");
        assert_eq!(first.expect("should be ok"), "hello");

        let second = reader.next().await.expect("should have second item");
        assert_eq!(second.expect("should be ok"), "world");

        assert!(reader.next().await.is_none(), "stream should end");
    }

    #[tokio::test]
    async fn read_stream_ends_on_close_frame() {
        let messages = stream::iter(vec![text_msg("before"), close_msg(), text_msg("after")]);
        let mut reader = WsReadStream::new(messages);

        let first = reader.next().await.expect("should have first item");
        assert_eq!(first.expect("should be ok"), "before");

        assert!(
            reader.next().await.is_none(),
            "close frame should end stream"
        );
    }

    #[tokio::test]
    async fn read_stream_skips_ping_pong() {
        let messages = stream::iter(vec![
            ping_msg(),
            text_msg("data"),
            pong_msg(),
            text_msg("more"),
        ]);
        let mut reader = WsReadStream::new(messages);

        let first = reader.next().await.expect("should have item");
        assert_eq!(first.expect("should be ok"), "data");

        let second = reader.next().await.expect("should have item");
        assert_eq!(second.expect("should be ok"), "more");

        assert!(reader.next().await.is_none());
    }

    #[tokio::test]
    async fn read_stream_converts_ws_error_to_io_error() {
        let messages = stream::iter(vec![ws_error()]);
        let mut reader = WsReadStream::new(messages);

        let result = reader.next().await.expect("should have item");
        assert!(result.is_err(), "should be an error");
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::ConnectionAborted);
    }

    #[tokio::test]
    async fn read_stream_yields_valid_utf8_binary_as_string() {
        let messages = stream::iter(vec![Ok(Message::Binary(b"binary data".to_vec().into()))]);
        let mut reader = WsReadStream::new(messages);

        let result = reader.next().await.expect("should have item");
        assert_eq!(result.expect("should be ok"), "binary data");
    }

    #[tokio::test]
    async fn read_stream_returns_error_for_invalid_utf8_binary() {
        let messages = stream::iter(vec![Ok(Message::Binary(vec![0xFF, 0xFE].into()))]);
        let mut reader = WsReadStream::new(messages);

        let result = reader.next().await.expect("should have item");
        let err = result.expect_err("should be an error");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn sink_sends_string_as_text_message() {
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<Message>();
        let sink = tx.sink_map_err(|e| WsError::Io(io::Error::other(e)));
        let mut ws_sink = WsSink::new(sink);

        ws_sink
            .send("hello".to_string())
            .await
            .expect("send should succeed");

        let msg = rx.next().await.expect("should receive message");
        match msg {
            Message::Text(text) => assert_eq!(text.to_string(), "hello"),
            other => panic!("expected Text message, got {other:?}"),
        }
    }
}
