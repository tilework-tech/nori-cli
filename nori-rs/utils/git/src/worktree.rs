use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use crate::errors::GitToolingError;

const ADJECTIVES: &[&str] = &[
    "swift", "bright", "calm", "bold", "keen", "warm", "cool", "fair", "glad", "kind", "neat",
    "pure", "rich", "safe", "tall", "vast", "wise", "able", "busy", "deep", "easy", "fast", "good",
    "high", "just", "lean", "mild", "open", "real", "slim",
];

const NOUNS: &[&str] = &[
    "oak", "fox", "elm", "bay", "owl", "ash", "bee", "cod", "dew", "elk", "fig", "gem", "hen",
    "ivy", "jay", "kit", "log", "map", "net", "orb", "pen", "ray", "sun", "tea", "urn", "van",
    "web", "yak", "zen", "arc",
];

/// Information about a git worktree (excluding the main worktree).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
}

/// Parse the porcelain output of `git worktree list --porcelain` into structured
/// worktree info. The first record (main worktree) is always skipped.
pub fn parse_worktree_list_porcelain(output: &str) -> Vec<WorktreeInfo> {
    if output.trim().is_empty() {
        return Vec::new();
    }

    let records: Vec<&str> = output.trim().split("\n\n").collect();

    // Skip first record (main worktree)
    records
        .iter()
        .skip(1)
        .filter_map(|record| {
            let record = record.trim();
            if record.is_empty() {
                return None;
            }

            let mut worktree_path = None;
            let mut branch = None;

            for line in record.lines() {
                if let Some(path) = line.strip_prefix("worktree ") {
                    worktree_path = Some(PathBuf::from(path));
                } else if let Some(branch_ref) = line.strip_prefix("branch ") {
                    branch = Some(
                        branch_ref
                            .strip_prefix("refs/heads/")
                            .unwrap_or(branch_ref)
                            .to_string(),
                    );
                }
            }

            worktree_path.map(|path| WorktreeInfo { path, branch })
        })
        .collect()
}

