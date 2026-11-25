//! A subscription's live events while the host is still writing its replay.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use futures::SinkExt;
use futures::StreamExt;
use nori_acp_host::remote::ManagedAgent;
use nori_acp_host::remote::ManagedReply;
use nori_acp_host::remote::ManagedSubscription;
use nori_acp_host::remote::RemoteAcpServer;
use nori_protocol::NoriEvent;
use nori_protocol::Notice;
use nori_protocol::SessionEnvelope;
use nori_protocol::SessionEvent;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// The live queue a host subscriber gets.
const LIVE_QUEUE: i64 = 256;
/// Replay frames large enough that the socket cannot buffer them all.
const REPLAY: i64 = 128;
const REPLAY_TEXT_BYTES: usize = 256 * 1024;
/// Live events published after the queue was already full.
const LATER_LIVE: i64 = 200;

fn envelope(seq: i64, text: String) -> SessionEnvelope {
    SessionEnvelope {
        conversation_id: "replayed".into(),
        agent: "synthetic".into(),
        seq,
        event: SessionEvent::Nori(NoriEvent::Notice(Notice { message: text })),
    }
}

/// Replies to `subscribe` with a long replay, a full live queue, and a
/// publisher that drops the subscriber on a full queue, as the journal does.
#[derive(Default)]
struct ReplayingHost {
    publisher: Mutex<Option<oneshot::Receiver<()>>>,
    published: Mutex<Option<oneshot::Sender<Result<(), String>>>>,
}

impl ManagedAgent for ReplayingHost {
    async fn request(&self, request: Value) -> ManagedReply {
        let mut subscription = None;
        if request["method"] == "subscribe" {
            let (tx, events) = mpsc::channel(LIVE_QUEUE as usize);
            for seq in REPLAY + 1..=REPLAY + LIVE_QUEUE {
                tx.try_send(envelope(seq, String::new())).unwrap();
            }
            let start = self.publisher.lock().unwrap().take().unwrap();
            let done = self.published.lock().unwrap().take().unwrap();
            tokio::spawn(async move {
                start.await.unwrap();
                let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
                while tx.capacity() < LATER_LIVE as usize {
                    if tokio::time::Instant::now() >= deadline {
                        drop(tx);
                        done.send(Err("the live queue stayed full during the replay".into()))
                            .unwrap();
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
                for seq in REPLAY + LIVE_QUEUE + 1..=REPLAY + LIVE_QUEUE + LATER_LIVE {
                    tx.try_send(envelope(seq, String::new())).unwrap();
                }
                done.send(Ok(())).unwrap();
                std::future::pending::<()>().await;
            });
            subscription = Some(ManagedSubscription {
                replay: (1..=REPLAY)
                    .map(|seq| envelope(seq, "r".repeat(REPLAY_TEXT_BYTES)))
                    .collect(),
                events,
            });
        }
        ManagedReply {
            delivery_guard: None,
            response: json!({"jsonrpc":"2.0","id":request["id"],"result":{}}),
            subscription,
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn live_events_published_during_a_long_replay_follow_it_once_in_order() {
    let (start, publisher) = oneshot::channel();
    let (published, done) = oneshot::channel();
    let host = Arc::new(ReplayingHost {
        publisher: Mutex::new(Some(publisher)),
        published: Mutex::new(Some(published)),
    });
    let server = RemoteAcpServer::bind_host("127.0.0.1:0".parse().unwrap(), host, "secret".into())
        .await
        .unwrap();
    let mut request = format!("ws://{}/sessions", server.local_addr())
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("Authorization", "Bearer secret".parse().unwrap());
    let mut socket = connect_async(request).await.unwrap().0;
    for (id, method) in [(1, "initialize"), (2, "subscribe")] {
        socket
            .send(Message::Text(
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":{}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
    }
    let mut next = async || -> Option<Value> {
        loop {
            match tokio::time::timeout(Duration::from_secs(10), socket.next())
                .await
                .unwrap()?
                .unwrap()
            {
                Message::Text(text) => return Some(serde_json::from_str(&text).unwrap()),
                Message::Close(_) => return None,
                _ => {}
            }
        }
    };
    let mut seqs = Vec::new();
    // Hold the rest of the replay unread while more live events arrive.
    while seqs.is_empty() {
        let frame = next().await.unwrap();
        if frame["method"] == "_nori/session/event" {
            seqs.push(frame["params"]["seq"].as_i64().unwrap());
        }
    }
    start.send(()).unwrap();
    assert_eq!(done.await.unwrap(), Ok(()));
    let total = REPLAY + LIVE_QUEUE + LATER_LIVE;
    while seqs.len() < total as usize {
        let Some(frame) = next().await else { break };
        if frame["method"] == "_nori/session/event" {
            seqs.push(frame["params"]["seq"].as_i64().unwrap());
        }
    }

    assert_eq!(seqs, (1..=total).collect::<Vec<_>>());
}
