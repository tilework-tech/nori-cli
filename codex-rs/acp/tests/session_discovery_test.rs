//! Tests for session transcript discovery functionality.

use codex_acp::DiscoveryError;
use codex_acp::cwd_to_claude_project_path;
use codex_acp::discover_transcript_path;
use codex_acp::discover_transcript_path_with_home;
use codex_acp::session_parser::AgentKind;
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;
use tokio::fs;

/// Helper to create a mock Claude projects directory structure
async fn create_claude_project_structure(
    home_dir: &Path,
    project_path: &str,
    session_id: &str,
) -> PathBuf {
    let projects_dir = home_dir.join(".claude").join("projects");
    let project_dir = projects_dir.join(project_path);
    fs::create_dir_all(&project_dir)
        .await
        .expect("create project dir");

    let transcript_path = project_dir.join(format!("{session_id}.jsonl"));
    fs::write(&transcript_path, r#"{"sessionId": "test"}"#)
        .await
        .expect("write transcript");

    transcript_path
}

/// Helper to create a mock Codex sessions directory structure
async fn create_codex_session_structure(
    home_dir: &Path,
    date_path: &str, // e.g., "2024/12/25"
    session_guid: &str,
) -> PathBuf {
    let sessions_dir = home_dir.join(".codex").join("sessions").join(date_path);
    fs::create_dir_all(&sessions_dir)
        .await
        .expect("create sessions dir");

    let transcript_filename = format!("rollout-2024-12-25T10-30-00-{session_guid}.jsonl");
    let transcript_path = sessions_dir.join(&transcript_filename);
    fs::write(
        &transcript_path,
        r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":100}}}}"#,
    )
    .await
    .expect("write transcript");

    transcript_path
}

/// Helper to create a mock Gemini tmp directory structure
async fn create_gemini_session_structure(
    home_dir: &Path,
    hashed_path: &str,
    session_id: &str,
) -> PathBuf {
    let chats_dir = home_dir
        .join(".gemini")
        .join("tmp")
        .join(hashed_path)
        .join("chats");
    fs::create_dir_all(&chats_dir)
        .await
        .expect("create chats dir");

    let transcript_filename = format!("session-2024-12-25T10-30-{session_id}.json");
    let transcript_path = chats_dir.join(&transcript_filename);
    fs::write(&transcript_path, r#"{"sessionId": "test", "messages": []}"#)
        .await
        .expect("write transcript");

    transcript_path
}

#[test]
fn test_cwd_to_claude_project_path_converts_slashes_to_dashes() {
    let cwd = Path::new("/home/user/nori-cli");
    let project_path = cwd_to_claude_project_path(cwd);

    // Expected: `/home/user/nori-cli` -> `-home-user-nori-cli`
    assert_eq!(
        project_path, "-home-user-nori-cli",
        "cwd should be converted to project path with dashes"
    );
}

#[test]
fn test_cwd_to_claude_project_path_handles_root() {
    let cwd = Path::new("/");
    let project_path = cwd_to_claude_project_path(cwd);

    // Root path should become just "-"
    assert_eq!(project_path, "-", "root path should convert to single dash");
}

#[test]
fn test_cwd_to_claude_project_path_handles_nested_path() {
    let cwd = Path::new("/a/b/c/d/e");
    let project_path = cwd_to_claude_project_path(cwd);

    assert_eq!(
        project_path, "-a-b-c-d-e",
        "nested path should have all slashes replaced"
    );
}

#[tokio::test]
async fn test_claude_transcript_discovery_finds_existing_transcript() {
    let temp_dir = TempDir::new().expect("create temp dir");

    // Simulate cwd as /home/user/myproject
    let fake_cwd = PathBuf::from("/home/user/myproject");
    let project_path = "-home-user-myproject";
    let session_id = "abc123-def456-789";

    // Create the expected transcript file in the mock home directory
    let expected_transcript =
        create_claude_project_structure(temp_dir.path(), project_path, session_id).await;

    // Use the _with_home variant for testing
    let result = discover_transcript_path_with_home(
        AgentKind::Claude,
        session_id,
        &fake_cwd,
        temp_dir.path(),
    )
    .await;

    match result {
        Ok(path) => {
            assert_eq!(
                path, expected_transcript,
                "should find the correct transcript path"
            );
        }
        Err(e) => panic!("expected to find transcript, got error: {e}"),
    }
}

#[tokio::test]
async fn test_claude_transcript_discovery_returns_error_for_missing_transcript() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let fake_cwd = PathBuf::from("/home/user/nonexistent-project");
    let session_id = "does-not-exist-session";

    let result = discover_transcript_path_with_home(
        AgentKind::Claude,
        session_id,
        &fake_cwd,
        temp_dir.path(),
    )
    .await;

    // Should return TranscriptNotFound when the file doesn't exist
    assert!(
        matches!(result, Err(DiscoveryError::TranscriptNotFound)),
        "should return TranscriptNotFound for missing transcript, got: {result:?}"
    );
}

#[tokio::test]
async fn test_codex_transcript_discovery_finds_by_session_guid() {
    let temp_dir = TempDir::new().expect("create temp dir");

    let session_guid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let date_path = "2024/12/25";

    // Create the expected transcript file
    let expected_transcript =
        create_codex_session_structure(temp_dir.path(), date_path, session_guid).await;

    let fake_cwd = PathBuf::from("/home/user/project");

    let result = discover_transcript_path_with_home(
        AgentKind::Codex,
        session_guid,
        &fake_cwd,
        temp_dir.path(),
    )
    .await;

    match result {
        Ok(path) => {
            assert!(
                path.to_string_lossy().contains(session_guid),
                "found transcript should contain session GUID"
            );
            assert_eq!(
                path, expected_transcript,
                "should find the correct transcript path"
            );
        }
        Err(e) => panic!("expected to find transcript, got error: {e}"),
    }
}

#[tokio::test]
async fn test_codex_transcript_discovery_returns_error_for_missing_session() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let fake_cwd = PathBuf::from("/home/user/project");
    let session_guid = "nonexistent-guid";

    let result = discover_transcript_path_with_home(
        AgentKind::Codex,
        session_guid,
        &fake_cwd,
        temp_dir.path(),
    )
    .await;

    assert!(
        matches!(result, Err(DiscoveryError::TranscriptNotFound)),
        "should return TranscriptNotFound for missing Codex session"
    );
}