/// List all git worktrees (excluding the main worktree) for the given repo root.
pub fn list_worktrees(repo_root: &Path) -> Result<Vec<WorktreeInfo>, GitToolingError> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .output()?;

    if !output.status.success() {
        return Err(GitToolingError::GitCommand {
            command: "git worktree list --porcelain".to_string(),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_worktree_list_porcelain(&stdout))
}

/// Create a git worktree at the specified path with the given branch name.
pub fn create_worktree(
    repo_root: &Path,
    worktree_path: &Path,
    branch_name: &str,
) -> Result<PathBuf, GitToolingError> {
    let output = Command::new("git")
        .args([
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            "-b",
            branch_name,
        ])
        .current_dir(repo_root)
        .output()?;

    if !output.status.success() {
        return Err(GitToolingError::GitCommand {
            command: format!("git worktree add {}", worktree_path.display()),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    Ok(worktree_path.to_path_buf())
}

/// Ensure an entry exists in the repo's .gitignore file.
pub fn ensure_gitignore_entry(repo_root: &Path, entry: &str) -> Result<(), GitToolingError> {
    let gitignore_path = repo_root.join(".gitignore");

    if gitignore_path.exists() {
        let content = std::fs::read_to_string(&gitignore_path)?;
        // Check if entry already exists (as a whole line)
        if content.lines().any(|line| line.trim() == entry) {
            return Ok(());
        }
        // Append the entry, ensuring it starts on a new line
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&gitignore_path)?;
        if !content.ends_with('\n') && !content.is_empty() {
            writeln!(file)?;
        }
        writeln!(file, "{entry}")?;
    } else {
        std::fs::write(&gitignore_path, format!("{entry}\n"))?;
    }

    Ok(())
}

/// Generate a unique worktree branch name with random words and a timestamp.
pub fn generate_worktree_branch_name() -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hash;
    use std::hash::Hasher;
    use std::time::SystemTime;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();

    // Use time-based hashing for pseudo-random word selection
    let mut hasher = DefaultHasher::new();
    now.as_nanos().hash(&mut hasher);
    let hash = hasher.finish();

    let adj = ADJECTIVES[hash as usize % ADJECTIVES.len()];
    let noun = NOUNS[(hash >> 16) as usize % NOUNS.len()];

    let timestamp = format_timestamp();
    format!("auto/{adj}-{noun}-{timestamp}")
}

/// Generate a worktree branch name from a prompt summary string.
///
/// Converts the summary to a lowercase slug, appends a timestamp, and prefixes
/// with `auto/`. If the summary is empty or whitespace-only after sanitization,
/// falls back to [`generate_worktree_branch_name`].
///
/// # Examples
/// - `"Fix auth bug"` → `"auto/fix-auth-bug-20260202-120000"`
/// - `""` → falls back to random name like `"auto/swift-oak-20260202-120000"`
pub fn summary_to_branch_name(summary: &str) -> String {
    // Sanitize: lowercase, replace non-alphanumeric with hyphens, collapse, trim
    let slug: String = summary
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive hyphens and trim leading/trailing
    let mut collapsed = String::with_capacity(slug.len());
    let mut last_was_hyphen = true; // start true to trim leading hyphens
    for c in slug.chars() {
        if c == '-' {
            if !last_was_hyphen {
                collapsed.push('-');
            }
            last_was_hyphen = true;
        } else {
            collapsed.push(c);
            last_was_hyphen = false;
        }
    }
    // Trim trailing hyphen
    let collapsed = collapsed.trim_end_matches('-');

    if collapsed.is_empty() {
        return generate_worktree_branch_name();
    }

    // Truncate to 40 chars at a word boundary (hyphen)
    let truncated = if collapsed.len() > 40 {
        match collapsed[..40].rfind('-') {
            Some(pos) => &collapsed[..pos],
            None => &collapsed[..40],
        }
    } else {
        collapsed
    };

    // Append timestamp
    let timestamp = format_timestamp();
    format!("auto/{truncated}-{timestamp}")
}

/// Rename a git worktree's branch in place.
///
/// Runs `git branch -m` to rename the branch. The worktree directory is left
/// unchanged so that processes running inside it are not disrupted.
pub fn rename_worktree_branch(
    repo_root: &Path,
    old_branch: &str,
    new_branch: &str,
) -> Result<(), GitToolingError> {
    let output = Command::new("git")
        .args(["branch", "-m", old_branch, new_branch])
        .current_dir(repo_root)
        .output()?;

    if !output.status.success() {
        return Err(GitToolingError::GitCommand {
            command: format!("git branch -m {old_branch} {new_branch}"),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }

    Ok(())
}

/// Format the current UTC timestamp as YYYYMMDD-HHMMSS.
fn format_timestamp() -> String {
    use std::time::SystemTime;

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();

    let secs = now.as_secs();
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days);

    format!("{year:04}{month:02}{day:02}-{hours:02}{minutes:02}{seconds:02}")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_ymd(days: u64) -> (i32, i32, i32) {
    // Algorithm from https://howardhinnant.github.io/date_algorithms.html
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as i32, d as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
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
        // Create an initial commit so HEAD exists (required for worktree add)
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
    fn test_create_worktree_creates_directory() {
        let temp = init_temp_repo();
        let worktree_path = temp.path().join(".worktrees").join("test-branch");
        std::fs::create_dir_all(temp.path().join(".worktrees")).unwrap();

        let result = create_worktree(temp.path(), &worktree_path, "test-branch");
        assert!(result.is_ok(), "create_worktree should succeed");
        let path = result.unwrap();
        assert!(path.exists(), "worktree directory should exist");

        // Verify the branch was created
        let output = Command::new("git")
            .args(["branch", "--list", "test-branch"])
            .current_dir(temp.path())
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&output.stdout);
        assert!(
            branches.contains("test-branch"),
            "branch 'test-branch' should exist"
        );
    }

    #[test]
    fn test_ensure_gitignore_adds_entry_when_missing() {
        let temp = init_temp_repo();
        // Create .gitignore without the entry
        std::fs::write(temp.path().join(".gitignore"), "node_modules/\n").unwrap();

        let result = ensure_gitignore_entry(temp.path(), ".worktrees/");
        assert!(result.is_ok());

        let content = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
        assert!(
            content.contains(".worktrees/"),
            "gitignore should contain .worktrees/"
        );
        // Original content should be preserved
        assert!(
            content.contains("node_modules/"),
            "original gitignore content should be preserved"
        );
    }

    #[test]
    fn test_ensure_gitignore_no_duplicate() {
        let temp = init_temp_repo();
        std::fs::write(temp.path().join(".gitignore"), ".worktrees/\n").unwrap();

        let result = ensure_gitignore_entry(temp.path(), ".worktrees/");
        assert!(result.is_ok());

        let content = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
        let count = content.matches(".worktrees/").count();
        assert_eq!(count, 1, ".worktrees/ should appear exactly once");
    }

    #[test]
    fn test_ensure_gitignore_creates_file_when_missing() {
        let temp = init_temp_repo();
        // Do NOT create .gitignore
        assert!(!temp.path().join(".gitignore").exists());

        let result = ensure_gitignore_entry(temp.path(), ".worktrees/");
        assert!(result.is_ok());

        let content = std::fs::read_to_string(temp.path().join(".gitignore")).unwrap();
        assert!(content.contains(".worktrees/"));
    }

    #[test]
    fn test_parse_worktree_list_porcelain_multiple_worktrees() {
        let output = "\
worktree /home/user/project
HEAD abc123def456
branch refs/heads/main

worktree /home/user/project/.worktrees/feature-auth
HEAD def456abc789
branch refs/heads/feature/auth

worktree /home/user/project/.worktrees/bugfix-login
HEAD 111222333444
branch refs/heads/bugfix/login
";
        let worktrees = parse_worktree_list_porcelain(output);
        assert_eq!(worktrees.len(), 2, "should skip main worktree, return 2");
        assert_eq!(
            worktrees[0].path,
            PathBuf::from("/home/user/project/.worktrees/feature-auth")
        );
        assert_eq!(worktrees[0].branch, Some("feature/auth".to_string()));
        assert_eq!(
            worktrees[1].path,
            PathBuf::from("/home/user/project/.worktrees/bugfix-login")
        );
        assert_eq!(worktrees[1].branch, Some("bugfix/login".to_string()));
    }

    #[test]
    fn test_parse_worktree_list_porcelain_only_main() {
        let output = "\
worktree /home/user/project
HEAD abc123def456
branch refs/heads/main
";
        let worktrees = parse_worktree_list_porcelain(output);
        assert_eq!(
            worktrees.len(),
            0,
            "should return empty when only main worktree exists"
        );
    }

    #[test]
    fn test_parse_worktree_list_porcelain_empty() {
        let worktrees = parse_worktree_list_porcelain("");
        assert_eq!(worktrees.len(), 0, "should return empty for empty input");
    }

    #[test]
    fn test_parse_worktree_list_porcelain_bare_and_detached() {
        let output = "\
worktree /home/user/project
HEAD abc123def456
branch refs/heads/main

worktree /home/user/project/.worktrees/detached-work
HEAD 999888777666
detached
";
        let worktrees = parse_worktree_list_porcelain(output);
        assert_eq!(worktrees.len(), 1);
        assert_eq!(
            worktrees[0].path,
            PathBuf::from("/home/user/project/.worktrees/detached-work")
        );
        assert_eq!(
            worktrees[0].branch, None,
            "detached worktree should have no branch"
        );
    }

    #[test]
    fn test_list_worktrees_returns_extra_worktrees() {
        let temp = init_temp_repo();
        let worktree_path = temp.path().join(".worktrees").join("test-wt");
        std::fs::create_dir_all(temp.path().join(".worktrees")).unwrap();
        create_worktree(temp.path(), &worktree_path, "test-wt-branch").unwrap();

        let worktrees = list_worktrees(temp.path()).unwrap();
        assert_eq!(worktrees.len(), 1, "should find one extra worktree");
        assert!(
            worktrees[0].path.to_string_lossy().contains("test-wt"),
            "worktree path should contain 'test-wt'"
        );
        assert_eq!(worktrees[0].branch, Some("test-wt-branch".to_string()));
    }

    #[test]
    fn test_list_worktrees_no_extras() {
        let temp = init_temp_repo();
        let worktrees = list_worktrees(temp.path()).unwrap();
        assert_eq!(
            worktrees.len(),
            0,
            "fresh repo should have no extra worktrees"
        );
    }

    #[test]
    fn test_generate_worktree_branch_name_format() {
        let name = generate_worktree_branch_name();
        assert!(
            name.starts_with("auto/"),
            "branch name should start with 'auto/', got: {name}"
        );
        // Should contain at least two hyphens (word-word-date-time)
        let after_prefix = &name["auto/".len()..];
        assert!(
            after_prefix.split('-').count() >= 4,
            "branch name should have at least 4 hyphen-separated parts after 'auto/', got: {name}"
        );
    }

    #[test]
    fn test_summary_to_branch_name_normal_summary() {
        let name = summary_to_branch_name("Fix auth bug");
        assert!(
            name.starts_with("auto/fix-auth-bug-"),
            "branch name should start with 'auto/fix-auth-bug-', got: {name}"
        );
        // Should end with a timestamp (YYYYMMDD-HHMMSS)
        let after_slug = name.strip_prefix("auto/fix-auth-bug-").unwrap();
        assert_eq!(
            after_slug.len(),
            15,
            "timestamp portion should be 15 chars (YYYYMMDD-HHMMSS), got: {after_slug}"
        );
    }

    #[test]
    fn test_summary_to_branch_name_strips_special_chars() {
        let name = summary_to_branch_name("Add dark mode!!!");
        assert!(
            name.starts_with("auto/add-dark-mode-"),
            "special chars should be stripped, got: {name}"
        );
    }

    #[test]
    fn test_summary_to_branch_name_empty_falls_back_to_random() {
        let name = summary_to_branch_name("");
        assert!(
            name.starts_with("auto/"),
            "empty summary should fall back to random name, got: {name}"
        );
        // Should look like a random name (adjective-noun-date-time), not just "auto/-date-time"
        let after_prefix = &name["auto/".len()..];
        assert!(
            after_prefix.split('-').count() >= 4,
            "fallback should have at least 4 parts, got: {name}"
        );
        // Should not start with a digit (should start with an adjective)
        assert!(
            after_prefix.chars().next().unwrap().is_alphabetic(),
            "fallback should start with a letter, got: {name}"
        );
    }

    #[test]
    fn test_summary_to_branch_name_truncates_long_summary() {
        let name = summary_to_branch_name(
            "Implement comprehensive user authentication with OAuth2 and JWT tokens",
        );
        assert!(name.starts_with("auto/"));
        // The slug portion (before timestamp) should be capped
        let after_prefix = &name["auto/".len()..];
        // Total after prefix should be slug + hyphen + timestamp (15 chars)
        // So slug should be at most ~40 chars
        let parts: Vec<&str> = after_prefix.rsplitn(3, '-').collect();
        // parts[0] = HHMMSS, parts[1] = YYYYMMDD, parts[2] = slug
        assert!(
            parts.len() == 3,
            "should have slug-YYYYMMDD-HHMMSS structure, got: {after_prefix}"
        );
        let slug = parts[2];
        assert!(
            slug.len() <= 40,
            "slug should be at most 40 chars, got {} chars: {slug}",
            slug.len()
        );
    }

    #[test]
    fn test_summary_to_branch_name_collapses_consecutive_hyphens() {
        let name = summary_to_branch_name("fix   multiple   spaces");
        assert!(
            name.starts_with("auto/fix-multiple-spaces-"),
            "consecutive hyphens should be collapsed, got: {name}"
        );
    }

    #[test]
    fn test_rename_worktree_branch_renames_branch_only() {
        let temp = init_temp_repo();
        let worktrees_dir = temp.path().join(".worktrees");
        std::fs::create_dir_all(&worktrees_dir).unwrap();

        // Create initial worktree
        let wt_path = worktrees_dir.join("old-name");
        create_worktree(temp.path(), &wt_path, "auto/old-name").unwrap();
        assert!(wt_path.exists());

        // Rename the branch only
        let result = rename_worktree_branch(temp.path(), "auto/old-name", "auto/new-name");
        assert!(
            result.is_ok(),
            "rename_worktree_branch should succeed: {:?}",
            result.err()
        );

        // Directory should remain at original path
        assert!(
            wt_path.exists(),
            "worktree directory should still exist at original path"
        );

        // New branch should exist
        let output = Command::new("git")
            .args(["branch", "--list", "auto/new-name"])
            .current_dir(temp.path())
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&output.stdout);
        assert!(
            branches.contains("auto/new-name"),
            "new branch name should exist"
        );

        // Old branch should not exist
        let output = Command::new("git")
            .args(["branch", "--list", "auto/old-name"])
            .current_dir(temp.path())
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&output.stdout);
        assert!(
            !branches.contains("auto/old-name"),
            "old branch name should not exist"
        );
    }
}
