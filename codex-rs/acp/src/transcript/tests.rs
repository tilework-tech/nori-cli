//! Tests for transcript persistence.

use super::*;
use tempfile::TempDir;

mod project_id_tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::process::Command;

    /// Helper to initialize a git repo with optional remote.
    fn init_git_repo(dir: &std::path::Path, remote_url: Option<&str>) {
        Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .expect("git init failed");

        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(dir)
            .output()
            .expect("git config email failed");

        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(dir)
            .output()
            .expect("git config name failed");

        if let Some(url) = remote_url {
            Command::new("git")
                .args(["remote", "add", "origin", url])
                .current_dir(dir)
                .output()
                .expect("git remote add failed");
        }
    }

    #[test]
    fn test_compute_project_id_for_git_repo_with_remote() {
        let temp = TempDir::new().expect("create temp dir");
        let repo_dir = temp.path().join("my-project");
        std::fs::create_dir(&repo_dir).expect("create repo dir");

        init_git_repo(&repo_dir, Some("git@github.com:user/my-project.git"));

        let project_id = compute_project_id(&repo_dir).expect("compute project id");

        // ID should be 16 hex characters
        assert_eq!(project_id.id.len(), 16);
        assert!(project_id.id.chars().all(|c| c.is_ascii_hexdigit()));

        // Name should be the directory/repo name
        assert_eq!(project_id.name, "my-project");

        // Git remote should be captured
        assert_eq!(
            project_id.git_remote,
            Some("git@github.com:user/my-project.git".to_string())
        );

        // Git root should be the repo directory (canonicalized for macOS symlink handling)
        let expected_root = repo_dir.canonicalize().ok();
        assert_eq!(project_id.git_root, expected_root);

        // cwd should be preserved (canonicalized)
        assert_eq!(project_id.cwd, repo_dir.canonicalize().unwrap_or(repo_dir));
    }

    #[test]
    fn test_compute_project_id_for_git_repo_without_remote() {
        let temp = TempDir::new().expect("create temp dir");
        let repo_dir = temp.path().join("local-only");
        std::fs::create_dir(&repo_dir).expect("create repo dir");

        init_git_repo(&repo_dir, None);

        let project_id = compute_project_id(&repo_dir).expect("compute project id");

        // ID should be 16 hex characters
        assert_eq!(project_id.id.len(), 16);
        assert!(project_id.id.chars().all(|c| c.is_ascii_hexdigit()));

        // Name should be the directory name
        assert_eq!(project_id.name, "local-only");

        // No git remote
        assert_eq!(project_id.git_remote, None);

        // Git root should still be set (canonicalized for macOS symlink handling)
        let expected_root = repo_dir.canonicalize().ok();
        assert_eq!(project_id.git_root, expected_root);
    }

    #[test]
    fn test_compute_project_id_for_non_git_directory() {
        let temp = TempDir::new().expect("create temp dir");
        let dir = temp.path().join("plain-dir");
        std::fs::create_dir(&dir).expect("create dir");

        let project_id = compute_project_id(&dir).expect("compute project id");

        // ID should be 16 hex characters
        assert_eq!(project_id.id.len(), 16);
        assert!(project_id.id.chars().all(|c| c.is_ascii_hexdigit()));

        // Name should be the directory name
        assert_eq!(project_id.name, "plain-dir");

        // No git info
        assert_eq!(project_id.git_remote, None);
        assert_eq!(project_id.git_root, None);
    }

    #[test]
    fn test_compute_project_id_same_repo_different_subdirectory() {
        let temp = TempDir::new().expect("create temp dir");
        let repo_dir = temp.path().join("my-repo");
        std::fs::create_dir(&repo_dir).expect("create repo dir");

        init_git_repo(&repo_dir, Some("git@github.com:user/my-repo.git"));

        // Create subdirectories
        let subdir1 = repo_dir.join("src");
        let subdir2 = repo_dir.join("tests");
        std::fs::create_dir(&subdir1).expect("create src");
        std::fs::create_dir(&subdir2).expect("create tests");

        let id_from_root = compute_project_id(&repo_dir).expect("compute from root");
        let id_from_src = compute_project_id(&subdir1).expect("compute from src");
        let id_from_tests = compute_project_id(&subdir2).expect("compute from tests");

        // All should have the same project ID
        assert_eq!(id_from_root.id, id_from_src.id);
        assert_eq!(id_from_root.id, id_from_tests.id);

        // All should have same git root
        assert_eq!(id_from_root.git_root, id_from_src.git_root);
        assert_eq!(id_from_root.git_root, id_from_tests.git_root);

        // But cwd should differ (use canonicalized paths for macOS symlink handling)
        assert_eq!(
            id_from_root.cwd,
            repo_dir.canonicalize().unwrap_or(repo_dir.clone())
        );
        assert_eq!(
            id_from_src.cwd,
            subdir1.canonicalize().unwrap_or(subdir1.clone())
        );
        assert_eq!(
            id_from_tests.cwd,
            subdir2.canonicalize().unwrap_or(subdir2.clone())
        );
    }

    #[test]
    fn test_compute_project_id_is_deterministic() {
        let temp = TempDir::new().expect("create temp dir");
        let repo_dir = temp.path().join("stable-project");
        std::fs::create_dir(&repo_dir).expect("create repo dir");

        init_git_repo(&repo_dir, Some("git@github.com:user/stable.git"));

        let id1 = compute_project_id(&repo_dir).expect("first call");
        let id2 = compute_project_id(&repo_dir).expect("second call");

        assert_eq!(id1.id, id2.id);
    }

    #[test]
    fn test_different_remotes_produce_different_ids() {
        let temp = TempDir::new().expect("create temp dir");

        let repo1 = temp.path().join("repo1");
        let repo2 = temp.path().join("repo2");
        std::fs::create_dir(&repo1).expect("create repo1");
        std::fs::create_dir(&repo2).expect("create repo2");

        init_git_repo(&repo1, Some("git@github.com:user/project-a.git"));
        init_git_repo(&repo2, Some("git@github.com:user/project-b.git"));

        let id1 = compute_project_id(&repo1).expect("id1");
        let id2 = compute_project_id(&repo2).expect("id2");

        assert_ne!(id1.id, id2.id);
    }
}

