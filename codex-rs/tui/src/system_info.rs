use std::env;
use std::fs;
use std::process::Command;

#[derive(Clone, Debug, Default)]
pub(crate) struct SystemInfo {
    pub(crate) git_branch: Option<String>,
    pub(crate) nori_profile: Option<String>,
    pub(crate) nori_version: Option<String>,
    pub(crate) git_lines_added: Option<i32>,
    pub(crate) git_lines_removed: Option<i32>,
    /// Whether the current directory is a git worktree (not the main repo)
    pub(crate) is_worktree: bool,
}

impl SystemInfo {
    /// Collect system info synchronously (blocking).
    /// Only available in debug builds for E2E testing via NORI_SYNC_SYSTEM_INFO=1.
    #[cfg(debug_assertions)]
    pub fn collect_sync() -> Self {
        Self::collect_fresh()
    }

    /// Collect fresh system info. This is blocking and should be called from
    /// a background thread to avoid blocking TUI startup.
    pub(crate) fn collect_fresh() -> Self {
        let (git_lines_added, git_lines_removed) = get_git_stats(None);
        Self {
            git_branch: get_git_branch(None),
            nori_profile: get_nori_profile(),
            nori_version: get_nori_version(),
            git_lines_added,
            git_lines_removed,
            is_worktree: is_git_worktree(None),
        }
    }

    /// Collect system info for a specific directory. This is blocking and should
    /// be called from a background thread to avoid blocking TUI.
    ///
    /// This is used when the agent is working in a different directory than the
    /// TUI was launched from (e.g., a git worktree).
    pub(crate) fn collect_for_directory(dir: &std::path::Path) -> Self {
        let (git_lines_added, git_lines_removed) = get_git_stats(Some(dir));
        Self {
            git_branch: get_git_branch(Some(dir)),
            nori_profile: get_nori_profile(), // Profile search still uses process CWD
            nori_version: get_nori_version(), // Version is global
            git_lines_added,
            git_lines_removed,
            is_worktree: is_git_worktree(Some(dir)),
        }
    }
}

fn get_nori_version() -> Option<String> {
    let output = Command::new("nori-ai").arg("--version").output().ok()?;

    if !output.status.success() {
        return None;
    }

    let version_output = String::from_utf8(output.stdout).ok()?;
    parse_nori_version(&version_output)
}

fn parse_nori_version(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }

    let tokens: Vec<&str> = trimmed.split_whitespace().collect();

    match tokens.len() {
        // Format: "19.1.1" (version only)
        1 => Some(tokens[0].to_string()),
        // Format: "nori-ai 19.1.1" (program name + version)
        2 => Some(tokens[1].to_string()),
        // Unexpected format
        _ => None,
    }
}

fn get_nori_profile() -> Option<String> {
    // Search for .nori-config.json in current directory and parent directories
    let mut current_dir = env::current_dir().ok()?;

    loop {
        let config_path = current_dir.join(".nori-config.json");
        if config_path.exists() {
            // Try to read and parse the config file
            if let Ok(contents) = fs::read_to_string(&config_path)
                && let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents)
            {
                // Extract agents.claude-code.profile.baseProfile
                if let Some(profile) = json
                    .get("agents")
                    .and_then(|a| a.get("claude-code"))
                    .and_then(|c| c.get("profile"))
                    .and_then(|p| p.get("baseProfile"))
                    .and_then(|b| b.as_str())
                {
                    return Some(profile.to_string());
                }
            }
        }

        // Move to parent directory
        if !current_dir.pop() {
            break;
        }
    }

    None
}

fn get_git_stats(dir: Option<&std::path::Path>) -> (Option<i32>, Option<i32>) {
    let mut cmd = Command::new("git");
    cmd.args(["diff", "HEAD", "--shortstat"]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }

    let output = match cmd.output() {
        Ok(output) => output,
        Err(_) => return (None, None),
    };

    if !output.status.success() {
        return (None, None);
    }

    let stats = match String::from_utf8(output.stdout) {
        Ok(s) => s,
        Err(_) => return (None, None),
    };

    parse_git_shortstat(&stats)
}

fn parse_git_shortstat(output: &str) -> (Option<i32>, Option<i32>) {
    if output.trim().is_empty() {
        return (None, None);
    }

    let mut added = None;
    let mut removed = None;

    // Parse insertions: "10 insertions(+)" or "10 insertion(+)"
    if let Some(insertions_idx) = output.find("insertion") {
        // Extract the substring before "insertion"
        let before = &output[..insertions_idx];
        // Split by commas and spaces to get individual tokens
        let tokens: Vec<&str> = before
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .collect();
        // The number should be the last token before "insertion"
        if let Some(last_token) = tokens.last()
            && let Ok(num) = last_token.parse::<i32>()
        {
            added = Some(num);
        }
    }

    // Parse deletions: "3 deletions(-)" or "3 deletion(-)"
    if let Some(deletions_idx) = output.find("deletion") {
        let before = &output[..deletions_idx];
        let tokens: Vec<&str> = before
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .collect();
        if let Some(last_token) = tokens.last()
            && let Ok(num) = last_token.parse::<i32>()
        {
            removed = Some(num);
        }
    }

    // If we found insertions but not deletions, deletions is 0
    if added.is_some() && removed.is_none() {
        removed = Some(0);
    }

    // If we found deletions but not insertions, insertions is 0
    if removed.is_some() && added.is_none() {
        added = Some(0);
    }

    (added, removed)
}

