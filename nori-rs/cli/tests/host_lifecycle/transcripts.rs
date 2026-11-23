//! Standard ACP `session/list`, `session/load`, and `session/resume` on the
//! host: live sessions and sessions an earlier host recorded on disk.

use super::acp_client::Acp;
use super::acp_client::is_update_mentioning;
use super::acp_client::mentions;
use super::*;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::collections::HashSet;

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
        let id = id.trim().to_owned();
        self.wait_until_ready(&id);
        id
    }

    /// Wait until the session's turn has finished, so its transcript holds
    /// the whole turn.
    fn wait_until_ready(&self, id: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let listed = success(self.run(&["host", "ls"]));
            let ready = listed
                .lines()
                .any(|line| line.starts_with(id) && line.contains("ready"));
            if ready {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "{id} never became ready: {listed}"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn acp(&self) -> Acp {
        Acp::new(tokio::task::block_in_place(|| self.client()))
    }
}

fn row<'a>(listing: &'a Value, id: &str) -> &'a Value {
    listing["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["sessionId"] == id)
        .unwrap_or_else(|| panic!("{id} is not listed: {listing}"))
}

/// The sessions the host is running, from the legacy list.
async fn running(acp: &mut Acp) -> HashSet<String> {
    acp.call("_nori/session/list", json!({})).await.unwrap()["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["session_id"].as_str().unwrap().to_owned())
        .collect()
}

/// Copy a recorded transcript under `count` new session IDs, as if earlier
/// runs had recorded them.
fn clone_transcripts(host: &Host, id: &str, count: usize) -> Vec<String> {
    let sessions = std::fs::read_dir(host.0.path().join("transcripts/by-project"))
        .unwrap()
        .map(|project| project.unwrap().path().join("sessions"))
        .find(|sessions| sessions.join(format!("{id}.jsonl")).exists())
        .unwrap();
    let recorded = std::fs::read_to_string(sessions.join(format!("{id}.jsonl"))).unwrap();
    (0..count)
        .map(|_| {
            let clone = uuid::Uuid::new_v4().to_string();
            std::fs::write(
                sessions.join(format!("{clone}.jsonl")),
                recorded.replace(id, &clone),
            )
            .unwrap();
            clone
        })
        .collect()
}

fn listed_ids(listing: &Value) -> Vec<String> {
    listing["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["sessionId"].as_str().unwrap().to_owned())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn session_list_shows_live_sessions_and_sessions_recorded_by_an_earlier_host() {
    let host = Host::writing_files();
    let stored_folder = TempDir::new().unwrap();
    let stored = host.recorded_session(stored_folder.path(), "write a marker");
    host.restart();
    let live_folder = TempDir::new().unwrap();
    let live = host.recorded_session(live_folder.path(), "write a live marker");

    let listing = host.acp().call("session/list", json!({})).await.unwrap();

    let stored_row = row(&listing, &stored);
    assert_eq!(stored_row["cwd"], canonical(stored_folder.path()));
    assert_eq!(stored_row["title"], "write a marker");
    let updated = stored_row["updatedAt"].as_str().unwrap();
    assert!(
        chrono::DateTime::parse_from_rfc3339(updated).is_ok(),
        "updatedAt is an RFC 3339 time: {updated}"
    );
    assert_eq!(row(&listing, &live)["cwd"], canonical(live_folder.path()));
    assert_eq!(listing.get("nextCursor"), None, "{listing}");
}

#[tokio::test(flavor = "multi_thread")]
async fn session_list_keeps_only_sessions_in_the_requested_folder() {
    let host = Host::writing_files();
    let wanted = TempDir::new().unwrap();
    let other = TempDir::new().unwrap();
    let kept = host.recorded_session(wanted.path(), "write a marker");
    host.recorded_session(other.path(), "write a marker");
    host.restart();
    let live = host.recorded_session(wanted.path(), "write a marker");
    host.recorded_session(other.path(), "write a marker");

    let listing = host
        .acp()
        .call("session/list", json!({"cwd": canonical(wanted.path())}))
        .await
        .unwrap();

    let listed = listed_ids(&listing).into_iter().collect::<HashSet<_>>();
    assert_eq!(listed, HashSet::from([kept, live]));
}

#[tokio::test(flavor = "multi_thread")]
async fn session_list_pages_through_every_session_with_its_cursor() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let recorded = host.recorded_session(folder.path(), "write a marker");
    let mut ids = clone_transcripts(&host, &recorded, 51)
        .into_iter()
        .collect::<HashSet<_>>();
    ids.insert(recorded);
    host.restart();
    let live_folder = TempDir::new().unwrap();
    ids.insert(host.recorded_session(live_folder.path(), "write a marker"));
    let mut acp = host.acp();

    let first = acp.call("session/list", json!({})).await.unwrap();
    let cursor = first["nextCursor"].as_str().unwrap().to_owned();
    let second = acp
        .call("session/list", json!({"cursor": cursor}))
        .await
        .unwrap();

    assert_eq!(listed_ids(&first).len(), 50);
    assert_eq!(listed_ids(&second).len(), 3, "{second}");
    assert_eq!(second.get("nextCursor"), None, "{second}");
    let listed = listed_ids(&first)
        .into_iter()
        .chain(listed_ids(&second))
        .collect::<HashSet<_>>();
    assert_eq!(listed, ids);

    let refused = acp
        .call("session/list", json!({"cursor": "not a cursor"}))
        .await
        .unwrap_err();
    assert_eq!(refused.code, -32602, "{refused:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn loading_a_stored_session_replays_its_history_before_answering_then_streams_live() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let id = host.recorded_session(folder.path(), "write a marker");
    host.restart();
    let mut acp = host.acp();

    let loaded = acp
        .call(
            "session/load",
            json!({"sessionId": id, "cwd": canonical(folder.path()), "mcpServers": []}),
        )
        .await;

    assert!(loaded.is_ok(), "{loaded:?}");
    assert!(
        mentions(&acp.inbox, &id, "write a marker"),
        "history is replayed before the response: {:?}",
        acp.inbox
    );
    assert!(running(&mut acp).await.contains(&id));

    acp.inbox.clear();
    acp.prompt_alias(&id, "write again").await;
    acp.until(is_update_mentioning(&id, "write again")).await;
    tokio::task::block_in_place(|| wait_for_file(&folder.path().join("marker.txt")));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_watcher_loading_a_live_session_sees_its_history_and_its_next_turn() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let id = host.recorded_session(folder.path(), "write a marker");
    let mut controller = host.acp();
    let mut watcher = Acp::new(tokio::task::block_in_place(|| host.observer()));

    watcher
        .call(
            "session/load",
            json!({"sessionId": id, "cwd": canonical(folder.path()), "mcpServers": []}),
        )
        .await
        .unwrap();
    assert!(
        mentions(&watcher.inbox, &id, "write a marker"),
        "{:?}",
        watcher.inbox
    );

    controller.prompt_alias(&id, "a second request").await;
    watcher
        .until(is_update_mentioning(&id, "a second request"))
        .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn resuming_a_stored_session_attaches_without_replaying_its_history() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let id = host.recorded_session(folder.path(), "write a marker");
    host.restart();
    let mut acp = host.acp();

    acp.call(
        "session/resume",
        json!({"sessionId": id, "cwd": canonical(folder.path())}),
    )
    .await
    .unwrap();
    acp.prompt_alias(&id, "write again").await;
    acp.until(is_update_mentioning(&id, "write again")).await;
    tokio::task::block_in_place(|| wait_for_file(&folder.path().join("marker.txt")));
    acp.collect_for(Duration::from_millis(500)).await;

    assert!(
        !mentions(&acp.log, &id, "write a marker"),
        "resume does not replay history: {:?}",
        acp.log
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn loading_in_another_folder_or_an_unknown_session_is_refused() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    let id = host.recorded_session(folder.path(), "write a marker");
    host.restart();
    let mut acp = host.acp();

    let moved = acp
        .call(
            "session/load",
            json!({"sessionId": id, "cwd": canonical(elsewhere.path()), "mcpServers": []}),
        )
        .await
        .unwrap_err();
    let unknown = acp
        .call(
            "session/resume",
            json!({"sessionId": uuid::Uuid::new_v4().to_string(), "cwd": canonical(folder.path())}),
        )
        .await
        .unwrap_err();

    let unknown_load = acp
        .call(
            "session/load",
            json!({"sessionId": uuid::Uuid::new_v4().to_string(), "cwd": canonical(folder.path()), "mcpServers": []}),
        )
        .await
        .unwrap_err();
    let moved_resume = acp
        .call(
            "session/resume",
            json!({"sessionId": id, "cwd": canonical(elsewhere.path())}),
        )
        .await
        .unwrap_err();

    assert_eq!(moved.code, -32602, "{moved:?}");
    assert_eq!(moved_resume.code, -32602, "{moved_resume:?}");
    assert_eq!(unknown.code, -32002, "{unknown:?}");
    assert_eq!(unknown_load.code, -32002, "{unknown_load:?}");
    assert!(
        !running(&mut acp).await.contains(&id),
        "nothing restarted it"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_legacy_resume_alias_still_resumes_a_stored_session() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let id = host.recorded_session(folder.path(), "write a marker");
    host.restart();
    let mut client = tokio::task::block_in_place(|| host.client());

    let resumed = client
        .request("_nori/session/resume", json!({"session_id": id}))
        .await
        .unwrap();
    assert_eq!(resumed["session_id"], id);

    let listing = client
        .request("_nori/session/list", json!({}))
        .await
        .unwrap();
    let resumed = listing["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["session_id"] == id)
        .unwrap_or_else(|| panic!("{listing}"));
    assert_eq!(resumed["cwd"], canonical(folder.path()));
    send_prompt(&mut client, &id, "write again").await;
    tokio::task::block_in_place(|| wait_for_file(&folder.path().join("marker.txt")));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_damaged_transcript_is_left_out_of_the_listing() {
    let host = Host::writing_files();
    let folder = TempDir::new().unwrap();
    let id = host.recorded_session(folder.path(), "write a marker");
    host.restart();
    let recorded = std::fs::read_dir(host.0.path().join("transcripts/by-project"))
        .unwrap()
        .any(|project| {
            project
                .unwrap()
                .path()
                .join(format!("sessions/{id}.jsonl"))
                .exists()
        });
    assert!(
        recorded,
        "transcripts live where the damaged one is written"
    );
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

    let listing = host.acp().call("session/list", json!({})).await.unwrap();

    assert_eq!(listed_ids(&listing), vec![id]);
}