mod types_serialization_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_transcript_line_serialization_roundtrip() {
        let line = TranscriptLine {
            ts: "2025-01-26T10:30:00.000Z".to_string(),
            v: 1,
            entry: TranscriptEntry::User(UserEntry {
                id: "msg-001".to_string(),
                content: "Hello, world!".to_string(),
            }),
        };

        let json = serde_json::to_string(&line).expect("serialize");
        let parsed: TranscriptLine = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(line, parsed);
    }

    #[test]
    fn test_session_meta_serialization() {
        let entry = TranscriptEntry::SessionMeta(SessionMetaEntry {
            session_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            project_id: "a1b2c3d4e5f67890".to_string(),
            started_at: "2025-01-26T10:30:00.000Z".to_string(),
            cwd: std::path::PathBuf::from("/home/user/project"),
            model: Some("claude-sonnet-4-20250514".to_string()),
            cli_version: "0.1.0".to_string(),
            git: Some(types::GitInfo {
                branch: Some("main".to_string()),
                commit_hash: Some("abc123".to_string()),
                remote_url: Some("git@github.com:user/project.git".to_string()),
            }),
        });

        let json = serde_json::to_string(&entry).expect("serialize");

        // Verify expected fields are present
        assert!(json.contains("\"type\":\"session_meta\""));
        assert!(json.contains("\"session_id\":\"550e8400"));
        assert!(json.contains("\"project_id\":\"a1b2c3d4e5f67890\""));
    }

    #[test]
    fn test_user_entry_serialization() {
        let entry = TranscriptEntry::User(UserEntry {
            id: "msg-001".to_string(),
            content: "What files are in src?".to_string(),
        });

        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"type\":\"user\""));
        assert!(json.contains("\"content\":\"What files are in src?\""));
    }

    #[test]
    fn test_assistant_entry_with_text_content() {
        let entry = TranscriptEntry::Assistant(AssistantEntry {
            id: "msg-002".to_string(),
            content: vec![ContentBlock::Text {
                text: "Here are the files...".to_string(),
            }],
            model: Some("claude-sonnet-4-20250514".to_string()),
        });

        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"type\":\"assistant\""));
        assert!(json.contains("\"text\":\"Here are the files...\""));
    }

    #[test]
    fn test_assistant_entry_with_thinking_content() {
        let entry = TranscriptEntry::Assistant(AssistantEntry {
            id: "msg-002".to_string(),
            content: vec![
                ContentBlock::Thinking {
                    thinking: "Let me analyze this...".to_string(),
                },
                ContentBlock::Text {
                    text: "The answer is 42.".to_string(),
                },
            ],
            model: None,
        });

        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"type\":\"thinking\""));
        assert!(json.contains("\"thinking\":\"Let me analyze this...\""));
    }

    #[test]
    fn test_tool_call_entry_serialization() {
        let entry = TranscriptEntry::ToolCall(ToolCallEntry {
            call_id: "call-001".to_string(),
            name: "shell".to_string(),
            input: serde_json::json!({"command": "ls -la"}),
        });

        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"type\":\"tool_call\""));
        assert!(json.contains("\"name\":\"shell\""));
        assert!(json.contains("\"command\":\"ls -la\""));
    }

    #[test]
    fn test_tool_result_entry_serialization() {
        let entry = TranscriptEntry::ToolResult(ToolResultEntry {
            call_id: "call-001".to_string(),
            output: "file1.rs\nfile2.rs".to_string(),
            truncated: false,
            exit_code: Some(0),
        });

        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"type\":\"tool_result\""));
        assert!(json.contains("\"exit_code\":0"));

        // truncated: false should be omitted
        assert!(!json.contains("\"truncated\""));
    }

    #[test]
    fn test_tool_result_with_truncated_output() {
        let entry = TranscriptEntry::ToolResult(ToolResultEntry {
            call_id: "call-001".to_string(),
            output: "partial output...".to_string(),
            truncated: true,
            exit_code: None,
        });

        let json = serde_json::to_string(&entry).expect("serialize");
        assert!(json.contains("\"truncated\":true"));
        // exit_code: None should be omitted
        assert!(!json.contains("\"exit_code\""));
    }

    #[test]
    fn test_full_transcript_line_format() {
        let line = TranscriptLine {
            ts: "2025-01-26T10:30:05.123Z".to_string(),
            v: 1,
            entry: TranscriptEntry::User(UserEntry {
                id: "msg-001".to_string(),
                content: "Hello".to_string(),
            }),
        };

        let json = serde_json::to_string(&line).expect("serialize");

        // Should have ts, v, and flattened entry
        assert!(json.contains("\"ts\":\"2025-01-26T10:30:05.123Z\""));
        assert!(json.contains("\"v\":1"));
        assert!(json.contains("\"type\":\"user\""));
    }
}

