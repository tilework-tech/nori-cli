//! Tests for Nori transcript persistence.

use std::path::PathBuf;

use serde_json::json;
use tempfile::TempDir;
use tokio::process::Command;

use super::*;

mod types_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_transcript_line_serialization() {
        let line = TranscriptLine {
            ts: "2025-01-26T10:30:00.000Z".to_string(),
            v: 1,
            entry: TranscriptEntry::User(UserEntry {
                id: "msg-001".to_string(),
                content: "Hello, world!".to_string(),
                attachments: vec![],
            }),
        };

        let json = serde_json::to_string(&line).unwrap();
        let parsed: TranscriptLine = serde_json::from_str(&json).unwrap();

        assert_eq!(line, parsed);
    }

    #[test]
    fn test_user_entry_serialization() {
        let entry = TranscriptEntry::User(UserEntry {
            id: "msg-001".to_string(),
            content: "What files are in the src directory?".to_string(),
            attachments: vec![],
        });

        let line = TranscriptLine {
            ts: "2025-01-26T10:30:05.123Z".to_string(),
            v: 1,
            entry,
        };

        let json = serde_json::to_string(&line).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "user");
        assert_eq!(parsed["id"], "msg-001");
        assert_eq!(parsed["content"], "What files are in the src directory?");
        // Empty attachments should not be serialized
        assert!(parsed.get("attachments").is_none());
    }

    #[test]
    fn test_user_entry_with_attachments() {
        let entry = TranscriptEntry::User(UserEntry {
            id: "msg-002".to_string(),
            content: "Check this image".to_string(),
            attachments: vec![Attachment::FilePath {
                path: PathBuf::from("/tmp/screenshot.png"),
            }],
        });

        let line = TranscriptLine {
            ts: "2025-01-26T10:30:05.123Z".to_string(),
            v: 1,
            entry,
        };

        let json = serde_json::to_string(&line).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert!(parsed.get("attachments").is_some());
        assert_eq!(parsed["attachments"][0]["type"], "file_path");
    }

    #[test]
    fn test_assistant_entry_serialization() {
        let entry = TranscriptEntry::Assistant(AssistantEntry {
            id: "msg-002".to_string(),
            content: vec![ContentBlock::Text {
                text: "The src directory contains main.rs and lib.rs.".to_string(),
            }],
            agent: Some("claude-code".to_string()),
        });

        let line = TranscriptLine {
            ts: "2025-01-26T10:30:08.012Z".to_string(),
            v: 1,
            entry,
        };

        let json = serde_json::to_string(&line).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "assistant");
        assert_eq!(parsed["id"], "msg-002");
        assert_eq!(parsed["content"][0]["type"], "text");
        assert_eq!(
            parsed["content"][0]["text"],
            "The src directory contains main.rs and lib.rs."
        );
        assert_eq!(parsed["agent"], "claude-code");
    }

    #[test]
    fn test_tool_call_entry_serialization() {
        let entry = TranscriptEntry::ToolCall(ToolCallEntry {
            call_id: "call-001".to_string(),
            name: "shell".to_string(),
            input: json!({"command": "ls -la src/"}),
        });

        let line = TranscriptLine {
            ts: "2025-01-26T10:30:06.456Z".to_string(),
            v: 1,
            entry,
        };

        let json = serde_json::to_string(&line).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "tool_call");
        assert_eq!(parsed["call_id"], "call-001");
        assert_eq!(parsed["name"], "shell");
        assert_eq!(parsed["input"]["command"], "ls -la src/");
    }

    #[test]
    fn test_tool_result_entry_serialization() {
        let entry = TranscriptEntry::ToolResult(ToolResultEntry {
            call_id: "call-001".to_string(),
            output: "main.rs\nlib.rs".to_string(),
            truncated: false,
            exit_code: Some(0),
        });

        let line = TranscriptLine {
            ts: "2025-01-26T10:30:07.789Z".to_string(),
            v: 1,
            entry,
        };

        let json = serde_json::to_string(&line).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "tool_result");
        assert_eq!(parsed["call_id"], "call-001");
        assert_eq!(parsed["output"], "main.rs\nlib.rs");
        // truncated=false should not be serialized
        assert!(parsed.get("truncated").is_none());
        assert_eq!(parsed["exit_code"], 0);
    }

    #[test]
    fn test_tool_result_truncated() {
        let entry = TranscriptEntry::ToolResult(ToolResultEntry {
            call_id: "call-002".to_string(),
            output: "[truncated output]".to_string(),
            truncated: true,
            exit_code: None,
        });

        let line = TranscriptLine {
            ts: "2025-01-26T10:30:07.789Z".to_string(),
            v: 1,
            entry,
        };

        let json = serde_json::to_string(&line).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["truncated"], true);
        // exit_code=None should not be serialized
        assert!(parsed.get("exit_code").is_none());
    }

    #[test]
    fn test_patch_apply_entry_serialization() {
        let entry = TranscriptEntry::PatchApply(PatchApplyEntry {
            call_id: "call-003".to_string(),
            operation: PatchOperationType::Edit,
            path: PathBuf::from("/src/main.rs"),
            success: true,
            error: None,
        });

        let line = TranscriptLine {
            ts: "2025-01-26T10:30:07.789Z".to_string(),
            v: 1,
            entry,
        };

        let json = serde_json::to_string(&line).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "patch_apply");
        assert_eq!(parsed["call_id"], "call-003");
        assert_eq!(parsed["operation"], "edit");
        assert_eq!(parsed["path"], "/src/main.rs");
        assert_eq!(parsed["success"], true);
        // error=None should not be serialized
        assert!(parsed.get("error").is_none());
    }

    #[test]
    fn test_patch_apply_with_error() {
        let entry = TranscriptEntry::PatchApply(PatchApplyEntry {
            call_id: "call-004".to_string(),
            operation: PatchOperationType::Write,
            path: PathBuf::from("/src/new_file.rs"),
            success: false,
            error: Some("Permission denied".to_string()),
        });

        let line = TranscriptLine {
            ts: "2025-01-26T10:30:07.789Z".to_string(),
            v: 1,
            entry,
        };

        let json = serde_json::to_string(&line).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "patch_apply");
        assert_eq!(parsed["operation"], "write");
        assert_eq!(parsed["success"], false);
        assert_eq!(parsed["error"], "Permission denied");
    }

    #[test]
    fn test_session_meta_entry_serialization() {
        let entry = TranscriptEntry::SessionMeta(SessionMetaEntry {
            session_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            project_id: "a1b2c3d4e5f67890".to_string(),
            started_at: "2025-01-26T10:30:00.000Z".to_string(),
            cwd: PathBuf::from("/home/user/projects/nori-cli"),
            agent: Some("claude-code".to_string()),
            cli_version: "0.1.0".to_string(),
            git: Some(GitInfo {
                branch: Some("main".to_string()),
                commit_hash: Some("abc123def456".to_string()),
            }),
        });

        let line = TranscriptLine {
            ts: "2025-01-26T10:30:00.000Z".to_string(),
            v: 1,
            entry,
        };

        let json = serde_json::to_string(&line).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["type"], "session_meta");
        assert_eq!(parsed["session_id"], "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(parsed["project_id"], "a1b2c3d4e5f67890");
        assert_eq!(parsed["cli_version"], "0.1.0");
        assert_eq!(parsed["git"]["branch"], "main");
        assert_eq!(parsed["git"]["commit_hash"], "abc123def456");
    }

    #[test]
    fn test_session_meta_without_git() {
        let entry = TranscriptEntry::SessionMeta(SessionMetaEntry {
            session_id: "test-session".to_string(),
            project_id: "test-project".to_string(),
            started_at: "2025-01-26T10:30:00.000Z".to_string(),
            cwd: PathBuf::from("/tmp/no-git"),
            agent: None,
            cli_version: "0.1.0".to_string(),
            git: None,
        });

        let line = TranscriptLine {
            ts: "2025-01-26T10:30:00.000Z".to_string(),
            v: 1,
            entry,
        };

        let json = serde_json::to_string(&line).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        // git and agent should not be serialized when None
        assert!(parsed.get("git").is_none());
        assert!(parsed.get("agent").is_none());
    }

    #[test]
    fn test_transcript_line_deserialization_from_jsonl() {
        // Test deserializing from the example JSONL format in the spec
        let jsonl_line = r#"{"ts":"2025-01-26T10:30:05.123Z","v":1,"type":"user","id":"msg-001","content":"What files are in the src directory?"}"#;

        let parsed: TranscriptLine = serde_json::from_str(jsonl_line).unwrap();

        assert_eq!(parsed.ts, "2025-01-26T10:30:05.123Z");
        assert_eq!(parsed.v, 1);

        if let TranscriptEntry::User(user) = parsed.entry {
            assert_eq!(user.id, "msg-001");
            assert_eq!(user.content, "What files are in the src directory?");
        } else {
            panic!("Expected User entry");
        }
    }

    #[test]
    fn test_transcript_line_new_creates_timestamp() {
        let entry = TranscriptEntry::User(UserEntry {
            id: "test".to_string(),
            content: "test".to_string(),
            attachments: vec![],
        });

        let line = TranscriptLine::new(entry);

        // Timestamp should be in ISO 8601 format
        assert!(line.ts.contains('T'));
        assert!(line.ts.ends_with('Z'));
        assert_eq!(line.v, types::SCHEMA_VERSION);
    }
}

