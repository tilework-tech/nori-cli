//! Project key resolution for grouping transcripts by project.
//!
//! The project key is derived from the git repository root (if available) or
//! the current working directory. This allows session transcripts to be
//! grouped by project, similar to how Claude organizes sessions under
//! `~/.claude/projects/<PROJECT_PATH>/`.
//!
//! The key is a SHA-256 hash (truncated to 12 characters) of the canonical
//! project path, ensuring:
//! - Consistent, filesystem-safe directory names
//! - No collisions across different projects
//! - Worktree-aware resolution (worktrees map to their main repository)

use sha2::Digest;
use sha2::Sha256;
use std::path::Path;
use std::path::PathBuf;

/// Resolve the project root path from a working directory.
///
/// Resolution order:
/// 1. Try worktree-aware git root (`resolve_root_git_project_for_trust`)
/// 2. Fall back to simple git root detection (`get_git_repo_root`)
/// 3. Ultimate fallback to the working directory itself
///
/// Paths are canonicalized to handle symlinks and ensure consistency.
pub fn resolve_project_root(cwd: &Path) -> PathBuf {
    // Try worktree-aware resolution first (handles git worktrees properly)
    if let Some(root) = codex_core::git_info::resolve_root_git_project_for_trust(cwd) {
        return root;
    }

    // Fall back to simple git root detection
    if let Some(root) = codex_core::git_info::get_git_repo_root(cwd) {
        // Canonicalize to handle symlinks
        return std::fs::canonicalize(&root).unwrap_or(root);
    }

    // Not in a git repository, use cwd directly
    std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf())
}

/// Compute a project key from a working directory.
///
/// The key is a SHA-256 hash (truncated to 12 hex characters) of the
/// project root path. This provides:
/// - Filesystem-safe directory names
/// - Consistent keys for the same project
/// - Privacy (the original path is not directly visible)
///
/// # Arguments
/// * `cwd` - The current working directory
///
/// # Returns
/// A 12-character hex string representing the project key
pub fn project_key_from_cwd(cwd: &Path) -> String {
    let project_root = resolve_project_root(cwd);
    hash_path(&project_root)
}

