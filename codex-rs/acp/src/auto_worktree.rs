use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;

/// Rename an existing auto-worktree using a prompt summary.
///
/// This function:
/// 1. Converts the summary into a branch-name-safe slug
/// 2. Renames the git branch
/// 3. Moves the worktree directory
///
/// Returns the new worktree path. If renaming fails, the original worktree
/// is left unchanged and the error is returned.
pub fn rename_auto_worktree(
    repo_root: &Path,
    old_worktree_path: &Path,
    old_branch: &str,
    summary: &str,
) -> Result<PathBuf> {
    let new_branch = codex_git::summary_to_branch_name(summary);
    let dir_name = new_branch.strip_prefix("auto/").unwrap_or(&new_branch);
    let worktrees_dir = old_worktree_path
        .parent()
        .context("worktree path has no parent directory")?;
    let new_path = worktrees_dir.join(dir_name);

    codex_git::rename_worktree(
        repo_root,
        old_worktree_path,
        &new_path,
        old_branch,
        &new_branch,
    )
    .context("Failed to rename git worktree")?;

    Ok(new_path)
}

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

    #[test]
    fn test_rename_auto_worktree_renames_to_summary_based_name() {
        let temp = init_temp_repo();

        // Create the initial auto-worktree (random name)
        let worktree_path = setup_auto_worktree(temp.path()).unwrap();
        assert!(worktree_path.exists());

        // Extract the old branch name from the worktree
        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&worktree_path)
            .output()
            .unwrap();
        let old_branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert!(
            old_branch.starts_with("auto/"),
            "initial branch should start with auto/"
        );

        // Rename using a summary
        let result = rename_auto_worktree(temp.path(), &worktree_path, &old_branch, "Fix auth bug");
        assert!(
            result.is_ok(),
            "rename_auto_worktree should succeed: {:?}",
            result.err()
        );

        let new_path = result.unwrap();

        // Old path should be gone
        assert!(
            !worktree_path.exists(),
            "old worktree path should not exist"
        );
        // New path should exist
        assert!(new_path.exists(), "new worktree path should exist");

        // New path should contain the summary slug
        let dir_name = new_path.file_name().unwrap().to_string_lossy();
        assert!(
            dir_name.starts_with("fix-auth-bug-"),
            "new dir name should start with summary slug, got: {dir_name}"
        );

        // Branch should be renamed
        let output = Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&new_path)
            .output()
            .unwrap();
        let new_branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert!(
            new_branch.starts_with("auto/fix-auth-bug-"),
            "branch should be renamed to summary-based name, got: {new_branch}"
        );
    }
}
