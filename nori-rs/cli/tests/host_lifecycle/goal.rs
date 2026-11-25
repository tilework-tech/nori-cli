//! Each session's goal on the host: any terminal reads it, sees it change
//! live, and finds it on the list row after joining late; only the
//! controller sets, pauses, resumes or clears it.

use super::acp_client::Acp;
use super::acp_v1::controller;
use super::acp_v1::host_with;
use super::acp_v1::new_session;
use super::acp_v1::watcher;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

/// A host whose mock agent drives goals through its `_session/goal`
/// extension, so goal management is available to every session.
fn goal_host() -> Host {
    host_with("", &[("MOCK_AGENT_GOAL_EXT", "1")])
}

async fn get(acp: &mut Acp, id: &str) -> Value {
    acp.call("_nori/session/goal/get", json!({"sessionId": id}))
        .await
        .unwrap()["goal"]
        .clone()
}

async fn set(acp: &mut Acp, id: &str, op: Value) -> Result<Value, super::acp_client::RpcError> {
    acp.call("_nori/session/goal/set", json!({"sessionId": id, "op": op}))
        .await
}

/// The objective and status of a goal, the parts a terminal shows.
fn shown(goal: &Value) -> Value {
    if goal.is_null() {
        return Value::Null;
    }
    json!({"objective": goal["objective"], "status": goal["status"]})
}

async fn row(acp: &mut Acp, id: &str) -> Value {
    let listing = acp.call("_nori/session/list", json!({})).await.unwrap();
    listing["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["session_id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("{id} is listed: {listing}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_advertises_session_goals() {
    let host = host_with("", &[]);
    let mut acp = controller(&host);
    let result = acp
        .call("initialize", json!({"protocolVersion": 1}))
        .await
        .unwrap();
    assert_eq!(
        result["capabilities"]["nori_sessions"]["goal"],
        json!(true),
        "{result}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn each_session_keeps_its_own_goal() {
    let host = goal_host();
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (first, _) = new_session(&mut acp, folder.path()).await;
    let (second, _) = new_session(&mut acp, folder.path()).await;

    let reply = set(
        &mut acp,
        &first,
        json!({"set": {"objective": "ship the lantern"}}),
    )
    .await
    .unwrap();

    assert_eq!(
        shown(&reply["goal"]),
        json!({"objective": "ship the lantern", "status": "active"})
    );
    assert_eq!(
        shown(&get(&mut acp, &first).await),
        json!({"objective": "ship the lantern", "status": "active"})
    );
    assert_eq!(get(&mut acp, &second).await, Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn pause_resume_and_clear_change_the_goal() {
    let host = goal_host();
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    set(
        &mut acp,
        &id,
        json!({"set": {"objective": "tidy the pantry"}}),
    )
    .await
    .unwrap();

    set(&mut acp, &id, json!("pause")).await.unwrap();
    let paused = shown(&get(&mut acp, &id).await);
    set(&mut acp, &id, json!("resume")).await.unwrap();
    let resumed = shown(&get(&mut acp, &id).await);
    set(&mut acp, &id, json!("clear")).await.unwrap();

    assert_eq!(
        paused,
        json!({"objective": "tidy the pantry", "status": "paused"})
    );
    assert_eq!(
        resumed,
        json!({"objective": "tidy the pantry", "status": "active"})
    );
    assert_eq!(get(&mut acp, &id).await, Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_reads_and_follows_the_goal_but_cannot_change_it() {
    let host = goal_host();
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);
    watching
        .call("_nori/session/subscribe", json!({"session_id": id}))
        .await
        .unwrap();

    set(
        &mut acp,
        &id,
        json!({"set": {"objective": "water the ferns"}}),
    )
    .await
    .unwrap();
    let live = watching
        .until(|frame| {
            frame["method"] == "_nori/session/event"
                && frame["params"]["event"]["event"]["event_type"] == "goal_changed"
        })
        .await;
    let refused = set(&mut watching, &id, json!("clear")).await.unwrap_err();

    assert_eq!(
        shown(&live["params"]["event"]["event"]["event"]),
        json!({"objective": "water the ferns", "status": "active"})
    );
    assert_eq!(
        shown(&get(&mut watching, &id).await),
        json!({"objective": "water the ferns", "status": "active"})
    );
    assert_eq!(refused.code, -32016, "{refused:?}");
    assert_eq!(
        shown(&get(&mut acp, &id).await),
        json!({"objective": "water the ferns", "status": "active"})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_terminal_that_connects_later_sees_the_goal_on_its_list_row() {
    let host = goal_host();
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    set(
        &mut acp,
        &id,
        json!({"set": {"objective": "label the jars"}}),
    )
    .await
    .unwrap();

    let mut later = watcher(&host);
    let listed = row(&mut later, &id).await;

    assert_eq!(
        shown(&listed["goal"]),
        json!({"objective": "label the jars", "status": "active"})
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_session_is_refused() {
    let host = goal_host();
    let mut acp = controller(&host);
    let unknown = nori_harness::ConversationId::new().to_string();

    let read = acp
        .call("_nori/session/goal/get", json!({"sessionId": unknown}))
        .await
        .unwrap_err();
    let write = set(&mut acp, &unknown, json!("clear")).await.unwrap_err();

    assert_eq!((read.code, write.code), (-32002, -32002));
}
