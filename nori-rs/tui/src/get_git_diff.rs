//! Utility to compute the current Git diff for the working directory.
//!
//! Returns a PR-like diff: changes since the merge-base with the default
//! branch, plus any untracked files. When the current directory is not inside
//! a Git repository, the function returns `Ok((false, String::new()))`.

use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// Return value of [`get_git_diff`].
///
/// * `bool` – Whether the directory is inside a Git repo.
/// * `String` – The concatenated diff (may be empty).
pub(crate) async fn get_git_diff(dir: Option<&Path>) -> io::Result<(bool, String)> {
    // First check if we are inside a Git repository.
    if !inside_git_repo(dir).await? {
        return Ok((false, String::new()));
    }

    let diff_base = resolve_diff_base(dir).await;
    let diff_args = ["diff", "--color", &diff_base];

    // Run tracked diff and untracked file listing in parallel.
    let (tracked_diff_res, untracked_output_res) = tokio::join!(
        run_git(dir, &diff_args, true),
        run_git(dir, &["ls-files", "--others", "--exclude-standard"], false),
    );
    let tracked_diff = tracked_diff_res?;
    let untracked_output = untracked_output_res?;

    let mut untracked_diff = String::new();
    let null_device: &Path = if cfg!(windows) {
        Path::new("NUL")
    } else {
        Path::new("/dev/null")
    };

    let null_path = null_device.to_str().unwrap_or("/dev/null").to_string();
    let dir_buf = dir.map(PathBuf::from);
    let mut join_set: tokio::task::JoinSet<io::Result<String>> = tokio::task::JoinSet::new();
    for file in untracked_output
        .split('\n')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let null_path = null_path.clone();
        let file = file.to_string();
        let dir_buf = dir_buf.clone();
        join_set.spawn(async move {
            let args = ["diff", "--color", "--no-index", "--", &null_path, &file];
            run_git(dir_buf.as_deref(), &args, true).await
        });
    }
    while let Some(res) = join_set.join_next().await {
        match res {
            Ok(Ok(diff)) => untracked_diff.push_str(&diff),
            Ok(Err(err)) if err.kind() == io::ErrorKind::NotFound => {}
            Ok(Err(err)) => return Err(err),
            Err(_) => {}
        }
    }

    Ok((true, format!("{tracked_diff}{untracked_diff}")))
}

/// Run a git command, optionally in a specific directory.
///
/// When `allow_exit_1` is true, exit code 1 is treated as success (git diff
/// returns 1 when differences are found).
async fn run_git(dir: Option<&Path>, args: &[&str], allow_exit_1: bool) -> io::Result<String> {
    let mut cmd = Command::new("git");
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::null());
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let output = cmd.output().await?;

    let ok = output.status.success() || (allow_exit_1 && output.status.code() == Some(1));
    if ok {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(io::Error::other(format!(
            "git {args:?} failed with status {}",
            output.status
        )))
    }
}

/// Determine if the directory is inside a Git repository.
async fn inside_git_repo(dir: Option<&Path>) -> io::Result<bool> {
    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", "--is-inside-work-tree"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let status = cmd.status().await;

    match status {
        Ok(s) if s.success() => Ok(true),
        Ok(_) => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false), // git not installed
        Err(e) => Err(e),
    }
}

/// Resolve the diff base to produce a PR-like diff.
///
/// Resolution order:
/// 1. `origin/HEAD` → merge-base with HEAD
/// 2. `main` branch → merge-base with HEAD
/// 3. `master` branch → merge-base with HEAD
/// 4. Fallback to `"HEAD"` (uncommitted changes only)
async fn resolve_diff_base(dir: Option<&Path>) -> String {
    // Try origin/HEAD first
    if let Some(default_branch) = get_origin_head(dir).await
        && let Some(merge_base) = get_merge_base(dir, &format!("origin/{default_branch}")).await
    {
        return merge_base;
    }

    // Try common default branch names
    for branch in &["main", "master"] {
        if branch_exists(dir, branch).await
            && let Some(merge_base) = get_merge_base(dir, branch).await
        {
            return merge_base;
        }
    }

    // Fallback: diff against HEAD (uncommitted changes only)
    "HEAD".to_string()
}

async fn get_origin_head(dir: Option<&Path>) -> Option<String> {
    let output = run_git(dir, &["symbolic-ref", "refs/remotes/origin/HEAD"], false)
        .await
        .ok()?;
    let stdout = output.trim();
    // e.g. "refs/remotes/origin/main" → "main"
    stdout
        .strip_prefix("refs/remotes/origin/")
        .map(String::from)
}

