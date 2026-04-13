//! E2E coverage for ACP runaway in-progress tool snapshot handling.
//!
//! These tests drive the real `nori` binary against `mock-acp-agent`.
//! The mock emits many `in_progress` updates for the same Search tool call,
//! with a cumulatively growing text payload on each update.
//!
//! The transcript should retain durable tool lifecycle states without
//! persisting every intermediate rewrite of the same streaming tool call.

use std::path::Path;
use std::time::Duration;

use pretty_assertions::assert_eq;
use serde_json::Value;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

fn find_transcripts(nori_home: &Path) -> Vec<(String, String)> {
    let transcripts_dir = nori_home.join("transcripts").join("by-project");
    if !transcripts_dir.exists() {
        return Vec::new();
    }

    let mut results = Vec::new();
    if let Ok(projects) = std::fs::read_dir(&transcripts_dir) {
        for project_entry in projects.flatten() {
            let project_path = project_entry.path();
            if !project_path.is_dir() {
                continue;
            }

            let project_id = project_entry.file_name().to_string_lossy().into_owned();
            let sessions_dir = project_path.join("sessions");
            if !sessions_dir.exists() {
                continue;
            }

            if let Ok(sessions) = std::fs::read_dir(&sessions_dir) {
                for session_entry in sessions.flatten() {
                    let session_path = session_entry.path();
                    if session_path.extension().is_some_and(|ext| ext == "jsonl") {
                        let session_id = session_path
                            .file_stem()
                            .map(|stem| stem.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        results.push((project_id.clone(), session_id));
                    }
                }
            }
        }
    }

    results
}

fn read_transcript(nori_home: &Path, project_id: &str, session_id: &str) -> String {
    let path = nori_home
        .join("transcripts")
        .join("by-project")
        .join(project_id)
        .join("sessions")
        .join(format!("{session_id}.jsonl"));
    std::fs::read_to_string(path).expect("should read transcript")
}

#[derive(Debug)]
struct RunawaySnapshotStats {
    call_id: String,
    title: String,
    has_pending_snapshot: bool,
    has_completed_snapshot: bool,
    in_progress_count: usize,
    total_snapshot_count: usize,
    max_artifact_text_len: usize,
}

fn runaway_snapshot_stats(transcript: &str, expected_title: &str) -> Option<RunawaySnapshotStats> {
    let mut call_id = None;
    let mut title = None;
    let mut has_pending_snapshot = false;
    let mut has_completed_snapshot = false;
    let mut in_progress_count = 0usize;
    let mut total_snapshot_count = 0usize;
    let mut max_artifact_text_len = 0usize;

    for line in transcript.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).expect("transcript line should parse");
        if value.get("type").and_then(Value::as_str) != Some("client_event") {
            continue;
        }

        let Some(event) = value.get("event") else {
            continue;
        };
        if event.get("event_type").and_then(Value::as_str) != Some("tool_snapshot") {
            continue;
        }
        if event.get("title").and_then(Value::as_str) != Some(expected_title) {
            continue;
        }

        total_snapshot_count += 1;
        call_id = event
            .get("call_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or(call_id);
        title = event
            .get("title")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or(title);

        match event.get("phase").and_then(Value::as_str) {
            Some("pending") => has_pending_snapshot = true,
            Some("completed") => has_completed_snapshot = true,
            Some("in_progress") => in_progress_count += 1,
            Some(_) | None => {}
        }

        if let Some(artifacts) = event.get("artifacts").and_then(Value::as_array) {
            for artifact in artifacts {
                if artifact.get("artifact_type").and_then(Value::as_str) == Some("text")
                    && let Some(text) = artifact.get("text").and_then(Value::as_str)
                {
                    max_artifact_text_len = max_artifact_text_len.max(text.len());
                }
            }
        }
    }

    call_id.map(|call_id| RunawaySnapshotStats {
        call_id,
        title: title.unwrap_or_default(),
        has_pending_snapshot,
        has_completed_snapshot,
        in_progress_count,
        total_snapshot_count,
        max_artifact_text_len,
    })
}

fn find_runaway_snapshot_stats(nori_home: &Path, expected_title: &str) -> RunawaySnapshotStats {
    let mut matching_stats = find_transcripts(nori_home)
        .into_iter()
        .filter_map(|(project_id, session_id)| {
            let transcript = read_transcript(nori_home, &project_id, &session_id);
            runaway_snapshot_stats(&transcript, expected_title)
        })
        .collect::<Vec<_>>();

    assert_eq!(
        matching_stats.len(),
        1,
        "expected exactly one transcript matching {expected_title:?} in {nori_home:?}, found {matching_stats:?}"
    );

    matching_stats
        .pop()
        .expect("matching transcript should exist")
}

#[test]
#[cfg(target_os = "linux")]
fn test_runaway_search_transcript_omits_in_progress_snapshots_for_one_call() {
    let expected_title = "Search runaway-pattern in runaway-search-fixture";
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH", "1")
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH_UPDATES", "24")
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH_LINES_PER_UPDATE", "18")
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH_LINE_LEN", "96")
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH_DELAY_MS", "2");

    let mut session =
        TuiSession::spawn_with_config(24, 80, config).expect("failed to spawn runaway search TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start in ACP mode");
    std::thread::sleep(TIMEOUT_INPUT);

    let nori_home = session
        .nori_home_path()
        .expect("should have NORI_HOME path");

    session.send_str("reproduce runaway search").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    session
        .wait_for_text("Explored", Duration::from_secs(10))
        .expect("live in-progress tool updates should reach the TUI");
    session
        .wait_for_text("Runaway search scenario complete.", Duration::from_secs(15))
        .expect("mock runaway scenario should complete");

    std::thread::sleep(Duration::from_millis(500));
    session.send_key(Key::Ctrl('c')).unwrap();
    std::thread::sleep(Duration::from_millis(1000));

    let stats = find_runaway_snapshot_stats(&nori_home, expected_title);
    assert_eq!(stats.title, expected_title);
    assert_eq!(stats.call_id, "runaway-search-001");
    assert!(
        stats.has_pending_snapshot,
        "expected the transcript to keep the initial pending snapshot, stats={stats:?}"
    );
    assert!(
        stats.has_completed_snapshot,
        "expected the transcript to keep the final completed snapshot, stats={stats:?}"
    );
    assert_eq!(
        stats.in_progress_count, 0,
        "expected transcripts to omit in_progress snapshots for streaming tool updates, stats={stats:?}"
    );
    assert!(
        stats.total_snapshot_count >= 2,
        "expected the transcript to retain durable snapshots for the call, stats={stats:?}"
    );
    assert!(
        stats.max_artifact_text_len >= 20_000,
        "expected the final completed snapshot to preserve the search output, stats={stats:?}"
    );
}
