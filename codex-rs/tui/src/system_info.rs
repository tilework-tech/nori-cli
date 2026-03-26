use codex_acp::AgentKind;
use codex_acp::TranscriptLocation;
use std::env;
use std::process::Command;

/// Indicates which command was used to detect the Nori version.
/// This affects the UI display text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NoriVersionSource {
    /// Version from `nori-skillsets` command (new installer) - displays as "Skillsets"
    #[default]
    Skillsets,
    /// Version from `nori-ai` command (legacy installer) - displays as "Profiles"
    Profiles,
}

impl NoriVersionSource {
    /// Returns the display label for this version source.
    pub(crate) fn label(self) -> &'static str {
        match self {
            NoriVersionSource::Skillsets => "Skillsets",
            NoriVersionSource::Profiles => "Profiles",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SystemInfo {
    pub(crate) git_branch: Option<String>,
    pub(crate) active_skillsets: Vec<String>,
    pub(crate) nori_version: Option<String>,
    /// Indicates which command was used to detect the version (affects UI display).
    pub(crate) nori_version_source: Option<NoriVersionSource>,
    pub(crate) git_lines_added: Option<i32>,
    pub(crate) git_lines_removed: Option<i32>,
    /// Whether the current directory is a git worktree (not the main repo)
    pub(crate) is_worktree: bool,
    /// The worktree directory name (last path component when parent is `.worktrees/`)
    pub(crate) worktree_name: Option<String>,
    /// Current transcript location if running within an agent environment
    pub(crate) transcript_location: Option<TranscriptLocation>,
    /// Warning about low disk space with worktrees present (session-start check)
    pub(crate) worktree_cleanup_warning: Option<WorktreeCleanupWarning>,
}

impl SystemInfo {
    /// Collect system info synchronously (blocking).
    /// Only available in debug builds for E2E testing via NORI_SYNC_SYSTEM_INFO=1.
    #[cfg(debug_assertions)]
    pub fn collect_sync() -> Self {
        Self::collect_fresh(None)
    }

    /// Collect fresh system info. This is blocking and should be called from
    /// a background thread to avoid blocking TUI startup.
    ///
    /// # Arguments
    ///
    /// * `agent_kind` - Optional agent kind to use for transcript discovery.
    ///   If provided, searches for transcripts from that specific agent.
    ///   If None, attempts to detect the agent from environment variables.
    pub(crate) fn collect_fresh(agent_kind: Option<AgentKind>) -> Self {
        Self::collect_impl(None, agent_kind, None)
    }

    /// Collect system info for a specific directory. This is blocking and should
    /// be called from a background thread to avoid blocking TUI.
    ///
    /// This is used when the agent is working in a different directory than the
    /// TUI was launched from (e.g., a git worktree).
    ///
    /// # Arguments
    ///
    /// * `dir` - The directory to collect system info for
    /// * `agent_kind` - Optional agent kind to use for transcript discovery.
    ///   If provided, searches for transcripts from that specific agent.
    ///   If None, attempts to detect the agent from environment variables.
    #[cfg(test)]
    pub(crate) fn collect_for_directory(
        dir: &std::path::Path,
        agent_kind: Option<AgentKind>,
    ) -> Self {
        Self::collect_impl(Some(dir), agent_kind, None)
    }

    /// Collect system info for a specific directory with first-message matching.
    ///
    /// This is the preferred method for Claude Code transcript discovery as it
    /// uses the first user message to accurately identify the correct transcript.
    ///
    /// # Arguments
    ///
    /// * `dir` - The directory to collect system info for
    /// * `agent_kind` - Optional agent kind to use for transcript discovery.
    /// * `first_message` - The first user message for transcript matching (used by Claude Code)
    pub(crate) fn collect_for_directory_with_message(
        dir: &std::path::Path,
        agent_kind: Option<AgentKind>,
        first_message: Option<&str>,
    ) -> Self {
        Self::collect_impl(Some(dir), agent_kind, first_message)
    }

    fn collect_impl(
        dir: Option<&std::path::Path>,
        agent_kind: Option<AgentKind>,
        first_message: Option<&str>,
    ) -> Self {
        let (git_lines_added, git_lines_removed) = get_git_stats(dir);
        let transcript_location = match dir {
            Some(dir) => discover_transcript(dir, agent_kind, first_message),
            None => env::current_dir()
                .ok()
                .and_then(|cwd| discover_transcript(&cwd, agent_kind, first_message)),
        };
        let (nori_version, nori_version_source) = get_nori_version();

        #[cfg(unix)]
        let worktree_cleanup_warning = {
            let effective_dir = dir
                .map(std::path::PathBuf::from)
                .or_else(|| env::current_dir().ok());
            effective_dir.and_then(|d| check_worktree_cleanup(&d))
        };
        #[cfg(not(unix))]
        let worktree_cleanup_warning = None;

        let is_worktree = is_git_worktree(dir);

        Self {
            git_branch: get_git_branch(dir),
            active_skillsets: get_active_skillsets(dir),
            nori_version,
            nori_version_source,
            git_lines_added,
            git_lines_removed,
            is_worktree,
            worktree_name: if is_worktree {
                dir.and_then(extract_worktree_name)
            } else {
                None
            },
            transcript_location,
            worktree_cleanup_warning,
        }
    }
}

/// Helper to discover transcript location for a specific agent kind.
///
/// Uses first-message matching for accurate transcript identification.
/// For Claude Code, the first_message is required to find the correct transcript.
/// For other agents (Codex, Gemini), the first_message is ignored.
fn discover_transcript(
    dir: &std::path::Path,
    agent_kind: Option<AgentKind>,
    first_message: Option<&str>,
) -> Option<TranscriptLocation> {
    agent_kind.and_then(|agent| {
        codex_acp::discover_transcript_for_agent_with_message(dir, agent, first_message).ok()
    })
}

fn get_nori_version() -> (Option<String>, Option<NoriVersionSource>) {
    // Try nori-skillsets first (new installer)
    if let Ok(output) = Command::new("nori-skillsets").arg("--version").output()
        && output.status.success()
        && let Some(version) = String::from_utf8(output.stdout)
            .ok()
            .and_then(|s| parse_nori_version(&s))
    {
        return (Some(version), Some(NoriVersionSource::Skillsets));
    }

    // Fallback to nori-ai (legacy installer)
    if let Ok(output) = Command::new("nori-ai").arg("--version").output()
        && output.status.success()
        && let Some(version) = String::from_utf8(output.stdout)
            .ok()
            .and_then(|s| parse_nori_version(&s))
    {
        return (Some(version), Some(NoriVersionSource::Profiles));
    }

    (None, None)
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

/// Parse the stdout of `nori-skillsets list-active` into a list of skillset names.
///
/// Expects one skillset name per line. Trims whitespace and skips blank lines.
fn parse_active_skillsets(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Get active skillsets by running `nori-skillsets list-active`.
///
/// Falls back to reading `.nori-config.json` if `list-active` is not supported
/// (older versions of nori-skillsets). Returns an empty vec if no skillsets are
/// active or nori-skillsets is not installed.
fn get_active_skillsets(dir: Option<&std::path::Path>) -> Vec<String> {
    let mut cmd = Command::new("nori-skillsets");
    cmd.arg("list-active");
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    match cmd.output() {
        Ok(output) if output.status.success() => String::from_utf8(output.stdout)
            .ok()
            .map(|s| parse_active_skillsets(&s))
            .unwrap_or_default(),
        Ok(output) => {
            // Non-zero exit. Distinguish "no active skillsets" (known command,
            // no results) from "unknown subcommand" (old nori-skillsets version).
            // When the subcommand is unknown, the CLI framework writes to stderr.
            let has_stderr = output.stderr.iter().any(|&b| !b.is_ascii_whitespace());
            if has_stderr {
                // Likely an old version that doesn't support list-active.
                // Fall back to reading .nori-config.json directly.
                get_nori_profile().into_iter().collect()
            } else {
                // Known command, just no active skillsets.
                Vec::new()
            }
        }
        // nori-skillsets not installed at all.
        Err(_) => Vec::new(),
    }
}

/// Read the active skillset from `.nori-config.json` by walking parent directories.
///
/// This is the legacy fallback for older versions of nori-skillsets that don't
/// support the `list-active` subcommand.
fn get_nori_profile() -> Option<String> {
    let mut current_dir = env::current_dir().ok()?;

    loop {
        let config_path = current_dir.join(".nori-config.json");
        if config_path.exists()
            && let Ok(contents) = std::fs::read_to_string(&config_path)
            && let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents)
        {
            // Try new format: activeSkillset
            if let Some(profile) = json.get("activeSkillset").and_then(|v| v.as_str()) {
                return Some(profile.to_string());
            }
            // Fall back to old format: agents.claude-code.profile.baseProfile
            if let Some(profile) = json
                .get("agents")
                .and_then(|a| a.get("claude-code"))
                .and_then(|c| c.get("profile"))
                .and_then(|p| p.get("baseProfile"))
                .and_then(|b| b.as_str())
            {
                return Some(profile.to_string());
            }
            // Fall back to oldest format: profile.baseProfile
            if let Some(profile) = json
                .get("profile")
                .and_then(|p| p.get("baseProfile"))
                .and_then(|b| b.as_str())
            {
                return Some(profile.to_string());
            }
        }

        if !current_dir.pop() {
            break;
        }
    }

    None
}

/// Parse the output of `git symbolic-ref refs/remotes/origin/HEAD` to extract
/// the default branch name. Returns `None` if the output is malformed.
fn parse_origin_head(output: &str) -> Option<String> {
    let trimmed = output.trim();
    let suffix = trimmed.strip_prefix("ref: refs/remotes/origin/")?;
    if suffix.is_empty() {
        return None;
    }
    Some(suffix.to_string())
}

/// Count the number of lines in content (non-empty content).
fn count_lines_in_content(content: &str) -> i32 {
    if content.is_empty() {
        return 0;
    }
    content.lines().count() as i32
}

/// Resolve the diff base ref to compare against.
///
/// Tries, in order:
/// 1. `origin/HEAD` (via `git symbolic-ref`) to find the remote default branch
/// 2. `main` branch exists
/// 3. `master` branch exists
/// 4. Falls back to `"HEAD"` (uncommitted changes only)
///
/// Once a default branch is found, computes the merge-base with HEAD so that
/// the diff reflects what a PR would show.
fn resolve_diff_base(dir: Option<&std::path::Path>) -> String {
    // Try origin/HEAD first
    if let Some(default_branch) = get_origin_head(dir)
        && let Some(merge_base) = get_merge_base(dir, &format!("origin/{default_branch}"))
    {
        return merge_base;
    }

    // Try common default branch names
    for branch in &["main", "master"] {
        if branch_exists(dir, branch)
            && let Some(merge_base) = get_merge_base(dir, branch)
        {
            return merge_base;
        }
    }

    // Fallback: diff against HEAD (uncommitted changes only)
    "HEAD".to_string()
}

fn get_origin_head(dir: Option<&std::path::Path>) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["symbolic-ref", "refs/remotes/origin/HEAD"]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_origin_head(&stdout)
}

fn branch_exists(dir: Option<&std::path::Path>, branch: &str) -> bool {
    let mut cmd = Command::new("git");
    cmd.args(["rev-parse", "--verify", branch]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.status().is_ok_and(|s| s.success())
}

fn get_merge_base(dir: Option<&std::path::Path>, target: &str) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.args(["merge-base", "HEAD", target]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?;
    let sha = sha.trim();
    if sha.is_empty() {
        return None;
    }
    Some(sha.to_string())
}

/// Count lines in untracked files (files not yet added to git).
fn count_untracked_lines(dir: Option<&std::path::Path>) -> i32 {
    let mut cmd = Command::new("git");
    cmd.args(["ls-files", "--others", "--exclude-standard"]);
    if let Some(d) = dir {
        cmd.current_dir(d);
    }

    let output = match cmd.output() {
        Ok(output) if output.status.success() => output,
        _ => return 0,
    };

    let file_list = match String::from_utf8(output.stdout) {
        Ok(s) => s,
        Err(_) => return 0,
    };

    let base_dir = dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let mut total_lines = 0;
    for file in file_list.lines().map(str::trim).filter(|s| !s.is_empty()) {
        let path = base_dir.join(file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            total_lines += count_lines_in_content(&content);
        }
        // Skip binary files / files that can't be read as UTF-8
    }
    total_lines
}

fn get_git_stats(dir: Option<&std::path::Path>) -> (Option<i32>, Option<i32>) {
    let diff_base = resolve_diff_base(dir);

    let mut cmd = Command::new("git");
    cmd.args(["diff", &diff_base, "--shortstat"]);
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

    let (added, removed) = parse_git_shortstat(&stats);

    // Add untracked file lines to insertions
    let untracked = count_untracked_lines(dir);
    if untracked > 0 {
        let added = Some(added.unwrap_or(0) + untracked);
        let removed = Some(removed.unwrap_or(0));
        return (added, removed);
    }

    (added, removed)
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

/// Disk space threshold: warn when free space is below this percentage.
const DISK_SPACE_LOW_PERCENT: i32 = 10;

/// Disk space information parsed from `df` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiskSpaceInfo {
    pub(crate) used_percent: i32,
}

/// Warning about low disk space when git worktrees exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorktreeCleanupWarning {
    pub(crate) worktree_count: usize,
    pub(crate) free_percent: i32,
}

/// Parse the output of `df -Pk <dir>` into structured disk space info.
///
/// Expected format (POSIX):
/// ```text
/// Filesystem     1024-blocks      Used Available Capacity Mounted on
/// /dev/sda1       500000000 450000000  50000000      90% /
/// ```
fn parse_df_output(output: &str) -> Option<DiskSpaceInfo> {
    let lines: Vec<&str> = output.trim().lines().collect();
    if lines.len() < 2 {
        return None;
    }

    // Parse the data line (skip header)
    let parts: Vec<&str> = lines[1].split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }

    let used_percent: i32 = parts[4].trim_end_matches('%').parse().ok()?;

    Some(DiskSpaceInfo { used_percent })
}

/// Get disk space info for a directory by running `df -Pk`.
#[cfg(unix)]
fn get_disk_space(dir: &std::path::Path) -> Option<DiskSpaceInfo> {
    let output = Command::new("df").arg("-Pk").arg(dir).output().ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    parse_df_output(&stdout)
}

/// Evaluate whether a worktree cleanup warning should be shown.
///
/// Takes the number of extra worktrees and the disk usage percentage.
/// Returns `Some(WorktreeCleanupWarning)` if worktrees > 0 and free space < threshold.
fn evaluate_worktree_cleanup_warning(
    worktree_count: usize,
    used_percent: i32,
) -> Option<WorktreeCleanupWarning> {
    if worktree_count == 0 {
        return None;
    }

    let free_percent = (100 - used_percent).max(0);
    if free_percent >= DISK_SPACE_LOW_PERCENT {
        return None;
    }

    Some(WorktreeCleanupWarning {
        worktree_count,
        free_percent,
    })
}

/// Check if disk space is low and worktrees exist for the given directory.
///
/// This is called during background system info collection. Returns a warning
/// if worktrees are present and disk space is below the threshold.
#[cfg(unix)]
pub(crate) fn check_worktree_cleanup(cwd: &std::path::Path) -> Option<WorktreeCleanupWarning> {
    // Check if we're in a git repo
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let repo_root =
        std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string());

