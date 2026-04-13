//! E2E repro for ACP runaway in-progress tool snapshots.
//!
//! These tests drive the real `nori` binary against `mock-acp-agent`.
//! The mock emits many `in_progress` updates for the same Search tool call,
//! with a cumulatively growing text payload on each update.
//!
//! This reproduces the current ACP backend amplification bug:
//! one streaming tool call is normalized and persisted as many full snapshots.

use std::path::Path;
use std::time::Duration;

use pretty_assertions::assert_eq;
use serde_json::Value;
use tui_pty_e2e::Key;
use tui_pty_e2e::SessionConfig;
use tui_pty_e2e::TIMEOUT;
use tui_pty_e2e::TIMEOUT_INPUT;
use tui_pty_e2e::TuiSession;

fn extract_mock_agent_pids_from_log(log_path: &Path) -> Vec<u32> {
    let re = regex::Regex::new("ACP agent spawned \\(pid: Some\\((\\d+)\\)\\)")
        .expect("invalid pid regex");

    std::fs::read_to_string(log_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            re.captures(line)
                .and_then(|caps| caps.get(1).and_then(|m| m.as_str().parse().ok()))
        })
        .collect()
}

fn process_exists_and_not_zombie(pid: u32) -> bool {
    let proc_path = format!("/proc/{pid}");
    if !Path::new(&proc_path).exists() {
        return false;
    }

    let status_path = format!("/proc/{pid}/status");
    if let Ok(status) = std::fs::read_to_string(status_path) {
        for line in status.lines() {
            if line.starts_with("State:") {
                return !line.contains("Z (");
            }
        }
    }

    true
}

fn parent_pid_of(pid: u32) -> Option<u32> {
    let status_path = format!("/proc/{pid}/status");
    let status = std::fs::read_to_string(status_path).ok()?;
    status.lines().find_map(|line| {
        line.strip_prefix("PPid:")
            .map(str::trim)
            .and_then(|value| value.parse::<u32>().ok())
    })
}

#[cfg(unix)]
fn set_address_space_limit(pid: u32, bytes: u64) {
    let limit = libc::rlimit {
        rlim_cur: bytes as libc::rlim_t,
        rlim_max: bytes as libc::rlim_t,
    };

    let rc = unsafe {
        libc::prlimit(
            pid as libc::pid_t,
            libc::RLIMIT_AS,
            &limit,
            std::ptr::null_mut(),
        )
    };

    assert_eq!(
        rc,
        0,
        "prlimit(RLIMIT_AS) should succeed for pid {pid}: {}",
        std::io::Error::last_os_error()
    );
}

fn wait_for_process_exit(pid: u32, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if !process_exists_and_not_zombie(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !process_exists_and_not_zombie(pid)
}

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
    in_progress_count: usize,
    total_snapshot_count: usize,
    max_artifact_text_len: usize,
}

fn runaway_snapshot_stats(transcript: &str, expected_title: &str) -> RunawaySnapshotStats {
    let mut call_id = None;
    let mut title = None;
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

        if event.get("phase").and_then(Value::as_str) == Some("in_progress") {
            in_progress_count += 1;
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

    RunawaySnapshotStats {
        call_id: call_id.unwrap_or_default(),
        title: title.unwrap_or_default(),
        in_progress_count,
        total_snapshot_count,
        max_artifact_text_len,
    }
}

#[test]
#[cfg(target_os = "linux")]
fn test_runaway_search_repro_persists_many_in_progress_snapshots_for_one_call() {
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
        .wait_for_text("Runaway search scenario complete.", Duration::from_secs(15))
        .expect("mock runaway scenario should complete");

    std::thread::sleep(Duration::from_millis(500));
    session.send_key(Key::Ctrl('c')).unwrap();
    std::thread::sleep(Duration::from_millis(1000));

    let transcripts = find_transcripts(&nori_home);
    assert_eq!(
        transcripts.len(),
        1,
        "expected exactly one transcript in {nori_home:?}, found {transcripts:?}"
    );

    let (project_id, session_id) = &transcripts[0];
    let transcript = read_transcript(&nori_home, project_id, session_id);
    let stats = runaway_snapshot_stats(&transcript, expected_title);

    assert_eq!(stats.title, expected_title);
    assert_eq!(stats.call_id, "runaway-search-001");
    assert_eq!(
        stats.in_progress_count, 24,
        "expected one in_progress snapshot per update, stats={stats:?}"
    );
    assert!(
        stats.total_snapshot_count >= stats.in_progress_count,
        "expected total snapshots to include all in_progress updates, stats={stats:?}"
    );
    assert!(
        stats.max_artifact_text_len >= 20_000,
        "expected cumulative artifact text growth, stats={stats:?}"
    );
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "manual stress repro for the current ACP backend crash"]
fn test_runaway_search_repro_eventually_crashes_nori_under_memory_pressure() {
    let expected_title = "Search runaway-pattern in runaway-search-fixture";
    let config = SessionConfig::new()
        .with_model("mock-model".to_owned())
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH", "1")
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH_UPDATES", "800")
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH_LINES_PER_UPDATE", "48")
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH_LINE_LEN", "128")
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH_DELAY_MS", "5")
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH_SKIP_COMPLETION", "1")
        .with_agent_env("MOCK_AGENT_RUNAWAY_SEARCH_SKIP_FINAL_TEXT", "1");

    let mut session = TuiSession::spawn_with_config(24, 80, config)
        .expect("failed to spawn runaway crash repro TUI");

    session
        .wait_for_text("›", TIMEOUT)
        .expect("TUI should start in ACP mode");
    std::thread::sleep(TIMEOUT_INPUT);

    let nori_home = session
        .nori_home_path()
        .expect("should have NORI_HOME path");
    let log_path = session.acp_log_path().expect("should have ACP log path");
    let agent_pid = extract_mock_agent_pids_from_log(&log_path)
        .into_iter()
        .next()
        .expect("should have spawned mock agent");
    let nori_pid = parent_pid_of(agent_pid).expect("mock agent should have nori parent pid");

    session.send_str("crash with runaway search").unwrap();
    std::thread::sleep(TIMEOUT_INPUT);
    session.send_key(Key::Enter).unwrap();

    std::thread::sleep(Duration::from_millis(1200));
    set_address_space_limit(nori_pid, 256 * 1024 * 1024);

    assert!(
        wait_for_process_exit(nori_pid, Duration::from_secs(30)),
        "nori pid {nori_pid} should exit unexpectedly under runaway snapshot pressure; screen:\n{}",
        session.screen_contents()
    );

    std::thread::sleep(Duration::from_millis(500));

    let transcripts = find_transcripts(&nori_home);
    assert!(
        !transcripts.is_empty(),
        "expected a transcript to be left behind after crash in {nori_home:?}"
    );

    let (project_id, session_id) = &transcripts[0];
    let transcript = read_transcript(&nori_home, project_id, session_id);
    let stats = runaway_snapshot_stats(&transcript, expected_title);

    assert_eq!(stats.title, expected_title);
    assert_eq!(stats.call_id, "runaway-search-001");
    assert!(
        stats.in_progress_count >= 50,
        "expected many repeated in_progress snapshots before crash, stats={stats:?}"
    );
    assert!(
        stats.max_artifact_text_len >= 500_000,
        "expected large cumulative artifact payload before crash, stats={stats:?}"
    );
}