mod project_tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // Helper to create a test git repository
    async fn create_test_git_repo(temp_dir: &TempDir) -> PathBuf {
        let repo_path = temp_dir.path().join("repo");
        std::fs::create_dir(&repo_path).expect("Failed to create repo dir");

        let envs = vec![
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
        ];

        Command::new("git")
            .envs(envs.clone())
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .await
            .expect("Failed to init git repo");

        Command::new("git")
            .envs(envs.clone())
            .args(["config", "user.name", "Test User"])
            .current_dir(&repo_path)
            .output()
            .await
            .expect("Failed to set git user name");

        Command::new("git")
            .envs(envs.clone())
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&repo_path)
            .output()
            .await
            .expect("Failed to set git user email");

        // Create and commit a file
        std::fs::write(repo_path.join("test.txt"), "test content").unwrap();

        Command::new("git")
            .envs(envs.clone())
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .await
            .expect("Failed to add files");

        Command::new("git")
            .envs(envs)
            .args(["commit", "-m", "Initial commit"])
            .current_dir(&repo_path)
            .output()
            .await
            .expect("Failed to commit");

        repo_path
    }

    #[tokio::test]
    async fn test_compute_project_id_non_git_directory() {
        let temp_dir = TempDir::new().unwrap();

        let project_id = compute_project_id(temp_dir.path()).await.unwrap();

        // Should have a 16-char hex ID
        assert_eq!(project_id.id.len(), 16);
        assert!(project_id.id.chars().all(|c| c.is_ascii_hexdigit()));

        // Should have no git info
        assert!(project_id.git_remote.is_none());
        assert!(project_id.git_root.is_none());
    }

    #[tokio::test]
    async fn test_compute_project_id_git_repo_without_remote() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_git_repo(&temp_dir).await;

        let project_id = compute_project_id(&repo_path).await.unwrap();

        // Should have a 16-char hex ID
        assert_eq!(project_id.id.len(), 16);

        // Should have git root but no remote
        assert!(project_id.git_root.is_some());
        assert!(project_id.git_remote.is_none());

        // Name should be the directory name
        assert_eq!(project_id.name, "repo");
    }

    #[tokio::test]
    async fn test_compute_project_id_git_repo_with_remote() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_git_repo(&temp_dir).await;

        // Add a remote
        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/user/my-project.git",
            ])
            .current_dir(&repo_path)
            .output()
            .await
            .expect("Failed to add remote");

        let project_id = compute_project_id(&repo_path).await.unwrap();

        // Should have git remote
        assert!(project_id.git_remote.is_some());
        assert!(
            project_id
                .git_remote
                .as_ref()
                .unwrap()
                .contains("my-project")
        );

        // Name should be extracted from remote URL
        assert_eq!(project_id.name, "my-project");
    }

    #[tokio::test]
    async fn test_compute_project_id_same_repo_different_subdirectory() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = create_test_git_repo(&temp_dir).await;

        // Create a subdirectory
        let subdir = repo_path.join("src/nested");
        std::fs::create_dir_all(&subdir).unwrap();

        let id_root = compute_project_id(&repo_path).await.unwrap();
        let id_subdir = compute_project_id(&subdir).await.unwrap();

        // Same repository should produce same project ID
        assert_eq!(id_root.id, id_subdir.id);
        assert_eq!(id_root.name, id_subdir.name);
    }

    #[tokio::test]
    async fn test_compute_project_id_different_repos_different_ids() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let repo1 = create_test_git_repo(&temp_dir1).await;
        let repo2 = create_test_git_repo(&temp_dir2).await;

        let id1 = compute_project_id(&repo1).await.unwrap();
        let id2 = compute_project_id(&repo2).await.unwrap();

        // Different repositories should have different IDs
        assert_ne!(id1.id, id2.id);
    }

    #[tokio::test]
    async fn test_compute_project_id_same_remote_same_id() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let repo1 = create_test_git_repo(&temp_dir1).await;
        let repo2 = create_test_git_repo(&temp_dir2).await;

        // Add same remote to both
        for repo in [&repo1, &repo2] {
            Command::new("git")
                .args([
                    "remote",
                    "add",
                    "origin",
                    "https://github.com/user/shared-project.git",
                ])
                .current_dir(repo)
                .output()
                .await
                .expect("Failed to add remote");
        }

        let id1 = compute_project_id(&repo1).await.unwrap();
        let id2 = compute_project_id(&repo2).await.unwrap();

        // Same remote URL should produce same project ID
        assert_eq!(id1.id, id2.id);
    }

    #[tokio::test]
    async fn test_compute_project_id_ssh_and_https_same_id() {
        let temp_dir1 = TempDir::new().unwrap();
        let temp_dir2 = TempDir::new().unwrap();

        let repo1 = create_test_git_repo(&temp_dir1).await;
        let repo2 = create_test_git_repo(&temp_dir2).await;

        // Add SSH remote to repo1
        Command::new("git")
            .args(["remote", "add", "origin", "git@github.com:user/project.git"])
            .current_dir(&repo1)
            .output()
            .await
            .expect("Failed to add SSH remote");

        // Add HTTPS remote to repo2
        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                "https://github.com/user/project.git",
            ])
            .current_dir(&repo2)
            .output()
            .await
            .expect("Failed to add HTTPS remote");

        let id1 = compute_project_id(&repo1).await.unwrap();
        let id2 = compute_project_id(&repo2).await.unwrap();

        // SSH and HTTPS URLs for same repo should produce same project ID
        assert_eq!(id1.id, id2.id);
    }

    #[test]
    fn test_project_id_deterministic() {
        use crate::transcript::project::compute_hash;
        // Project ID computation should be deterministic
        let hash1 = compute_hash("test-input");
        let hash2 = compute_hash("test-input");

        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 16);
        assert!(hash1.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
