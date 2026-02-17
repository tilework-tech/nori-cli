//! Project identification for transcript organization.
//!
//! Projects are identified by a hash-based ID computed from:
//! 1. Git repository with remote: SHA-256 hash of the canonical remote URL
//! 2. Git repository without remote: SHA-256 hash of the git root absolute path
//! 3. No git: SHA-256 hash of the working directory absolute path

use std::io;
use std::path::Path;
use std::path::PathBuf;

use tokio::process::Command;
use tokio::time::Duration;
use tokio::time::timeout;

/// Timeout for git commands
const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Project identification result.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectId {
    /// The hash-based identifier (16 hex chars)
    pub id: String,
    /// Human-readable project name (directory name or repo name)
    pub name: String,
    /// Git remote URL if available
    pub git_remote: Option<String>,
    /// Git root path if in a git repo
    pub git_root: Option<PathBuf>,
    /// The original cwd
    pub cwd: PathBuf,
}

/// Compute project ID from working directory.
///
/// The project ID is computed as follows:
/// 1. If in a git repo with a remote: SHA-256 hash of the remote URL (first 16 hex chars)
/// 2. If in a git repo without remote: SHA-256 hash of the git root path (first 16 hex chars)
/// 3. If not in a git repo: SHA-256 hash of the cwd path (first 16 hex chars)
pub async fn compute_project_id(cwd: &Path) -> io::Result<ProjectId> {
    // Canonicalize the cwd
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());

    // Try to get git root
    let git_root = get_git_root(&cwd).await;

    // Try to get git remote URL
    let git_remote = if git_root.is_some() {
        get_git_remote_url(&cwd).await
    } else {
        None
    };

    // Compute the hash based on what we have
    let hash_input = if let Some(ref remote) = git_remote {
        // Normalize the remote URL for consistent hashing
        normalize_git_url(remote)
    } else if let Some(ref root) = git_root {
        root.to_string_lossy().to_string()
    } else {
        cwd.to_string_lossy().to_string()
    };

    let id = compute_hash(&hash_input);

    // Determine project name
    let name = if let Some(ref remote) = git_remote {
        extract_repo_name(remote)
    } else if let Some(ref root) = git_root {
        root.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    } else {
        cwd.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    };

    Ok(ProjectId {
        id,
        name,
        git_remote,
        git_root,
        cwd,
    })
}

/// Get the git repository root directory.
async fn get_git_root(cwd: &Path) -> Option<PathBuf> {
    let output = run_git_command(&["rev-parse", "--show-toplevel"], cwd).await?;

    if output.status.success() {
        let root = String::from_utf8(output.stdout).ok()?.trim().to_string();
        Some(PathBuf::from(root))
    } else {
        None
    }
}

/// Get the git remote URL (origin preferred).
async fn get_git_remote_url(cwd: &Path) -> Option<String> {
    let output = run_git_command(&["remote", "get-url", "origin"], cwd).await?;

    if output.status.success() {
        let url = String::from_utf8(output.stdout).ok()?.trim().to_string();
        if url.is_empty() { None } else { Some(url) }
    } else {
        None
    }
}

/// Run a git command with timeout.
async fn run_git_command(args: &[&str], cwd: &Path) -> Option<std::process::Output> {
    let result = timeout(
        GIT_COMMAND_TIMEOUT,
        Command::new("git").args(args).current_dir(cwd).output(),
    )
    .await;

    match result {
        Ok(Ok(output)) => Some(output),
        _ => None,
    }
}

/// Normalize a git URL for consistent hashing.
///
/// Handles:
/// - SSH URLs: git@github.com:user/repo.git -> github.com/user/repo
/// - HTTPS URLs: https://github.com/user/repo.git -> github.com/user/repo
/// - Removes trailing .git
fn normalize_git_url(url: &str) -> String {
    let url = url.trim();

    // Handle SSH format: git@host:path
    let normalized = if let Some(rest) = url.strip_prefix("git@") {
        rest.replace(':', "/")
    } else if let Some(rest) = url.strip_prefix("https://") {
        rest.to_string()
    } else if let Some(rest) = url.strip_prefix("http://") {
        rest.to_string()
    } else if let Some(rest) = url.strip_prefix("ssh://") {
        // ssh://git@host/path format
        rest.strip_prefix("git@").unwrap_or(rest).to_string()
    } else {
        url.to_string()
    };

    // Remove trailing .git
    normalized
        .strip_suffix(".git")
        .unwrap_or(&normalized)
        .to_string()
}

/// Extract repository name from a git URL.
fn extract_repo_name(url: &str) -> String {
    let normalized = normalize_git_url(url);

    // Get the last path component
    normalized
        .rsplit('/')
        .next()
        .unwrap_or("unknown")
        .to_string()
}

/// Compute a deterministic hash and return first 16 hex characters.
///
/// Note: This uses `DefaultHasher` which is deterministic within a single
/// Rust version but not guaranteed stable across Rust versions. If the hash
/// algorithm changes, existing transcript directories will become "orphaned"
/// (still on disk but not found by project lookup). This is acceptable for
/// transcript persistence since it's a non-critical feature - old transcripts
/// remain accessible via direct file access, and new sessions simply create
/// new directories.
pub(crate) fn compute_hash(input: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hash;
    use std::hash::Hasher;

    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    let hash = hasher.finish();

    // Format as 16 hex characters (u64 = 16 hex digits)
    format!("{hash:016x}")
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_normalize_git_url_ssh() {
        assert_eq!(
            normalize_git_url("git@github.com:user/repo.git"),
            "github.com/user/repo"
        );
    }

    #[test]
    fn test_normalize_git_url_https() {
        assert_eq!(
            normalize_git_url("https://github.com/user/repo.git"),
            "github.com/user/repo"
        );
    }

    #[test]
    fn test_normalize_git_url_no_suffix() {
        assert_eq!(
            normalize_git_url("https://github.com/user/repo"),
            "github.com/user/repo"
        );
    }

    #[test]
    fn test_extract_repo_name() {
        assert_eq!(
            extract_repo_name("git@github.com:user/my-project.git"),
            "my-project"
        );
        assert_eq!(
            extract_repo_name("https://github.com/user/another-repo.git"),
            "another-repo"
        );
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let hash1 = compute_hash("test-input");
        let hash2 = compute_hash("test-input");
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 16);
    }

    #[test]
    fn test_compute_hash_different_inputs() {
        let hash1 = compute_hash("input1");
        let hash2 = compute_hash("input2");
        assert_ne!(hash1, hash2);
    }
}
