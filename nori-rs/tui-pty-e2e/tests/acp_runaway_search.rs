//! E2E coverage for lossless ACP tool-update transcript recording.
//!
//! These tests drive the real `nori` binary against `mock-acp-agent`.
//! The mock emits many `in_progress` updates for the same Search tool call,
//! with a cumulatively growing text payload on each update.
//!
//! The v3 transcript should retain the exact raw ACP notifications. Compaction
//! and presentation are consumer concerns, not behavior of the public boundary.

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
struct RunawayToolEventStats {
    call_id: String,
    title: String,
    has_pending_snapshot: bool,
    has_completed_snapshot: bool,
    in_progress_count: usize,
    total_event_count: usize,
    max_content_text_len: usize,
}

fn max_text_len(value: &Value) -> usize {
    match value {
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| {
                let own = if key == "text" {
                    value.as_str().map(str::len).unwrap_or_default()
                } else {
                    0
                };
                own.max(max_text_len(value))
            })
            .max()
            .unwrap_or_default(),
        Value::Array(values) => values.iter().map(max_text_len).max().unwrap_or_default(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 0,
    }
}

fn runaway_tool_event_stats(transcript: &str) -> Option<RunawayToolEventStats> {
    let mut call_id = None;
    let mut title = None;
    let mut has_pending_snapshot = false;
    let mut has_completed_snapshot = false;
    let mut in_progress_count = 0usize;
    let mut total_event_count = 0usize;
    let mut max_content_text_len = 0usize;

    for line in transcript.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value = serde_json::from_str(line).expect("transcript line should parse");
        if value.get("type").and_then(Value::as_str) != Some("session_event") {
            continue;
        }

        let Some(event) = value.pointer("/event/event").filter(|event| {
            event.get("message_type").and_then(Value::as_str) == Some("notification")
        }) else {
            continue;
        };
        let Some(update) = event.get("update") else {
            continue;
        };
        let update_kind = update.get("sessionUpdate").and_then(Value::as_str);
        if !matches!(update_kind, Some("tool_call" | "tool_call_update")) {
            continue;
        }

        let event_call_id = update
            .get("toolCallId")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if event_call_id != "runaway-search-001" {
            continue;
        }

        total_event_count += 1;
        call_id = Some(event_call_id.to_string());
        title = update
            .get("title")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .or(title);

        match (update_kind, update.get("status").and_then(Value::as_str)) {
            (Some("tool_call"), None) => has_pending_snapshot = true,
            (_, Some("pending")) => has_pending_snapshot = true,
            (_, Some("completed")) => has_completed_snapshot = true,
            (_, Some("in_progress")) => in_progress_count += 1,
            (_, Some(_) | None) => {}
        }
        max_content_text_len = max_content_text_len.max(max_text_len(update));
    }

    call_id.map(|call_id| RunawayToolEventStats {
        call_id,
        title: title.unwrap_or_default(),
        has_pending_snapshot,
        has_completed_snapshot,
        in_progress_count,
        total_event_count,
        max_content_text_len,
    })
}

fn find_runaway_tool_event_stats(nori_home: &Path, expected_title: &str) -> RunawayToolEventStats {
    let mut matching_stats = find_transcripts(nori_home)
        .into_iter()
        .filter_map(|(project_id, session_id)| {
            let transcript = read_transcript(nori_home, &project_id, &session_id);
            runaway_tool_event_stats(&transcript)
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
fn test_runaway_search_transcript_preserves_the_raw_acp_tool_stream() {
    let expected_title = "Search runaway-pattern in runaway-search-fixture";
    let config = SessionConfig::new()
        .with_agent("mock-model".to_owned())
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

    session.submit_input("reproduce runaway search").unwrap();

    session
        .wait_for_text("Explored", Duration::from_secs(10))
        .expect("live in-progress tool updates should reach the TUI");
    session
        .wait_for_text("Runaway search scenario complete.", Duration::from_secs(15))
        .expect("mock runaway scenario should complete");

    std::thread::sleep(Duration::from_millis(500));
    session.send_key(Key::Ctrl('c')).unwrap();
    std::thread::sleep(Duration::from_millis(1000));

    let stats = find_runaway_tool_event_stats(&nori_home, expected_title);
    assert_eq!(stats.title, expected_title);
    assert_eq!(stats.call_id, "runaway-search-001");
    assert!(
        stats.has_pending_snapshot,
        "expected the transcript to keep the initial pending tool call, stats={stats:?}"
    );
    assert!(
        stats.has_completed_snapshot,
        "expected the transcript to keep the final completed update, stats={stats:?}"
    );
    assert_eq!(
        stats.in_progress_count, 24,
        "expected every raw in-progress ACP update to remain observable, stats={stats:?}"
    );
    assert_eq!(
        stats.total_event_count, 26,
        "expected one pending call, 24 progress updates, and one completion, stats={stats:?}"
    );
    assert!(
        stats.max_content_text_len >= 20_000,
        "expected the raw update stream to preserve the search output, stats={stats:?}"
    );
}
