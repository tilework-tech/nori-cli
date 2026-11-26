//! A paused queue on the host: any terminal sees it on its list row, the
//! controller resumes it or discards it, and watchers cannot change it.
//! A terminal that declares `capabilities.nori_sessions.composer` takes its
//! own queued prompts back, once; prompts from a client that does not
//! declare it, such as the Sessions broker, stay queued when taken.

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
    panic!("the agent saw {wanted} prompts: {:?}", prompt_log(logs));
}

/// Waits until the session is ready with nothing queued, so no further
/// prompt can still be on its way to the agent.
async fn settled(acp: &mut Acp, id: &str) -> Value {
    for _ in 0..200 {
        let row = row(acp, id).await;
        if row["status"] == "ready" && texts(&row["queue"]).is_empty() {
            return row["queue"].clone();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("session {id} settled: {}", row(acp, id).await);
}

fn take(id: &str, entry: &Value, key: &str) -> Value {
    json!({"sessionId": id, "entryId": entry, "requestKey": key})
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
    let settled = settled(&mut acp, &id).await;
    acp.call("_nori/session/queue/resume", json!({"sessionId": id}))
        .await
        .unwrap();

    assert_eq!(
        prompt_log(&logs),
        vec!["mock:stream-until-cancel", "alpha", "bravo", "charlie"]
    );
    assert_eq!(settled["paused"], Value::Null, "{settled}");
}

/// A controller whose follow-ups are typed by its own user.
fn composer(host: &Host) -> Acp {
    Acp::new(tokio::task::block_in_place(|| {
        host.connect(json!({"capabilities": {"nori_sessions": {"composer": true}}}))
    }))
}

#[tokio::test(flavor = "multi_thread")]
async fn a_terminal_takes_back_a_prompt_it_queued_exactly_once() {
    let (host, logs) = logging_host();
    let folder = TempDir::new().unwrap();
    let mut acp = composer(&host);
    let id = paused_with(&mut acp, folder.path(), &["mine", "kept"]).await;
    let queue = row(&mut acp, &id).await["queue"].clone();
    let mine = entry_id(&queue, "mine");

    let first = acp
        .call("_nori/session/queue/take", take(&id, &mine, "first"))
        .await
        .unwrap();
    let second = acp
        .call("_nori/session/queue/take", take(&id, &mine, "second"))
        .await
        .unwrap();
    acp.call("_nori/session/queue/resume", json!({"sessionId": id}))
        .await
        .unwrap();
    wait_for_log(&logs, 2).await;
    settled(&mut acp, &id).await;

    assert_eq!(first, json!({"text": "mine"}));
    assert_eq!(second, json!({"text": null}));
    assert_eq!(prompt_log(&logs), vec!["mock:stream-until-cancel", "kept"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_take_asked_again_with_its_key_gets_the_same_text_and_no_one_else_does() {
    let (host, logs) = logging_host();
    let folder = TempDir::new().unwrap();
    let mut acp = composer(&host);
    let id = paused_with(&mut acp, folder.path(), &["retried", "kept"]).await;
    let queue = row(&mut acp, &id).await["queue"].clone();
    let retried = entry_id(&queue, "retried");

    let first = acp
        .call(
            "_nori/session/queue/take",
            take(&id, &retried, "lost-reply"),
        )
        .await
        .unwrap();
    drop(acp);
    let mut reconnected = composer(&host);
    reconnected
        .call("_nori/control/take", json!({}))
        .await
        .unwrap();
    let again = reconnected
        .call(
            "_nori/session/queue/take",
            take(&id, &retried, "lost-reply"),
        )
        .await
        .unwrap();
    let other = reconnected
        .call("_nori/session/queue/take", take(&id, &retried, "another"))
        .await
        .unwrap();
    reconnected
        .call("_nori/session/queue/resume", json!({"sessionId": id}))
        .await
        .unwrap();
    wait_for_log(&logs, 2).await;
    settled(&mut reconnected, &id).await;

    assert_eq!(first, json!({"text": "retried"}));
    assert_eq!(again, json!({"text": "retried"}));
    assert_eq!(other, json!({"text": null}));
    assert_eq!(prompt_log(&logs), vec!["mock:stream-until-cancel", "kept"]);
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
            take(&id, &entry_id(&queue, "theirs"), "theirs"),
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
    let queue = settled(&mut acp, &id).await;
    acp.call("_nori/session/queue/resume", json!({"sessionId": id}))
        .await
        .unwrap();

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
        ("_nori/session/queue/take", take(&id, &held, "watching")),
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

#[tokio::test(flavor = "multi_thread")]
async fn whichever_terminal_controls_the_session_takes_a_terminal_prompt_back_once() {
    let (host, logs) = logging_host();
    let folder = TempDir::new().unwrap();
    let mut first = composer(&host);
    let id = paused_with(
        &mut first,
        folder.path(),
        &["prune the roses", "plant the cedar row"],
    )
    .await;
    let queue = row(&mut first, &id).await["queue"].clone();
    let roses = entry_id(&queue, "prune the roses");
    let cedar = entry_id(&queue, "plant the cedar row");
    // The terminal that queued them detaches.
    drop(first);

    // A fresh terminal attaches and takes control.
    let mut fresh = composer(&host);
    fresh.call("_nori/control/take", json!({})).await.unwrap();
    let fresh_takes = fresh
        .call("_nori/session/queue/take", take(&id, &roses, "fresh-roses"))
        .await
        .unwrap();

    // Another terminal takes control from it; the first one now watches.
    let mut other = composer(&host);
    other.call("_nori/control/take", json!({})).await.unwrap();
    let watcher_refused = fresh
        .call("_nori/session/queue/take", take(&id, &cedar, "watching"))
        .await
        .unwrap_err();
    let other_takes = other
        .call("_nori/session/queue/take", take(&id, &cedar, "other-cedar"))
        .await
        .unwrap();
    let taken_again = other
        .call("_nori/session/queue/take", take(&id, &cedar, "again"))
        .await
        .unwrap();

    // The edited prompt goes out as a new submission.
    other
        .call(
            "_nori/session/prompt",
            json!({"session_id": id, "prompt": text("plant the cedar row by the fence")}),
        )
        .await
        .unwrap();
    wait_for_log(&logs, 2).await;
    settled(&mut other, &id).await;

    assert_eq!(fresh_takes, json!({"text": "prune the roses"}));
    assert_eq!(watcher_refused.code, -32016);
    assert_eq!(other_takes, json!({"text": "plant the cedar row"}));
    assert_eq!(taken_again, json!({"text": null}));
    assert_eq!(
        prompt_log(&logs),
        vec![
            "mock:stream-until-cancel",
            "plant the cedar row by the fence"
        ],
        "only the corrected prompt reaches the agent"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_terminal_in_control_cannot_take_back_a_brokers_prompt() {
    let (host, _logs) = logging_host();
    let folder = TempDir::new().unwrap();
    let mut broker = controller(&host);
    let id = paused_with(&mut broker, folder.path(), &["from the broker"]).await;
    let queue = row(&mut broker, &id).await["queue"].clone();
    let mut terminal = composer(&host);
    terminal
        .call("_nori/control/take", json!({}))
        .await
        .unwrap();

    let taken = terminal
        .call(
            "_nori/session/queue/take",
            take(&id, &entry_id(&queue, "from the broker"), "terminal"),
        )
        .await
        .unwrap();

    assert_eq!(taken, json!({"text": null}));
    assert_eq!(
        texts(&row(&mut terminal, &id).await["queue"]),
        vec!["from the broker"]
    );
}