    // List worktrees (excluding main)
    let worktrees = codex_git::list_worktrees(&repo_root).ok()?;
    if worktrees.is_empty() {
        return None;
    }

    // Check disk space
    let disk_space = get_disk_space(&repo_root)?;

    evaluate_worktree_cleanup_warning(worktrees.len(), disk_space.used_percent)
}

/// Extract the worktree directory name from a path.
/// Returns the last path component if the parent directory is named `.worktrees`.
pub(crate) fn extract_worktree_name(dir: &std::path::Path) -> Option<String> {
    let parent = dir.parent()?;
    if parent.file_name()?.to_str()? == ".worktrees" {
        Some(dir.file_name()?.to_str()?.to_string())
    } else {
        None
    }
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
    fn test_parse_nori_version_skillsets_format() {
        // New format: "nori-skillsets 19.2.0"
        let version = parse_nori_version("nori-skillsets 19.2.0");
        assert_eq!(version, Some("19.2.0".to_string()));
    }

    #[test]
    fn test_nori_version_source_enum_exists() {
        // Test that NoriVersionSource enum exists and has the expected variants
        let skillsets = NoriVersionSource::Skillsets;
        let profiles = NoriVersionSource::Profiles;

        // Verify they are different
        assert_ne!(skillsets, profiles);

        // Verify display format
        assert_eq!(skillsets.label(), "Skillsets");
        assert_eq!(profiles.label(), "Profiles");
    }

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
        let info = SystemInfo::collect_for_directory(&current_dir, None);

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
        let info = SystemInfo::collect_for_directory(&temp_dir, None);

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
    fn test_parse_df_output_typical() {
        let output = "\
Filesystem     1024-blocks      Used Available Capacity Mounted on
/dev/sda1       500000000 450000000  50000000      90% /
";
        let info = parse_df_output(output).expect("should parse valid df output");
        assert_eq!(info.used_percent, 90);
    }

    #[test]
    fn test_parse_df_output_empty() {
        assert!(
            parse_df_output("").is_none(),
            "empty output should return None"
        );
    }

    #[test]
    fn test_parse_df_output_header_only() {
        let output = "Filesystem     1024-blocks      Used Available Capacity Mounted on\n";
        assert!(
            parse_df_output(output).is_none(),
            "header-only should return None"
        );
    }

    #[test]
    fn test_parse_df_output_malformed() {
        let output = "not a valid df output at all\n";
        assert!(
            parse_df_output(output).is_none(),
            "malformed output should return None"
        );
    }

    #[test]
    fn test_worktree_cleanup_warning_low_disk_with_worktrees() {
        let warning = evaluate_worktree_cleanup_warning(5, 94);
        assert!(
            warning.is_some(),
            "should warn when disk is 94% used and worktrees exist"
        );
        let w = warning.unwrap();
        assert_eq!(w.worktree_count, 5);
        assert_eq!(w.free_percent, 6);
    }

    #[test]
    fn test_worktree_cleanup_warning_sufficient_disk() {
        let warning = evaluate_worktree_cleanup_warning(5, 80);
        assert!(warning.is_none(), "should not warn when 20% free");
    }

    #[test]
    fn test_worktree_cleanup_warning_low_disk_no_worktrees() {
        let warning = evaluate_worktree_cleanup_warning(0, 95);
        assert!(warning.is_none(), "should not warn when no worktrees exist");
    }

    #[test]
    fn test_worktree_cleanup_warning_at_threshold() {
        let warning = evaluate_worktree_cleanup_warning(3, 90);
        assert!(
            warning.is_none(),
            "should not warn when exactly at 10% free (threshold)"
        );
    }

    #[test]
    fn test_worktree_cleanup_warning_just_below_threshold() {
        let warning = evaluate_worktree_cleanup_warning(3, 91);
        assert!(
            warning.is_some(),
            "should warn when 9% free (below threshold)"
        );
        let w = warning.unwrap();
        assert_eq!(w.free_percent, 9);
    }

    #[test]
    fn test_extract_worktree_name_from_worktrees_dir() {
        use std::path::Path;
        let path = Path::new("/home/user/repo/.worktrees/good-ash-20260205-204831");
        assert_eq!(
            extract_worktree_name(path),
            Some("good-ash-20260205-204831".to_string())
        );
    }

    #[test]
    fn test_extract_worktree_name_not_under_worktrees() {
        use std::path::Path;
        let path = Path::new("/home/user/repo/src/main");
        assert_eq!(extract_worktree_name(path), None);
    }

    #[test]
    fn test_extract_worktree_name_tmp_dir() {
        use std::path::Path;
        let path = Path::new("/tmp");
        assert_eq!(extract_worktree_name(path), None);
    }

    #[test]
    fn test_get_default_branch_prefers_origin_head() {
        // When origin/HEAD points to a branch, use that branch name
        assert_eq!(
            parse_origin_head("ref: refs/remotes/origin/develop"),
            Some("develop".to_string())
        );
    }

    #[test]
    fn test_get_default_branch_parses_main() {
        assert_eq!(
            parse_origin_head("ref: refs/remotes/origin/main"),
            Some("main".to_string())
        );
    }

    #[test]
    fn test_get_default_branch_parses_master() {
        assert_eq!(
            parse_origin_head("ref: refs/remotes/origin/master"),
            Some("master".to_string())
        );
    }

    #[test]
    fn test_get_default_branch_handles_empty() {
        assert_eq!(parse_origin_head(""), None);
    }

    #[test]
    fn test_get_default_branch_handles_malformed() {
        assert_eq!(parse_origin_head("not a valid ref"), None);
    }

    #[test]
    fn test_count_lines_in_content() {
        assert_eq!(count_lines_in_content("hello\nworld\n"), 2);
        assert_eq!(count_lines_in_content("single line"), 1);
        assert_eq!(count_lines_in_content(""), 0);
        assert_eq!(count_lines_in_content("a\nb\nc"), 3);
    }

    #[test]
    fn test_parse_active_skillsets_multiple_lines() {
        let output = "amol\nrust-dev\n";
        let result = parse_active_skillsets(output);
        assert_eq!(result, vec!["amol", "rust-dev"]);
    }

    #[test]
    fn test_parse_active_skillsets_single_line() {
        let output = "amol\n";
        let result = parse_active_skillsets(output);
        assert_eq!(result, vec!["amol"]);
    }

    #[test]
    fn test_parse_active_skillsets_empty() {
        let output = "";
        let result = parse_active_skillsets(output);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_active_skillsets_trims_whitespace() {
        let output = "  amol  \n  rust-dev  \n";
        let result = parse_active_skillsets(output);
        assert_eq!(result, vec!["amol", "rust-dev"]);
    }

    #[test]
    fn test_parse_active_skillsets_skips_blank_lines() {
        let output = "amol\n\n\nrust-dev\n";
        let result = parse_active_skillsets(output);
        assert_eq!(result, vec!["amol", "rust-dev"]);
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
