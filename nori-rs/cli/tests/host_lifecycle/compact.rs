//! Compacting a session on the host: the controller asks for it, and every
//! terminal following that session sees it finish on that session's own
//! stream, whether the agent compacts natively or the host summarizes and
//! swaps.

use super::acp_client::Acp;
use super::acp_v1::controller;
use super::acp_v1::host_with;
use super::acp_v1::new_session;
use super::acp_v1::watcher;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

async fn compact(acp: &mut Acp, id: &str) -> Result<Value, super::acp_client::RpcError> {
    acp.call("_nori/session/compact", json!({"sessionId": id}))
        .await
}

/// Follow each of `ids`; `*` follows every session on the host.
async fn follow(acp: &mut Acp, ids: &[&str]) {
    for id in ids {
        acp.call("_nori/session/subscribe", json!({"session_id": id}))
            .await
            .unwrap();
    }
}

fn event_type(frame: &Value) -> &Value {
    &frame["params"]["event"]["event"]["event_type"]
}

fn is_event(frame: &Value) -> bool {
    frame["method"] == "_nori/session/event"
}

/// Wait until `id` finishes compacting, and return the session each
/// `context_compacted` seen on the way belonged to.
async fn compacted_sessions(acp: &mut Acp, id: &str) -> Vec<String> {
    let mut seen = Vec::new();
    loop {
        let frame = acp
            .until(|frame| is_event(frame) && event_type(frame) == "context_compacted")
            .await;
        let session = frame["params"]["conversation_id"]
            .as_str()
            .unwrap()
            .to_owned();
        let done = session == id;
        seen.push(session);
        if done {
            return seen;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_advertises_session_compaction() {
    let host = host_with("", &[]);
    let mut acp = controller(&host);
    let result = acp
        .call("initialize", json!({"protocolVersion": 1}))
        .await
        .unwrap();
    assert_eq!(
        result["capabilities"]["nori_sessions"]["compact"],
        json!(true),
        "{result}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_native_compaction_finishes_on_that_session_for_every_follower() {
    let host = host_with("", &[("MOCK_AGENT_ADVERTISE_COMPACT", "1")]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (first, _) = new_session(&mut acp, folder.path()).await;
    let (_background, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);
    follow(&mut watching, &["*"]).await;

    compact(&mut acp, &first).await.unwrap();

    assert_eq!(
        compacted_sessions(&mut watching, &first).await,
        vec![first.clone()]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_summarizing_compaction_keeps_the_session_followers_see() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (first, _) = new_session(&mut acp, folder.path()).await;
    let (second, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);
    follow(&mut watching, &["*"]).await;

    compact(&mut acp, &first).await.unwrap();
    let seen = compacted_sessions(&mut watching, &first).await;
    let listing = watching
        .call("_nori/session/list", json!({}))
        .await
        .unwrap();
    let mut listed: Vec<String> = listing["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["session_id"].as_str().unwrap().to_owned())
        .collect();
    listed.sort();
    let mut expected = vec![first.clone(), second.clone()];
    expected.sort();

    assert_eq!((seen, listed), (vec![first.clone()], expected));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_terminal_that_follows_later_sees_the_compaction_in_the_replay() {
    let host = host_with("", &[("MOCK_AGENT_ADVERTISE_COMPACT", "1")]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut first_follower = watcher(&host);
    follow(&mut first_follower, &[&id]).await;
    compact(&mut acp, &id).await.unwrap();
    compacted_sessions(&mut first_follower, &id).await;

    let mut later = watcher(&host);
    follow(&mut later, &[&id]).await;

    assert_eq!(compacted_sessions(&mut later, &id).await, vec![id.clone()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_cannot_compact() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);

    let refused = compact(&mut watching, &id).await.unwrap_err();

    assert_eq!(refused.code, -32016, "{refused:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_session_is_refused() {
    let host = host_with("", &[]);
    let mut acp = controller(&host);
    let unknown = nori_harness::ConversationId::new().to_string();

    let refused = compact(&mut acp, &unknown).await.unwrap_err();

    assert_eq!(refused.code, -32002, "{refused:?}");
}