#[tokio::test]
async fn test_gemini_transcript_discovery_finds_by_session_id() {
    let temp_dir = TempDir::new().expect("create temp dir");

    let session_id = "gem-session-12345";
    let hashed_path = "hashed_project_path";

    // Create the expected transcript file
    let expected_transcript =
        create_gemini_session_structure(temp_dir.path(), hashed_path, session_id).await;

    let fake_cwd = PathBuf::from("/home/user/project");

    let result = discover_transcript_path_with_home(
        AgentKind::Gemini,
        session_id,
        &fake_cwd,
        temp_dir.path(),
    )
    .await;

    match result {
        Ok(path) => {
            assert!(
                path.to_string_lossy().contains(session_id),
                "found transcript should contain session ID"
            );
            assert_eq!(
                path, expected_transcript,
                "should find the correct transcript path"
            );
        }
        Err(e) => panic!("expected to find transcript, got error: {e}"),
    }
}

#[tokio::test]
async fn test_gemini_transcript_discovery_returns_error_for_missing_session() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let fake_cwd = PathBuf::from("/home/user/project");
    let session_id = "nonexistent-gemini-session";

    let result = discover_transcript_path_with_home(
        AgentKind::Gemini,
        session_id,
        &fake_cwd,
        temp_dir.path(),
    )
    .await;

    assert!(
        matches!(result, Err(DiscoveryError::TranscriptNotFound)),
        "should return TranscriptNotFound for missing Gemini session"
    );
}

#[tokio::test]
async fn test_discover_transcript_path_uses_real_home() {
    // This test verifies the main function works (uses real home directory)
    // It should return TranscriptNotFound for a nonexistent session
    let fake_cwd = PathBuf::from("/tmp/nonexistent-project-12345");
    let session_id = "nonexistent-session-xyz";

    let result = discover_transcript_path(AgentKind::Claude, session_id, &fake_cwd).await;

    // Should not find the transcript (unless by some coincidence it exists)
    assert!(
        matches!(result, Err(DiscoveryError::TranscriptNotFound)),
        "should return TranscriptNotFound for nonexistent session"
    );
}