mod recorder_tests {
    use super::*;

    #[tokio::test]
    async fn test_recorder_creates_transcript_file() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        let recorder = TranscriptRecorder::new(&nori_home, &cwd, Some("test-model".to_string()))
            .await
            .expect("create recorder");

        // Transcript file should exist
        assert!(recorder.transcript_path().exists());

        // Should be in the expected location
        let path = recorder.transcript_path();
        assert!(path.starts_with(&nori_home));
        assert!(path.to_string_lossy().contains("transcripts"));
        assert!(path.extension().map(|e| e == "jsonl").unwrap_or(false));
    }

    #[tokio::test]
    async fn test_recorder_writes_session_meta_as_first_line() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        let recorder = TranscriptRecorder::new(&nori_home, &cwd, Some("test-model".to_string()))
            .await
            .expect("create recorder");

        recorder.flush().await.expect("flush");

        // Read the file and verify first line is session meta
        let content = std::fs::read_to_string(recorder.transcript_path()).expect("read file");
        let first_line = content.lines().next().expect("first line");
        let parsed: TranscriptLine = serde_json::from_str(first_line).expect("parse line");

        assert!(matches!(parsed.entry, TranscriptEntry::SessionMeta(_)));
        if let TranscriptEntry::SessionMeta(meta) = parsed.entry {
            assert_eq!(meta.session_id, recorder.session_id());
            assert_eq!(meta.project_id, recorder.project_id());
            assert_eq!(meta.model, Some("test-model".to_string()));
        }
    }

    #[tokio::test]
    async fn test_recorder_records_user_message() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        let recorder = TranscriptRecorder::new(&nori_home, &cwd, None)
            .await
            .expect("create recorder");

        recorder
            .record_user_message("Hello, assistant!")
            .await
            .expect("record user message");
        recorder.flush().await.expect("flush");

        let content = std::fs::read_to_string(recorder.transcript_path()).expect("read file");
        let lines: Vec<&str> = content.lines().collect();

        // Should have 2 lines: session meta + user message
        assert_eq!(lines.len(), 2);

        let user_line: TranscriptLine = serde_json::from_str(lines[1]).expect("parse user line");
        assert!(matches!(user_line.entry, TranscriptEntry::User(_)));
        if let TranscriptEntry::User(user) = user_line.entry {
            assert_eq!(user.content, "Hello, assistant!");
        }
    }

    #[tokio::test]
    async fn test_recorder_records_tool_call_and_result() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        let recorder = TranscriptRecorder::new(&nori_home, &cwd, None)
            .await
            .expect("create recorder");

        recorder
            .record_tool_call("call-123", "shell", &serde_json::json!({"command": "ls"}))
            .await
            .expect("record tool call");
        recorder
            .record_tool_result("call-123", "file1.rs\nfile2.rs", false, Some(0))
            .await
            .expect("record tool result");
        recorder.flush().await.expect("flush");

        let content = std::fs::read_to_string(recorder.transcript_path()).expect("read file");
        let lines: Vec<&str> = content.lines().collect();

        // Should have 3 lines: session meta + tool call + tool result
        assert_eq!(lines.len(), 3);

        let call_line: TranscriptLine = serde_json::from_str(lines[1]).expect("parse call line");
        assert!(matches!(call_line.entry, TranscriptEntry::ToolCall(_)));
        if let TranscriptEntry::ToolCall(call) = call_line.entry {
            assert_eq!(call.call_id, "call-123");
            assert_eq!(call.name, "shell");
        }

        let result_line: TranscriptLine =
            serde_json::from_str(lines[2]).expect("parse result line");
        assert!(matches!(result_line.entry, TranscriptEntry::ToolResult(_)));
        if let TranscriptEntry::ToolResult(result) = result_line.entry {
            assert_eq!(result.call_id, "call-123");
            assert_eq!(result.exit_code, Some(0));
        }
    }

    #[tokio::test]
    async fn test_recorder_records_assistant_message() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        let recorder = TranscriptRecorder::new(&nori_home, &cwd, None)
            .await
            .expect("create recorder");

        recorder
            .record_assistant_message(
                vec![ContentBlock::Text {
                    text: "Here is the answer.".to_string(),
                }],
                Some("claude-sonnet-4".to_string()),
            )
            .await
            .expect("record assistant message");
        recorder.flush().await.expect("flush");

        let content = std::fs::read_to_string(recorder.transcript_path()).expect("read file");
        let lines: Vec<&str> = content.lines().collect();

        assert_eq!(lines.len(), 2);

        let assistant_line: TranscriptLine =
            serde_json::from_str(lines[1]).expect("parse assistant line");
        assert!(matches!(
            assistant_line.entry,
            TranscriptEntry::Assistant(_)
        ));
        if let TranscriptEntry::Assistant(assistant) = assistant_line.entry {
            assert_eq!(assistant.content.len(), 1);
            assert_eq!(assistant.model, Some("claude-sonnet-4".to_string()));
        }
    }

    #[tokio::test]
    async fn test_recorder_full_conversation_flow() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        let recorder =
            TranscriptRecorder::new(&nori_home, &cwd, Some("claude-sonnet-4".to_string()))
                .await
                .expect("create recorder");

        // Simulate a conversation
        recorder
            .record_user_message("List files in src/")
            .await
            .expect("record user");
        recorder
            .record_tool_call(
                "call-1",
                "shell",
                &serde_json::json!({"command": "ls src/"}),
            )
            .await
            .expect("record call");
        recorder
            .record_tool_result("call-1", "main.rs\nlib.rs", false, Some(0))
            .await
            .expect("record result");
        recorder
            .record_assistant_message(
                vec![ContentBlock::Text {
                    text: "The src/ directory contains main.rs and lib.rs.".to_string(),
                }],
                None,
            )
            .await
            .expect("record assistant");

        recorder.shutdown().await.expect("shutdown");

        let content = std::fs::read_to_string(recorder.transcript_path()).expect("read file");
        let lines: Vec<&str> = content.lines().collect();

        // session_meta + user + tool_call + tool_result + assistant = 5 lines
        assert_eq!(lines.len(), 5);

        // Verify each line parses correctly
        for line in lines {
            let _parsed: TranscriptLine = serde_json::from_str(line).expect("parse line");
        }
    }

    #[tokio::test]
    async fn test_recorder_uses_correct_project_directory() {
        use std::process::Command;

        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let project_dir = temp.path().join("my-awesome-project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&project_dir).expect("create project_dir");

        // Initialize as git repo with remote
        Command::new("git")
            .args(["init"])
            .current_dir(&project_dir)
            .output()
            .expect("git init");
        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "git@github.com:user/my-project.git",
            ])
            .current_dir(&project_dir)
            .output()
            .expect("git remote add");

        let recorder = TranscriptRecorder::new(&nori_home, &project_dir, None)
            .await
            .expect("create recorder");

        // Transcript path should include project ID directory
        let path = recorder.transcript_path();
        assert!(path.to_string_lossy().contains("by-project"));

        // Project ID should be computed from git remote
        assert_eq!(recorder.project_id().len(), 16);
    }
}

