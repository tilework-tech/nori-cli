//! Sessions recorded by an earlier host are listed, replayed, and resumed
//! from their transcripts on disk.

use super::*;
use nori_harness::remote_agent::ManagedClient;
use serde_json::Value;
use serde_json::json;
use std::collections::HashSet;

const LIST: &str = "_nori/session/list";

impl Host {
    /// Stop the running host and start a new one over the same `NORI_HOME`,
    /// so only what the first host wrote to disk remains.
    fn restart(&self) {
        success(self.run(&["host", "stop"]));
        success(bounded_output(
            self.command(&["host", "--port", "0"])
                .env("MOCK_AGENT_WRITE_FILE", "marker.txt"),
        ));
    }

    /// Start a session in `folder` whose first prompt writes its marker, and
    /// wait until the agent has handled it.
    fn recorded_session(&self, folder: &std::path::Path, prompt: &str) -> String {
        let id = success(self.run(&[
            "host",
            "new",
            "--cwd",
            folder.to_str().unwrap(),
            "--prompt",
            prompt,
        ]));
        let marker = folder.join("marker.txt");
        wait_for_file(&marker);
        std::fs::remove_file(&marker).unwrap();
        id.trim().to_owned()
    }
}

fn client(host: &Host) -> ManagedClient {
    tokio::task::block_in_place(|| host.client())
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_of_a_previous_host_are_listed_from_their_transcripts() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let id = host.recorded_session(folder.path(), "write a marker");
    host.restart();

    let listing = client(&host).request(LIST, json!({})).await.unwrap();

    assert_eq!(listing["sessions"], json!([]), "{listing}");
    let stored = &listing["transcripts"][0];
    assert_eq!(stored["session_id"], id, "{listing}");
    assert_eq!(stored["cwd"], canonical(folder.path()));
    assert_eq!(stored["agent"], "mock-model");
    assert_eq!(stored["title"], "write a marker");
    let updated = stored["updated_at"].as_str().unwrap();
    assert!(
        chrono::DateTime::parse_from_rfc3339(updated).is_ok(),
        "updated_at is an RFC 3339 time: {updated}"
    );
    assert_eq!(listing["transcripts_next_cursor"], Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn stored_sessions_are_listed_in_pages() {
    let host = Host::writing_files();
    let folders = [(); 3].map(|()| TempDir::new().unwrap());
    let ids = folders
        .iter()
        .map(|folder| host.recorded_session(folder.path(), "write a marker"))
        .collect::<HashSet<_>>();
    host.restart();
    let mut client = client(&host);

    let first = client
        .request(LIST, json!({"transcript_limit": 2}))
        .await
        .unwrap();
    let cursor = first["transcripts_next_cursor"].as_str().unwrap();
    let second = client
        .request(
            LIST,
            json!({"transcript_limit": 2, "transcript_cursor": cursor}),
        )
        .await
        .unwrap();

    let page = |listing: &Value| listing["transcripts"].as_array().unwrap().clone();
    assert_eq!(page(&first).len(), 2, "{first}");
    assert_eq!(page(&second).len(), 1, "{second}");
    assert_eq!(second["transcripts_next_cursor"], Value::Null);
    let listed = page(&first)
        .iter()
        .chain(page(&second).iter())
        .map(|stored| stored["session_id"].as_str().unwrap().to_owned())
        .collect::<HashSet<_>>();
    assert_eq!(listed, ids);
    let time = |stored: &Value| {
        chrono::DateTime::parse_from_rfc3339(stored["updated_at"].as_str().unwrap()).unwrap()
    };
    assert!(
        page(&first)
            .iter()
            .all(|newer| time(newer) >= time(&page(&second)[0])),
        "pages are newest first: {first} {second}"
    );

    let refused = client
        .request(LIST, json!({"transcript_cursor": "not a cursor"}))
        .await
        .unwrap_err();
    assert!(
        refused.to_string().starts_with("host error -32602:"),
        "{refused}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stored_session_resumes_under_its_id_in_its_folder() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let id = host.recorded_session(folder.path(), "write a marker");
    host.restart();
    let mut client = client(&host);

    let resumed = client
        .request("_nori/session/resume", json!({"session_id": id}))
        .await
        .unwrap();
    assert_eq!(resumed["session_id"], id);

    let listing = client.request(LIST, json!({})).await.unwrap();
    assert_eq!(listing["sessions"][0]["session_id"], id, "{listing}");
    assert_eq!(listing["sessions"][0]["cwd"], canonical(folder.path()));
    assert_eq!(
        listing["transcripts"],
        json!([]),
        "a running session is listed once: {listing}"
    );
    send_prompt(&mut client, &id, "write again").await;
    tokio::task::block_in_place(|| wait_for_file(&folder.path().join("marker.txt")));
}

#[tokio::test(flavor = "multi_thread")]
async fn resuming_an_unknown_session_fails() {
    let host = Host::writing_files();
    let mut client = client(&host);

    let refused = client
        .request(
            "_nori/session/resume",
            json!({"session_id": uuid::Uuid::new_v4().to_string()}),
        )
        .await
        .unwrap_err();

    assert!(
        refused.to_string().contains("unknown session id"),
        "{refused}"
    );
    let listing = client.request(LIST, json!({})).await.unwrap();
    assert_eq!(listing["sessions"], json!([]));
}

/// Collect `_nori/session/event` envelopes until the transcript replay ends.
async fn replayed(client: &mut ManagedClient) -> Vec<Value> {
    let mut envelopes = Vec::new();
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let frame = client.recv().await.unwrap();
            if frame["method"] != "_nori/session/event" {
                continue;
            }
            let envelope = frame["params"].clone();
            let finished = envelope.to_string().contains("replay_finished");
            envelopes.push(envelope);
            if finished {
                return;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the replay never finished: {envelopes:?}"));
    envelopes
}

#[tokio::test(flavor = "multi_thread")]
async fn subscribing_to_a_stored_session_replays_its_transcript() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let id = host.recorded_session(folder.path(), "write a marker");
    host.restart();
    let mut client = client(&host);

    client
        .request("_nori/session/subscribe", json!({"session_id": id}))
        .await
        .unwrap();
    let envelopes = replayed(&mut client).await;

    assert!(
        envelopes
            .iter()
            .all(|envelope| envelope["conversation_id"] == id),
        "{envelopes:?}"
    );
    let seqs = envelopes
        .iter()
        .map(|envelope| envelope["seq"].as_i64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(seqs, (1..=seqs.len() as i64).collect::<Vec<_>>());
    assert!(
        envelopes
            .iter()
            .any(|envelope| envelope.to_string().contains("write a marker")),
        "the replay carries the recorded prompt: {envelopes:?}"
    );

    // A cursor skips what the client already has.
    let last = *seqs.last().unwrap();
    client
        .request(
            "_nori/session/subscribe",
            json!({"session_id": id, "cursors": {&id: last - 1}}),
        )
        .await
        .unwrap();
    let rest = replayed(&mut client).await;
    assert_eq!(rest.len(), 1, "{rest:?}");
    assert_eq!(rest[0]["seq"], last);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_damaged_transcript_is_left_out_of_the_listing() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let id = host.recorded_session(folder.path(), "write a marker");
    host.restart();
    let damaged = host
        .0
        .path()
        .join("transcripts/by-project/damaged/sessions");
    std::fs::create_dir_all(&damaged).unwrap();
    std::fs::write(
        damaged.join(format!("{}.jsonl", uuid::Uuid::new_v4())),
        "not a transcript\n",
    )
    .unwrap();

    let listing = client(&host).request(LIST, json!({})).await.unwrap();

    let listed = listing["transcripts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stored| stored["session_id"].clone())
        .collect::<Vec<_>>();
    assert_eq!(listed, vec![json!(id)]);
}