async fn branch_exists(dir: Option<&Path>, branch: &str) -> bool {
    run_git(dir, &["rev-parse", "--verify", branch], false)
        .await
        .is_ok()
}

async fn get_merge_base(dir: Option<&Path>, target: &str) -> Option<String> {
    let output = run_git(dir, &["merge-base", "HEAD", target], false)
        .await
        .ok()?;
    let sha = output.trim();
    if sha.is_empty() {
        return None;
    }
    Some(sha.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Create a temp git repo with an initial commit and return the TempDir.
    async fn create_temp_git_repo() -> TempDir {
        let dir = TempDir::new().expect("failed to create temp dir");
        let path = dir.path();

        // git init + initial commit
        run_in(path, &["git", "init"]).await;
        run_in(path, &["git", "config", "user.email", "test@test.com"]).await;
        run_in(path, &["git", "config", "user.name", "Test"]).await;
        std::fs::write(path.join("file.txt"), "initial\n").unwrap();
        run_in(path, &["git", "add", "."]).await;
        run_in(path, &["git", "commit", "-m", "init"]).await;

        dir
    }

    async fn run_in(dir: &Path, args: &[&str]) {
        let status = tokio::process::Command::new(args[0])
            .args(&args[1..])
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .unwrap();
        assert!(status.success(), "command failed: {args:?}");
    }

    #[tokio::test]
    async fn diff_with_explicit_dir_shows_changes() {
        let dir = create_temp_git_repo().await;
        let path = dir.path();

        // Modify the tracked file
        std::fs::write(path.join("file.txt"), "modified\n").unwrap();

        let (is_git, diff_text) = get_git_diff(Some(path)).await.unwrap();
        assert!(is_git, "should detect git repo");
        assert!(
            !diff_text.is_empty(),
            "diff should be non-empty when there are changes"
        );
        assert!(
            diff_text.contains("file.txt"),
            "diff should mention the changed file"
        );
    }

    #[tokio::test]
    async fn diff_with_explicit_dir_includes_untracked_files() {
        let dir = create_temp_git_repo().await;
        let path = dir.path();

        // Create an untracked file
        std::fs::write(path.join("new_file.txt"), "new content\n").unwrap();

        let (is_git, diff_text) = get_git_diff(Some(path)).await.unwrap();
        assert!(is_git);
        assert!(
            diff_text.contains("new_file.txt"),
            "diff should include untracked files"
        );
    }

    #[tokio::test]
    async fn diff_with_non_git_dir_returns_not_git() {
        let dir = TempDir::new().expect("failed to create temp dir");
        let (is_git, diff_text) = get_git_diff(Some(dir.path())).await.unwrap();
        assert!(!is_git, "non-git dir should return false");
        assert!(diff_text.is_empty());
    }

    #[tokio::test]
    async fn diff_includes_branch_commits_not_just_uncommitted() {
        let dir = create_temp_git_repo().await;
        let path = dir.path();

        // Create a "main" branch reference at current HEAD
        run_in(path, &["git", "branch", "-M", "main"]).await;

        // Create a feature branch and make a committed change
        run_in(path, &["git", "checkout", "-b", "feature"]).await;
        std::fs::write(path.join("file.txt"), "changed on feature\n").unwrap();
        run_in(path, &["git", "add", "."]).await;
        run_in(path, &["git", "commit", "-m", "feature change"]).await;

        // No uncommitted changes, but there's a committed diff vs main
        let (is_git, diff_text) = get_git_diff(Some(path)).await.unwrap();
        assert!(is_git);
        assert!(
            !diff_text.is_empty(),
            "diff should show committed changes vs merge-base (PR-like diff)"
        );
        assert!(
            diff_text.contains("file.txt"),
            "diff should show the file changed on the feature branch"
        );
    }

    #[tokio::test]
    async fn diff_on_main_with_no_changes_is_empty() {
        let dir = create_temp_git_repo().await;
        let path = dir.path();
        run_in(path, &["git", "branch", "-M", "main"]).await;

        let (is_git, diff_text) = get_git_diff(Some(path)).await.unwrap();
        assert!(is_git);
        assert!(
            diff_text.is_empty(),
            "no changes on main should produce empty diff"
        );
    }
}