mod loader_tests {
    use super::*;

    /// Helper to create a test transcript file with some entries.
    async fn create_test_transcript(
        nori_home: &std::path::Path,
        cwd: &std::path::Path,
    ) -> TranscriptRecorder {
        let recorder = TranscriptRecorder::new(nori_home, cwd, Some("test-model".to_string()))
            .await
            .expect("create recorder");

        recorder
            .record_user_message("Hello!")
            .await
            .expect("record user");
        recorder
            .record_assistant_message(
                vec![ContentBlock::Text {
                    text: "Hi there!".to_string(),
                }],
                None,
            )
            .await
            .expect("record assistant");
        recorder.flush().await.expect("flush");

        recorder
    }

    #[tokio::test]
    async fn test_loader_list_projects_empty() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");

        let loader = TranscriptLoader::new(nori_home);
        let projects = loader.list_projects().await.expect("list projects");

        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn test_loader_list_projects_with_transcripts() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        // Create a transcript
        let recorder = create_test_transcript(&nori_home, &cwd).await;
        let project_id = recorder.project_id().to_string();

        let loader = TranscriptLoader::new(nori_home);
        let projects = loader.list_projects().await.expect("list projects");

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, project_id);
        assert_eq!(projects[0].session_count, 1);
    }

    #[tokio::test]
    async fn test_loader_list_sessions() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        // Create two transcripts for the same project
        let recorder1 = create_test_transcript(&nori_home, &cwd).await;
        let project_id = recorder1.project_id().to_string();
        let session1_id = recorder1.session_id().to_string();

        let recorder2 = create_test_transcript(&nori_home, &cwd).await;
        let session2_id = recorder2.session_id().to_string();

        let loader = TranscriptLoader::new(nori_home);
        let sessions = loader
            .list_sessions(&project_id)
            .await
            .expect("list sessions");

        assert_eq!(sessions.len(), 2);

        let session_ids: Vec<&str> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert!(session_ids.contains(&session1_id.as_str()));
        assert!(session_ids.contains(&session2_id.as_str()));
    }

    #[tokio::test]
    async fn test_loader_load_transcript() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        let recorder = create_test_transcript(&nori_home, &cwd).await;
        let project_id = recorder.project_id().to_string();
        let session_id = recorder.session_id().to_string();

        let loader = TranscriptLoader::new(nori_home);
        let transcript = loader
            .load_transcript(&project_id, &session_id)
            .await
            .expect("load transcript");

        // Should have session_meta + user + assistant = 3 entries
        assert_eq!(transcript.entries.len(), 3);
        assert_eq!(transcript.meta.session_id, session_id);
        assert_eq!(transcript.meta.project_id, project_id);
    }

    #[tokio::test]
    async fn test_loader_load_session_meta() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        let recorder = create_test_transcript(&nori_home, &cwd).await;
        let project_id = recorder.project_id().to_string();
        let session_id = recorder.session_id().to_string();

        let loader = TranscriptLoader::new(nori_home);
        let meta = loader
            .load_session_meta(&project_id, &session_id)
            .await
            .expect("load session meta");

        assert_eq!(meta.session_id, session_id);
        assert_eq!(meta.model, Some("test-model".to_string()));
    }

    #[tokio::test]
    async fn test_loader_find_sessions_for_cwd() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let project1 = temp.path().join("project1");
        let project2 = temp.path().join("project2");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&project1).expect("create project1");
        std::fs::create_dir_all(&project2).expect("create project2");

        // Create transcripts for different projects
        let recorder1 = create_test_transcript(&nori_home, &project1).await;
        let _recorder2 = create_test_transcript(&nori_home, &project2).await;

        let loader = TranscriptLoader::new(nori_home);

        // Find sessions for project1
        let sessions = loader
            .find_sessions_for_cwd(&project1)
            .await
            .expect("find sessions");

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, recorder1.session_id());
    }

    #[tokio::test]
    async fn test_loader_nonexistent_project() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");

        let loader = TranscriptLoader::new(nori_home);
        let sessions = loader
            .list_sessions("nonexistent-project-id")
            .await
            .expect("list sessions");

        assert!(sessions.is_empty());
    }
}

