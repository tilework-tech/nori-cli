use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;

/// Set up an auto-worktree for the given working directory.
///
/// This function:
/// 1. Verifies the cwd is inside a git repo
/// 2. Ensures `.worktrees/` is in `.gitignore`
/// 3. Creates the `.worktrees/` directory if needed
/// 4. Generates a unique branch name
/// 5. Creates a new git worktree
///
/// Returns the path to the new worktree directory.
pub fn setup_auto_worktree(cwd: &Path) -> Result<PathBuf> {
    // 1. Find the git repo root
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .context("Failed to run git rev-parse")?;

    if !output.status.success() {
        anyhow::bail!("Not inside a git repository");
    }

    let repo_root = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string());

    // 2. Ensure .worktrees/ is in .gitignore
    codex_git::ensure_gitignore_entry(&repo_root, ".worktrees/")
        .context("Failed to update .gitignore")?;

    // 3. Create .worktrees/ directory if needed
    let worktrees_dir = repo_root.join(".worktrees");
    std::fs::create_dir_all(&worktrees_dir).context("Failed to create .worktrees directory")?;

    // 4. Generate a unique branch name
    let branch_name = codex_git::generate_worktree_branch_name();

    // 5. Create the worktree
    // Extract the short name from the branch (e.g., "auto/swift-oak-20260201-120000" -> "swift-oak-20260201-120000")
    let dir_name = branch_name.strip_prefix("auto/").unwrap_or(&branch_name);
    let worktree_path = worktrees_dir.join(dir_name);

    codex_git::create_worktree(&repo_root, &worktree_path, &branch_name)
        .context("Failed to create git worktree")?;

    Ok(worktree_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn init_temp_repo() -> tempfile::TempDir {
        let temp_dir = tempfile::TempDir::new().unwrap();
        Command::new("git")
            .args(["init"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        std::fs::write(temp_dir.path().join("README.md"), "init").unwrap();
        Command::new("git")
            .args(["add", "."])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(temp_dir.path())
            .output()
            .unwrap();
        temp_dir
    }

    #[test]
    fn test_setup_auto_worktree_creates_worktree() {
        let temp = init_temp_repo();

        let result = setup_auto_worktree(temp.path());
        assert!(result.is_ok(), "setup_auto_worktree should succeed");

        let worktree_path = result.unwrap();
        assert!(worktree_path.exists(), "worktree path should exist");
        assert!(
            worktree_path.to_string_lossy().contains(".worktrees"),
            "worktree path should be inside .worktrees/"
        );

        // .gitignore should contain .worktrees/
        let gitignore = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
        assert!(gitignore.contains(".worktrees/"));
    }

    #[test]
    fn test_setup_auto_worktree_fails_outside_git_repo() {
        let temp = tempfile::TempDir::new().unwrap();
        // No git init - this is not a git repo

        let result = setup_auto_worktree(temp.path());
        assert!(result.is_err(), "should fail outside a git repo");
    }
}
