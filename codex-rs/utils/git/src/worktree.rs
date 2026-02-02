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

    // Format timestamp as YYYYMMDD-HHMMSS
    let secs = now.as_secs();
    // Simple UTC date/time calculation
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since epoch to year/month/day (simplified)
    let (year, month, day) = days_to_ymd(days);

    format!("auto/{adj}-{noun}-{year:04}{month:02}{day:02}-{hours:02}{minutes:02}{seconds:02}")
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
}