fn get_git_branch(dir: Option<&std::path::Path>) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["branch", "--show-current"]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }

    let output = match cmd.output() {
        Ok(output) => output,
        Err(_e) => {
            return None;
        }
    };

    if !output.status.success() {
        return None;
    }

    let branch = String::from_utf8(output.stdout).ok()?;
    let branch = branch.trim();

    if branch.is_empty() {
        return None;
    }

    // Truncate long branch names to 30 characters
    let truncated = if branch.len() > 30 {
        format!("{}...", &branch[..27])
    } else {
        branch.to_string()
    };

    Some(truncated)
}

/// Check if the directory is a git worktree (not the main repository).
/// Returns true if this is a linked worktree, false if it's the main repo or not a git repo.
fn is_git_worktree(dir: Option<&std::path::Path>) -> bool {
    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", "--git-common-dir"]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }

    let common_dir_output = match cmd.output() {
        Ok(output) if output.status.success() => String::from_utf8(output.stdout).ok(),
        _ => return false,
    };

    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", "--git-dir"]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }

    let git_dir_output = match cmd.output() {
        Ok(output) if output.status.success() => String::from_utf8(output.stdout).ok(),
        _ => return false,
    };

    match (common_dir_output, git_dir_output) {
        (Some(common), Some(git)) => {
            let common = common.trim();
            let git = git.trim();
            // In a worktree, git-dir points to .git/worktrees/<name>
            // while common-dir points to the main .git directory
            // They differ when in a worktree
            common != git && !git.ends_with(".git")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_git_branch_in_git_repo() {
        // This test runs in the actual git repo, so it should detect a branch
        // However, CI runners may checkout in detached HEAD state, so we only
        // verify format when a branch is detected, not that it must be present
        let branch = get_git_branch(None);
        if let Some(branch_name) = branch {
            assert!(!branch_name.is_empty(), "Branch name should not be empty");
            assert!(
                !branch_name.contains('\n'),
                "Branch name should not contain newlines"
            );
        }
        // If no branch is detected (detached HEAD, shallow clone), that's OK for CI
    }

    #[test]
    fn test_get_git_branch_truncates_long_names() {
        // Test that very long branch names are truncated
        // We'll implement truncation in get_git_branch
        // For now this tests that we handle the output correctly
        let branch = get_git_branch(None);
        if let Some(name) = branch {
            assert!(
                name.len() <= 30,
                "Branch names should be truncated to 30 characters"
            );
        }
    }

    #[test]
    fn test_collect_for_directory_uses_specified_path() {
        // Test that collect_for_directory runs git commands in the specified directory
        // We use the current repo directory which should be a valid git repo
        let current_dir = std::env::current_dir().expect("should have current dir");
        let info = SystemInfo::collect_for_directory(&current_dir);

        // The current directory is a git repo, so we should get branch info
        // (unless in detached HEAD state in CI)
        if let Some(branch) = &info.git_branch {
            assert!(!branch.is_empty());
        }
    }

    #[test]
    fn test_collect_for_directory_non_git_returns_none_branch() {
        // Test that a non-git directory returns None for git_branch
        let temp_dir = std::env::temp_dir();
        let info = SystemInfo::collect_for_directory(&temp_dir);

        // /tmp is not a git repo, so git_branch should be None
        assert!(
            info.git_branch.is_none(),
            "Non-git directory should have no branch"
        );
        assert!(
            !info.is_worktree,
            "Non-git directory should not be a worktree"
        );
    }

    #[test]
    fn test_is_git_worktree_returns_false_for_non_git() {
        // Non-git directories should return false
        let temp_dir = std::env::temp_dir();
        assert!(
            !is_git_worktree(Some(&temp_dir)),
            "Non-git dir should not be worktree"
        );
    }

    #[test]
    fn test_parse_git_shortstat_with_changes() {
        let output = " 2 files changed, 10 insertions(+), 3 deletions(-)";
        let (added, removed) = parse_git_shortstat(output);
        assert_eq!(added, Some(10));
        assert_eq!(removed, Some(3));
    }

    #[test]
    fn test_parse_git_shortstat_only_additions() {
        let output = " 1 file changed, 5 insertions(+)";
        let (added, removed) = parse_git_shortstat(output);
        assert_eq!(added, Some(5));
        assert_eq!(removed, Some(0));
    }

    #[test]
    fn test_parse_git_shortstat_only_deletions() {
        let output = " 1 file changed, 7 deletions(-)";
        let (added, removed) = parse_git_shortstat(output);
        assert_eq!(added, Some(0));
        assert_eq!(removed, Some(7));
    }

    #[test]
    fn test_parse_git_shortstat_empty() {
        let output = "";
        let (added, removed) = parse_git_shortstat(output);
        assert_eq!(added, None);
        assert_eq!(removed, None);
    }

    #[test]
    fn test_parse_nori_version_with_program_name() {
        // Old format: "nori-ai 19.1.1"
        let version = parse_nori_version("nori-ai 19.1.1");
        assert_eq!(version, Some("19.1.1".to_string()));
    }

    #[test]
    fn test_parse_nori_version_version_only() {
        // Current format: "19.1.1"
        let version = parse_nori_version("19.1.1");
        assert_eq!(version, Some("19.1.1".to_string()));
    }

    #[test]
    fn test_parse_nori_version_with_newline() {
        // Real output has trailing newline
        let version = parse_nori_version("19.1.1\n");
        assert_eq!(version, Some("19.1.1".to_string()));
    }

    #[test]
    fn test_collect_sync_returns_valid_data() {
        // This test runs collect_sync and verifies the returned data is valid
        // Note: CI runners may checkout in detached HEAD state, so git_branch
        // may be None - we only verify format when it's present
        let info = SystemInfo::collect_sync();
        if let Some(branch) = &info.git_branch {
            assert!(!branch.is_empty(), "git_branch should not be empty if set");
            assert!(
                branch.len() <= 30,
                "git_branch should be truncated to 30 chars"
            );
        }
        // Other fields may or may not be populated depending on environment
    }
}
