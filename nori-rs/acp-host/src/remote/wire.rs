//! WebSocket frame adapters for the remote ACP transport.
//!
//! The upstream RFD maps one JSON-RPC message onto one UTF-8 text frame, so
//! the socket is adapted into the SDK's [`agent_client_protocol::Lines`]
//! shape: a `Sink<String>` and a `Stream<io::Result<String>>` where each
//! `String` is exactly one message. Binary frames are ignored per the RFD;
//! ping/pong stays transport-level liveness with no ACP meaning (axum answers
//! incoming pings automatically).

use std::io;

use axum::extract::ws::Message;
use axum::extract::ws::WebSocket;
use futures::SinkExt;
use futures::Stream;
use futures::StreamExt;
use futures::stream::SplitSink;
use futures::stream::SplitStream;
use tokio_util::sync::CancellationToken;

/// Bounded outgoing frame queue. A consumer that stops reading fills this
/// queue and stalls the JSON-RPC writer, which in turn overflows the host's
/// bounded event queue — the overflow policy that closes slow consumers.
const OUTGOING_QUEUE_FRAMES: usize = 256;

/// Adapt the outgoing socket half into a line sink: one line, one text frame.
///
/// Frames pass through a bounded queue to a writer task that finishes with a
/// best-effort close handshake, so clients observe a clean WebSocket close
/// instead of a reset when the connection ends or is replaced.
pub(super) fn outgoing_lines(
    sink: SplitSink<WebSocket, Message>,
    cancel: CancellationToken,
) -> impl futures::Sink<String, Error = io::Error> + Send + 'static {
    let (tx, mut rx) = futures::channel::mpsc::channel::<String>(OUTGOING_QUEUE_FRAMES);
    tokio::spawn(async move {
        let mut sink = sink;
        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                line = rx.next() => match line {
                    Some(line) => {
                        if sink.send(Message::Text(line.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
        let _ = sink.send(Message::Close(None)).await;
    });
    tx.sink_map_err(io::Error::other)
}

/// Adapt the incoming socket half into a line stream: text frames pass
/// through, binary/ping/pong frames are skipped, and close/disconnect/forced
/// close surface as an error so the JSON-RPC connection tears down promptly.
pub(super) fn incoming_lines(
    stream: SplitStream<WebSocket>,
    cancel: CancellationToken,
) -> impl Stream<Item = io::Result<String>> + Send + 'static {
    futures::stream::unfold(
        (stream, cancel, false),
        |(mut stream, cancel, done)| async move {
            if done {
                return None;
            }
            loop {
                tokio::select! {
                    () = cancel.cancelled() => {
                        let error = io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "remote ACP connection closed by the server",
                        );
                        return Some((Err(error), (stream, cancel, true)));
                    }
                    frame = stream.next() => match frame {
                        Some(Ok(Message::Text(text))) => {
                            return Some((Ok(text.as_str().to_owned()), (stream, cancel, false)));
                        }
                        Some(Ok(Message::Binary(_) | Message::Ping(_) | Message::Pong(_))) => {}
                        Some(Ok(Message::Close(_))) | None => {
                            let error = io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "remote ACP client disconnected",
                            );
                            return Some((Err(error), (stream, cancel, true)));
                        }
                        Some(Err(error)) => {
                            return Some((Err(io::Error::other(error)), (stream, cancel, true)));
                        }
                    }
                }
            }
        },
    )
}
