//! Git repository identity used by project trust settings.

use std::path::Path;
use std::path::PathBuf;

/// Resolve a path to the primary repository working directory.
///
/// `git --git-common-dir` points linked worktrees back to the main repository,
/// so a single trust decision applies throughout the project.
pub fn resolve_root_git_project_for_trust(cwd: &Path) -> Option<PathBuf> {
    let base = if cwd.is_dir() { cwd } else { cwd.parent()? };
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--git-common-dir"])
        .current_dir(base)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let common_dir = String::from_utf8(output.stdout).ok()?;
    let common_dir = Path::new(common_dir.trim());
    let common_dir = if common_dir.is_absolute() {
        common_dir.to_path_buf()
    } else {
        base.join(common_dir)
    };
    let common_dir = std::fs::canonicalize(&common_dir).unwrap_or(common_dir);
    common_dir.parent().map(Path::to_path_buf)
}
