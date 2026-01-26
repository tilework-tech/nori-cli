//! Project identification for transcript organization.

use sha2::Digest;
use sha2::Sha256;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

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
/// 1. Git repository with remote: SHA-256 hash of the canonical remote URL (first 16 hex chars)
/// 2. Git repository without remote: SHA-256 hash of the git root absolute path (first 16 hex chars)
/// 3. No git: SHA-256 hash of the working directory absolute path (first 16 hex chars)
pub fn compute_project_id(cwd: &Path) -> std::io::Result<ProjectId> {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());

    // Try to get git root
    let git_root = get_git_root(&cwd);

    // Try to get git remote (only if we have a git root)
    let git_remote = git_root.as_ref().and_then(|root| get_git_remote(root));

    // Determine what to hash for the project ID
    let hash_input = if let Some(ref remote) = git_remote {
        // Hash the remote URL for repos with remotes
        remote.clone()
    } else if let Some(ref root) = git_root {
        // Hash the git root path for local-only repos
        root.to_string_lossy().into_owned()
    } else {
        // Hash the cwd for non-git directories
        cwd.to_string_lossy().into_owned()
    };

    let id = compute_hash(&hash_input);

    // Determine project name from directory
    let name = git_root
        .as_ref()
        .unwrap_or(&cwd)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unknown".to_string());

    Ok(ProjectId {
        id,
        name,
        git_remote,
        git_root,
        cwd,
    })
}

/// Get the git root directory for a path.
fn get_git_root(cwd: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;

    if output.status.success() {
        let path_str = String::from_utf8_lossy(&output.stdout);
        let path = PathBuf::from(path_str.trim());
        // Canonicalize to ensure consistent paths
        path.canonicalize().ok()
    } else {
        None
    }
}

/// Get the origin remote URL for a git repository.
fn get_git_remote(git_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(git_root)
        .output()
        .ok()?;

    if output.status.success() {
        let url = String::from_utf8_lossy(&output.stdout);
        let url = url.trim();
        if url.is_empty() {
            None
        } else {
            Some(url.to_string())
        }
    } else {
        None
    }
}

/// Compute SHA-256 hash of input and return first 16 hex characters.
fn compute_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    // Take first 8 bytes (16 hex chars)
    hex::encode(&result[..8])
}
