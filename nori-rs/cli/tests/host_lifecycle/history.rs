//! `_nori/host/history/*`: the prompts any terminal sends are kept in the
//! host's own history, so every terminal on that host recalls and searches
//! the same list.

use super::acp_v1::connect;
use super::acp_v1::controller;
use super::acp_v1::host_with;
use super::acp_v1::new_session;
use super::acp_v1::watcher;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

fn texts(result: &Value) -> Vec<&str> {
    result["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["text"].as_str().unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_advertises_history() {
    let host = host_with("", &[]);
    let mut client = connect(&host);
    let result = client.request("initialize", json!({})).await.unwrap();
    assert_eq!(
        result["capabilities"]["nori_sessions"]["history"],
        json!(true)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_prompt_one_terminal_records_is_recalled_and_found_by_another() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);
    let before = watching
        .call("_nori/host/history/metadata", json!({}))
        .await
        .unwrap();
    assert_eq!(before["entryCount"], json!(0), "{before}");

    for text in ["plant the tomatoes", "water the basil"] {
        acp.call(
            "_nori/host/history/add",
            json!({"sessionId": id, "text": text}),
        )
        .await
        .unwrap();
    }

    let after = watching
        .call("_nori/host/history/metadata", json!({}))
        .await
        .unwrap();
    assert_eq!(after["entryCount"], json!(2), "{after}");
    let entry = watching
        .call(
            "_nori/host/history/entry",
            json!({"logId": after["logId"], "offset": 1}),
        )
        .await
        .unwrap();
    assert_eq!(
        (
            entry["entry"]["text"].clone(),
            entry["entry"]["sessionId"].clone()
        ),
        (json!("water the basil"), json!(id))
    );
    let past_the_end = watching
        .call(
            "_nori/host/history/entry",
            json!({"logId": after["logId"], "offset": 2}),
        )
        .await
        .unwrap();
    assert_eq!(past_the_end, json!({"entry": null}));

    let found = watching
        .call("_nori/host/history/search", json!({"maxResults": 10}))
        .await
        .unwrap();
    // Newest first by the second each was sent; these share one.
    let mut found = texts(&found);
    found.sort_unstable();
    assert_eq!(found, vec!["plant the tomatoes", "water the basil"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn history_is_kept_in_the_hosts_own_home() {
    let host = host_with("", &[]);
    let client_home = TempDir::new().unwrap();
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;

    acp.call(
        "_nori/host/history/add",
        json!({"sessionId": id, "text": "prune the roses"}),
    )
    .await
    .unwrap();

    let kept = std::fs::read_to_string(host.0.path().join("history.jsonl")).unwrap();
    assert!(kept.contains("prune the roses"), "{kept}");
    assert!(!client_home.path().join("history.jsonl").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_cannot_record_a_prompt() {
    let host = host_with("", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;
    let mut watching = watcher(&host);

    let refused = watching
        .call(
            "_nori/host/history/add",
            json!({"sessionId": id, "text": "dig a pond"}),
        )
        .await
        .unwrap_err();
    assert_eq!(refused.code, -32016);
    let found = watching
        .call("_nori/host/history/search", json!({"maxResults": 10}))
        .await
        .unwrap();
    assert_eq!(texts(&found), Vec::<&str>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_is_recorded_when_the_host_keeps_no_history() {
    let host = host_with("history_persistence = \"none\"", &[]);
    let folder = TempDir::new().unwrap();
    let mut acp = controller(&host);
    let (id, _) = new_session(&mut acp, folder.path()).await;

    acp.call(
        "_nori/host/history/add",
        json!({"sessionId": id, "text": "mulch the beds"}),
    )
    .await
    .unwrap();

    let found = acp
        .call("_nori/host/history/search", json!({"maxResults": 10}))
        .await
        .unwrap();
    assert_eq!(texts(&found), Vec::<&str>::new());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_prompt_for_a_session_the_host_does_not_know_is_not_recorded() {
    let host = host_with("", &[]);
    let mut acp = controller(&host);
    let unknown = nori_harness::ConversationId::new().to_string();

    let refused = acp
        .call(
            "_nori/host/history/add",
            json!({"sessionId": unknown, "text": "water the ferns"}),
        )
        .await
        .unwrap_err();
    let malformed = acp
        .call(
            "_nori/host/history/add",
            json!({"sessionId": "not-a-session", "text": "trim the hedge"}),
        )
        .await
        .unwrap_err();

    assert_eq!((refused.code, malformed.code), (-32002, -32002));
    let found = acp
        .call("_nori/host/history/search", json!({"maxResults": 10}))
        .await
        .unwrap();
    assert_eq!(texts(&found), Vec::<&str>::new());
}
