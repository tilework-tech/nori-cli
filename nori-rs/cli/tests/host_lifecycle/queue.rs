//! A paused queue on the host: any terminal sees it on its list row, the
//! controller resumes it or discards it, and watchers cannot change it.
//! Prompts queued by another client stay queued when taken; the composer
//! that queued a prompt takes it back (harness `queue_take` test).

use std::path::Path;
use std::time::Duration;

use super::acp_client::Acp;
use super::acp_client::is_update_mentioning;
use super::acp_v1::controller;
use super::acp_v1::host_with;
use super::acp_v1::new_session;
use super::acp_v1::text;
use super::acp_v1::watcher;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

/// A host whose mock agent logs the last text block of every prompt it
/// receives, and the folder holding that log.
fn logging_host() -> (Host, TempDir) {
    let logs = TempDir::new().unwrap();
    let log = logs.path().join("prompts.log");
    let host = host_with("", &[("MOCK_AGENT_PROMPT_LOG", log.to_str().unwrap())]);
    (host, logs)
}

fn prompt_log(logs: &TempDir) -> Vec<String> {
    std::fs::read_to_string(logs.path().join("prompts.log"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str::<String>(line).unwrap())
        .collect()
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

/// Start a turn that runs until cancelled, queue `queued` behind it, then
/// cancel it so the queue pauses. Returns the session ID.
async fn paused_with(acp: &mut Acp, folder: &Path, queued: &[&str]) -> String {
    let (id, _) = new_session(acp, folder).await;
    let started = acp
        .call(
            "_nori/session/prompt",
            json!({"session_id": id, "prompt": text("mock:stream-until-cancel")}),
        )
        .await
        .unwrap();
    acp.until(is_update_mentioning(&id, "agent_message_chunk"))
        .await;
    for (index, prompt) in queued.iter().enumerate() {
        let receipt = acp
            .call(
                "_nori/session/follow_up",
                json!({"session_id": id, "expected_turn": started["request_id"],
                    "message_id": format!("queued-{index}"), "intent": "queue",
                    "prompt": text(prompt)}),
            )
            .await
            .unwrap();
        assert_eq!(receipt["status"], "queued", "{receipt}");
    }
    acp.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":id}}))
        .await;
    wait_for_queue(acp, &id, |queue| queue["paused"] == "interrupted").await;
    id
}

async fn wait_for_queue(acp: &mut Acp, id: &str, done: impl Fn(&Value) -> bool) -> Value {
    for _ in 0..200 {
        let queue = row(acp, id).await["queue"].clone();
        if done(&queue) {
            return queue;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("queue never settled: {}", row(acp, id).await);
}

fn texts(queue: &Value) -> Vec<String> {
    queue["entries"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| entry["text"].as_str().unwrap().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn entry_id(queue: &Value, wanted: &str) -> Value {
    queue["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["text"] == wanted)
        .map(|entry| entry["id"].clone())
        .unwrap_or_else(|| panic!("{wanted} is queued: {queue}"))
}

async fn wait_for_log(logs: &TempDir, wanted: usize) -> Vec<String> {
    for _ in 0..200 {
        let log = prompt_log(logs);
        if log.len() >= wanted {
            return log;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    prompt_log(logs)
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_advertises_queue_control() {
    let host = host_with("", &[]);
    let mut acp = controller(&host);
    let result = acp
        .call("initialize", json!({"protocolVersion": 1}))
        .await
        .unwrap();
    assert_eq!(
        result["capabilities"]["nori_sessions"]["queue"],
        json!(true),
        "{result}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_terminal_that_connects_later_sees_the_paused_queue() {
    let (host, _logs) = logging_host();
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = paused_with(
        &mut acp,
        folder.path(),
        &["first follow-up", "second follow-up"],
    )
    .await;

    let mut later = watcher(&host);
    let queue = row(&mut later, &id).await["queue"].clone();

    assert_eq!(queue["paused"], "interrupted", "{queue}");
    assert_eq!(texts(&queue), vec!["first follow-up", "second follow-up"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn resuming_sends_each_queued_prompt_once_in_order() {
    let (host, logs) = logging_host();
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = paused_with(&mut acp, folder.path(), &["alpha", "bravo", "charlie"]).await;

    acp.call("_nori/session/queue/resume", json!({"sessionId": id}))
        .await
        .unwrap();
    wait_for_log(&logs, 4).await;
    let settled = wait_for_queue(&mut acp, &id, |queue| texts(queue).is_empty()).await;
    acp.call("_nori/session/queue/resume", json!({"sessionId": id}))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_eq!(
        prompt_log(&logs),
        vec!["mock:stream-until-cancel", "alpha", "bravo", "charlie"]
    );
    assert_eq!(settled["paused"], Value::Null, "{settled}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_prompt_another_client_queued_is_not_taken_into_this_composer() {
    let (host, _logs) = logging_host();
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = paused_with(&mut acp, folder.path(), &["theirs"]).await;
    let queue = row(&mut acp, &id).await["queue"].clone();

    let taken = acp
        .call(
            "_nori/session/queue/take",
            json!({"sessionId": id, "entryId": entry_id(&queue, "theirs")}),
        )
        .await
        .unwrap();

    assert_eq!(taken, json!({"text": null}));
    assert_eq!(texts(&row(&mut acp, &id).await["queue"]), vec!["theirs"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn discarding_drops_every_queued_prompt_without_sending_it() {
    let (host, logs) = logging_host();
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = paused_with(&mut acp, folder.path(), &["one", "two"]).await;

    let discarded = acp
        .call("_nori/session/queue/discard", json!({"sessionId": id}))
        .await
        .unwrap();
    let queue = wait_for_queue(&mut acp, &id, |queue| texts(queue).is_empty()).await;
    acp.call("_nori/session/queue/resume", json!({"sessionId": id}))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_eq!(discarded, json!({"discarded": 2}));
    assert_eq!(queue["paused"], Value::Null, "{queue}");
    assert_eq!(prompt_log(&logs), vec!["mock:stream-until-cancel"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn watchers_cannot_change_the_queue() {
    let (host, _logs) = logging_host();
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let id = paused_with(&mut acp, folder.path(), &["held"]).await;
    let queue = row(&mut acp, &id).await["queue"].clone();
    let held = entry_id(&queue, "held");
    let mut watching = watcher(&host);

    for (method, params) in [
        ("_nori/session/queue/resume", json!({"sessionId": id})),
        (
            "_nori/session/queue/take",
            json!({"sessionId": id, "entryId": held}),
        ),
        ("_nori/session/queue/discard", json!({"sessionId": id})),
    ] {
        let refused = watching.call(method, params).await.unwrap_err();
        assert_eq!(refused.code, -32016, "{method}: {refused:?}");
    }
    let queue = row(&mut acp, &id).await["queue"].clone();
    assert_eq!(texts(&queue), vec!["held"]);
    assert_eq!(queue["paused"], "interrupted");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_session_is_refused() {
    let host = host_with("", &[]);
    let mut acp = controller(&host);

    let refused = acp
        .call(
            "_nori/session/queue/resume",
            json!({"sessionId": nori_harness::ConversationId::new().to_string()}),
        )
        .await
        .unwrap_err();

    assert_eq!(refused.code, -32002, "{refused:?}");
}