/// Hash a path to produce a 12-character hex key.
fn hash_path(path: &Path) -> String {
    let path_str = path.to_string_lossy();
    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let result = hasher.finalize();
    // Take first 6 bytes (12 hex chars)
    hex::encode(&result[..6])
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_project_key_from_cwd_non_git_directory() {
        let temp_dir = TempDir::new().expect("create temp dir");

        let key = project_key_from_cwd(temp_dir.path());

        // Key should be a 12-character hex string
        assert_eq!(key.len(), 12);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_project_key_consistent_for_same_path() {
        let temp_dir = TempDir::new().expect("create temp dir");

        let key1 = project_key_from_cwd(temp_dir.path());
        let key2 = project_key_from_cwd(temp_dir.path());

        assert_eq!(key1, key2, "same path should produce same key");
    }

    #[test]
    fn test_project_key_different_for_different_paths() {
        let temp_dir1 = TempDir::new().expect("create temp dir 1");
        let temp_dir2 = TempDir::new().expect("create temp dir 2");

        let key1 = project_key_from_cwd(temp_dir1.path());
        let key2 = project_key_from_cwd(temp_dir2.path());

        assert_ne!(key1, key2, "different paths should produce different keys");
    }

    #[test]
    fn test_project_key_from_nested_non_git_directory() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let nested = temp_dir.path().join("a").join("b").join("c");
        fs::create_dir_all(&nested).expect("create nested dirs");

        let key_root = project_key_from_cwd(temp_dir.path());
        let key_nested = project_key_from_cwd(&nested);

        // Without git, nested dirs should have different keys (based on their own path)
        assert_ne!(
            key_root, key_nested,
            "without git, nested dir should have different key"
        );
    }

    #[tokio::test]
    async fn test_project_key_from_git_root() {
        // Skip if git is not available or sandbox prevents execution
        if std::env::var("CODEX_SANDBOX_NETWORK_DISABLED").is_ok() {
            return;
        }

        let temp_dir = TempDir::new().expect("create temp dir");
        let repo_path = temp_dir.path().join("repo");
        fs::create_dir(&repo_path).expect("create repo dir");

        // Initialize git repo
        let output = tokio::process::Command::new("git")
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .await;

        if output.is_err() || !output.as_ref().unwrap().status.success() {
            // Git not available, skip test
            return;
        }

        // Create nested directory
        let nested = repo_path.join("src").join("lib");
        fs::create_dir_all(&nested).expect("create nested dirs");

        let key_root = project_key_from_cwd(&repo_path);
        let key_nested = project_key_from_cwd(&nested);

        // With git, both should resolve to the same project key
        assert_eq!(
            key_root, key_nested,
            "git repo root and nested dir should have same key"
        );
    }

    #[tokio::test]
    async fn test_project_key_handles_worktrees() {
        // Skip if git is not available or sandbox prevents execution
        if std::env::var("CODEX_SANDBOX_NETWORK_DISABLED").is_ok() {
            return;
        }

        let temp_dir = TempDir::new().expect("create temp dir");
        let repo_path = temp_dir.path().join("repo");
        fs::create_dir(&repo_path).expect("create repo dir");

        // Initialize git repo with initial commit (required for worktrees)
        let envs = vec![
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
        ];

        let init_output = tokio::process::Command::new("git")
            .envs(envs.clone())
            .args(["init"])
            .current_dir(&repo_path)
            .output()
            .await;

        if init_output.is_err() || !init_output.as_ref().unwrap().status.success() {
            return;
        }

        // Configure git user
        let _ = tokio::process::Command::new("git")
            .envs(envs.clone())
            .args(["config", "user.name", "Test"])
            .current_dir(&repo_path)
            .output()
            .await;
        let _ = tokio::process::Command::new("git")
            .envs(envs.clone())
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&repo_path)
            .output()
            .await;

        // Create initial commit
        fs::write(repo_path.join("README.md"), "test").expect("write file");
        let _ = tokio::process::Command::new("git")
            .envs(envs.clone())
            .args(["add", "."])
            .current_dir(&repo_path)
            .output()
            .await;
        let commit_output = tokio::process::Command::new("git")
            .envs(envs.clone())
            .args(["commit", "-m", "Initial"])
            .current_dir(&repo_path)
            .output()
            .await;

        if commit_output.is_err() || !commit_output.as_ref().unwrap().status.success() {
            return;
        }

        // Create worktree
        let wt_path = temp_dir.path().join("worktree");
        let wt_output = tokio::process::Command::new("git")
            .envs(envs)
            .args([
                "worktree",
                "add",
                wt_path.to_str().unwrap(),
                "-b",
                "feature",
            ])
            .current_dir(&repo_path)
            .output()
            .await;

        if wt_output.is_err() || !wt_output.as_ref().unwrap().status.success() {
            return;
        }

        let key_main = project_key_from_cwd(&repo_path);
        let key_worktree = project_key_from_cwd(&wt_path);

        // Both should resolve to the same main repository
        assert_eq!(
            key_main, key_worktree,
            "main repo and worktree should have same project key"
        );
    }

    #[test]
    fn test_hash_path_produces_consistent_output() {
        let path = Path::new("/some/test/path");
        let hash1 = hash_path(path);
        let hash2 = hash_path(path);

        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 12);
    }

    #[test]
    fn test_resolve_project_root_non_git() {
        let temp_dir = TempDir::new().expect("create temp dir");
        let nested = temp_dir.path().join("nested");
        fs::create_dir(&nested).expect("create nested dir");

        let root = resolve_project_root(&nested);

        // Should return the canonicalized nested path (not temp_dir root)
        let expected = std::fs::canonicalize(&nested).unwrap();
        assert_eq!(root, expected);
    }
}