mod integration_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Full integration test: create a transcript, close it, then load it back.
    /// This verifies the entire flow from recording to loading.
    #[tokio::test]
    async fn test_full_roundtrip_record_and_load() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("my-project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        // Record a full conversation
        let recorder =
            TranscriptRecorder::new(&nori_home, &cwd, Some("claude-sonnet-4".to_string()))
                .await
                .expect("create recorder");

        let project_id = recorder.project_id().to_string();
        let session_id = recorder.session_id().to_string();

        // User asks a question
        recorder
            .record_user_message("What is in the current directory?")
            .await
            .expect("record user");

        // Agent calls a tool
        recorder
            .record_tool_call(
                "call-001",
                "shell",
                &serde_json::json!({"command": "ls -la"}),
            )
            .await
            .expect("record tool call");

        // Tool returns result
        recorder
            .record_tool_result(
                "call-001",
                "total 8\n-rw-r--r-- 1 user user 1234 Jan 26 10:00 README.md",
                false,
                Some(0),
            )
            .await
            .expect("record tool result");

        // Agent responds
        recorder
            .record_assistant_message(
                vec![
                    ContentBlock::Thinking {
                        thinking: "Let me analyze the directory contents...".to_string(),
                    },
                    ContentBlock::Text {
                        text: "The current directory contains a README.md file.".to_string(),
                    },
                ],
                Some("claude-sonnet-4".to_string()),
            )
            .await
            .expect("record assistant");

        // Close the recorder
        recorder.shutdown().await.expect("shutdown");

        // Now load it back
        let loader = TranscriptLoader::new(nori_home.clone());

        // Verify project is listed
        let projects = loader.list_projects().await.expect("list projects");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, project_id);
        assert_eq!(projects[0].session_count, 1);

        // Verify session is listed
        let sessions = loader
            .list_sessions(&project_id)
            .await
            .expect("list sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, session_id);
        assert_eq!(sessions[0].model, Some("claude-sonnet-4".to_string()));

        // Load full transcript
        let transcript = loader
            .load_transcript(&project_id, &session_id)
            .await
            .expect("load transcript");

        // Verify structure: meta + user + tool_call + tool_result + assistant = 5 entries
        assert_eq!(transcript.entries.len(), 5);
        assert_eq!(transcript.meta.session_id, session_id);
        assert_eq!(transcript.meta.model, Some("claude-sonnet-4".to_string()));

        // Verify entry types in order
        assert!(matches!(
            transcript.entries[0].entry,
            TranscriptEntry::SessionMeta(_)
        ));
        assert!(matches!(
            transcript.entries[1].entry,
            TranscriptEntry::User(_)
        ));
        assert!(matches!(
            transcript.entries[2].entry,
            TranscriptEntry::ToolCall(_)
        ));
        assert!(matches!(
            transcript.entries[3].entry,
            TranscriptEntry::ToolResult(_)
        ));
        assert!(matches!(
            transcript.entries[4].entry,
            TranscriptEntry::Assistant(_)
        ));

        // Verify user message content
        if let TranscriptEntry::User(user) = &transcript.entries[1].entry {
            assert_eq!(user.content, "What is in the current directory?");
        }

        // Verify assistant has both thinking and text blocks
        if let TranscriptEntry::Assistant(assistant) = &transcript.entries[4].entry {
            assert_eq!(assistant.content.len(), 2);
            assert!(matches!(
                assistant.content[0],
                ContentBlock::Thinking { .. }
            ));
            assert!(matches!(assistant.content[1], ContentBlock::Text { .. }));
        }
    }

    /// Test that multiple sessions from the same project are tracked correctly.
    #[tokio::test]
    async fn test_multiple_sessions_same_project() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&cwd).expect("create cwd");

        // Create 3 sessions
        let mut session_ids = Vec::new();
        for i in 0..3 {
            let recorder = TranscriptRecorder::new(&nori_home, &cwd, None)
                .await
                .expect("create recorder");
            session_ids.push(recorder.session_id().to_string());
            recorder
                .record_user_message(&format!("Message in session {i}"))
                .await
                .expect("record");
            recorder.shutdown().await.expect("shutdown");
        }

        let loader = TranscriptLoader::new(nori_home);

        // Should have 1 project with 3 sessions
        let projects = loader.list_projects().await.expect("list projects");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].session_count, 3);

        // All sessions should be loadable
        let sessions = loader
            .list_sessions(&projects[0].id)
            .await
            .expect("list sessions");
        assert_eq!(sessions.len(), 3);

        for session_id in &session_ids {
            let transcript = loader
                .load_transcript(&projects[0].id, session_id)
                .await
                .expect("load transcript");
            assert_eq!(transcript.meta.session_id, *session_id);
        }
    }

    /// Test finding sessions by cwd works across different paths.
    #[tokio::test]
    async fn test_find_sessions_by_cwd_isolation() {
        let temp = TempDir::new().expect("create temp dir");
        let nori_home = temp.path().join("nori_home");
        let project_a = temp.path().join("project-a");
        let project_b = temp.path().join("project-b");
        std::fs::create_dir_all(&nori_home).expect("create nori_home");
        std::fs::create_dir_all(&project_a).expect("create project_a");
        std::fs::create_dir_all(&project_b).expect("create project_b");

        // Create sessions in different projects
        let recorder_a = TranscriptRecorder::new(&nori_home, &project_a, None)
            .await
            .expect("create recorder a");
        let session_a = recorder_a.session_id().to_string();
        recorder_a
            .record_user_message("In project A")
            .await
            .expect("record");
        recorder_a.shutdown().await.expect("shutdown");

        let recorder_b = TranscriptRecorder::new(&nori_home, &project_b, None)
            .await
            .expect("create recorder b");
        let session_b = recorder_b.session_id().to_string();
        recorder_b
            .record_user_message("In project B")
            .await
            .expect("record");
        recorder_b.shutdown().await.expect("shutdown");

        let loader = TranscriptLoader::new(nori_home);

        // Find sessions for project A - should only return A's session
        let sessions_a = loader
            .find_sessions_for_cwd(&project_a)
            .await
            .expect("find sessions a");
        assert_eq!(sessions_a.len(), 1);
        assert_eq!(sessions_a[0].session_id, session_a);

        // Find sessions for project B - should only return B's session
        let sessions_b = loader
            .find_sessions_for_cwd(&project_b)
            .await
            .expect("find sessions b");
        assert_eq!(sessions_b.len(), 1);
        assert_eq!(sessions_b[0].session_id, session_b);
    }
}
