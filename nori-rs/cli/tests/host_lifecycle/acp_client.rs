//! A test-side ACP v1 client over the host's authenticated `/sessions`
//! socket. It keeps every frame that is not the awaited response, so tests
//! can assert on the notifications and agent requests around each call.

use std::time::Duration;

use nori_harness::remote_agent::ManagedClient;
use serde_json::Value;
use serde_json::json;

pub(crate) struct Acp {
    client: ManagedClient,
    next_id: i64,
    /// Frames received while waiting, in arrival order, not yet taken.
    pub(crate) inbox: Vec<Value>,
    /// Every frame received on this connection, in arrival order.
    pub(crate) log: Vec<Value>,
}

/// A JSON-RPC error the host returned.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct RpcError {
    pub(crate) code: i64,
    pub(crate) message: String,
}

impl Acp {
    pub(crate) fn new(client: ManagedClient) -> Self {
        Self {
            client,
            next_id: 1000,
            inbox: Vec::new(),
            log: Vec::new(),
        }
    }

    pub(crate) async fn send(&mut self, message: Value) {
        self.client.send(message).await.unwrap();
    }

    /// Send a request and wait for its response. Frames that arrive before
    /// the response are kept in `inbox`.
    pub(crate) async fn call(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        let response = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                let frame = self.client.recv().await.unwrap();
                self.log.push(frame.clone());
                if frame.get("method").is_none() && frame.get("id") == Some(&json!(id)) {
                    return frame;
                }
                self.inbox.push(frame);
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{method} got no response; frames: {:?}", self.inbox));
        match response.get("error") {
            Some(error) => Err(RpcError {
                code: error["code"].as_i64().unwrap(),
                message: error["message"].as_str().unwrap_or_default().to_owned(),
            }),
            None => Ok(response["result"].clone()),
        }
    }

    /// Return the first frame matching `wanted`, from `inbox` or the socket,
    /// removing it and everything before it from `inbox`.
    pub(crate) async fn until(&mut self, wanted: impl Fn(&Value) -> bool) -> Value {
        if let Some(index) = self.inbox.iter().position(&wanted) {
            return self.inbox.drain(..=index).next_back().unwrap();
        }
        self.inbox.clear();
        tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                let frame = self.client.recv().await.unwrap();
                self.log.push(frame.clone());
                if wanted(&frame) {
                    return frame;
                }
            }
        })
        .await
        .unwrap_or_else(|_| {
            let recent = &self.log[self.log.len().saturating_sub(8)..];
            panic!("the expected frame never arrived; last frames: {recent:#?}")
        })
    }

    /// Collect frames for `duration` without waiting for any one of them.
    pub(crate) async fn collect_for(&mut self, duration: Duration) -> Vec<Value> {
        let mut frames = std::mem::take(&mut self.inbox);
        let _ = tokio::time::timeout(duration, async {
            loop {
                let frame = self.client.recv().await.unwrap();
                self.log.push(frame.clone());
                frames.push(frame);
            }
        })
        .await;
        frames
    }
}

impl Acp {
    /// Start a turn through the legacy alias, retrying while an earlier
    /// turn is still finishing.
    pub(crate) async fn prompt_alias(&mut self, session: &str, text: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        loop {
            match self
                .call(
                    "_nori/session/prompt",
                    json!({"session_id": session, "prompt": [{"type":"text","text":text}]}),
                )
                .await
            {
                Ok(_) => return,
                Err(error) if error.code == -32015 && std::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                Err(error) => panic!("prompt was refused: {error:?}"),
            }
        }
    }
}

/// `session/update` notifications for `session` among `frames`.
pub(crate) fn updates<'a>(frames: &'a [Value], session: &str) -> Vec<&'a Value> {
    frames
        .iter()
        .filter(|frame| {
            frame["method"] == "session/update" && frame["params"]["sessionId"] == session
        })
        .collect()
}

/// Whether any `session/update` for `session` among `frames` mentions `text`.
pub(crate) fn mentions(frames: &[Value], session: &str, text: &str) -> bool {
    updates(frames, session)
        .iter()
        .any(|frame| frame["params"]["update"].to_string().contains(text))
}

pub(crate) fn is_update_mentioning(session: &str, text: &str) -> impl Fn(&Value) -> bool {
    let session = session.to_owned();
    let text = text.to_owned();
    move |frame| {
        frame["method"] == "session/update"
            && frame["params"]["sessionId"] == session.as_str()
            && frame["params"]["update"].to_string().contains(&text)
    }
}
