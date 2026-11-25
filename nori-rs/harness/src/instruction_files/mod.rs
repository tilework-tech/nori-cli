//! The instruction files an agent loads from a folder, found on the
//! machine that runs the session, with their token counts.

pub mod token_count;

use std::path::Path;
use std::path::PathBuf;

use token_count::TokenCount;
use token_count::count_tokens;

fn is_git_marker(path: &Path) -> bool {
    path.is_file() || path.join("HEAD").is_file()
}

/// Simple enum to identify agent type for instruction file activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKindSimple {
    Claude,
    Codex,
    Gemini,
}

/// Represents an instruction file with its activation status.
#[derive(Debug, Clone)]
pub struct InstructionFile {
    /// Path to the instruction file.
    pub path: PathBuf,
    /// Whether this file is active for the current agent.
    pub active: bool,
    /// Token count for the file (only computed for active files).
    pub token_count: Option<TokenCount>,
}

/// Detect agent kind from a model/agent string.
///
/// Returns `Some(AgentKindSimple)` if the string matches a known agent pattern,
/// or `None` if unknown.
pub fn detect_agent_kind(agent: &str) -> Option<AgentKindSimple> {
    let lower = agent.to_lowercase();
    if lower.starts_with("claude") {
        Some(AgentKindSimple::Claude)
    } else if lower.starts_with("codex") {
        Some(AgentKindSimple::Codex)
    } else if lower.starts_with("gemini") || matches!(lower.as_str(), "antigravity" | "agy") {
        Some(AgentKindSimple::Gemini)
    } else {
        None
    }
}

/// Discover ALL instruction files in the directory hierarchy and mark them as active/inactive
/// based on the current agent's activation algorithm.
///
/// Files are discovered from git root (or cwd if no git root) to cwd, plus user-level configs.
/// The activation algorithm varies by agent:
/// - Claude: activates .claude/CLAUDE.md, CLAUDE.md, CLAUDE.local.md (all can be active per dir)
/// - Codex: activates AGENTS.override.md OR AGENTS.md per dir (preferring override)
/// - Gemini: activates only GEMINI.md per dir (no hidden variants, no overrides)
pub fn discover_all_instruction_files(
    cwd: &Path,
    agent_kind: Option<AgentKindSimple>,
) -> Vec<InstructionFile> {
    // In debug builds, allow E2E tests to mock instruction files for consistent snapshots.
    // This returns a constant list to ensure banner width is consistent across machines.
    #[cfg(debug_assertions)]
    if std::env::var("NORI_MOCK_INSTRUCTION_FILES").is_ok() {
        return vec![InstructionFile {
            path: std::path::PathBuf::from("~/.claude/CLAUDE.md"),
            active: true,
            token_count: None,
        }];
    }

    discover_all_instruction_files_with_paths(
        cwd,
        agent_kind,
        dirs::home_dir().as_deref(),
        default_managed_policy_dir().as_deref(),
    )
}

/// Default platform-specific directory containing the managed-policy CLAUDE.md
/// that Claude Code loads in addition to user/project files.
fn default_managed_policy_dir() -> Option<PathBuf> {
    if cfg!(target_os = "linux") {
        Some(PathBuf::from("/etc/claude-code"))
    } else if cfg!(target_os = "macos") {
        Some(PathBuf::from("/Library/Application Support/ClaudeCode"))
    } else if cfg!(target_os = "windows") {
        Some(PathBuf::from(r"C:\Program Files\ClaudeCode"))
    } else {
        None
    }
}

/// Internal function that discovers instruction files with an optional custom home directory.
/// Used by tests that want to inject a fake home directory but don't care about the
/// managed-policy path.
#[cfg(test)]
fn discover_all_instruction_files_with_home(
    cwd: &Path,
    agent_kind: Option<AgentKindSimple>,
    home_dir: Option<&Path>,
) -> Vec<InstructionFile> {
    discover_all_instruction_files_with_paths(cwd, agent_kind, home_dir, None)
}

/// Internal function that discovers instruction files with optional custom home and managed-policy
/// directories. Tests can inject paths to avoid touching real filesystem locations.
pub fn discover_all_instruction_files_with_paths(
    cwd: &Path,
    agent_kind: Option<AgentKindSimple>,
    home_dir: Option<&Path>,
    managed_policy_dir: Option<&Path>,
) -> Vec<InstructionFile> {
    // Build the full chain from cwd up to filesystem root, recording whether we
    // saw a `.git` marker and at which level.
    let mut chain: Vec<PathBuf> = Vec::new();
    let mut current = cwd.to_path_buf();
    let mut git_root: Option<PathBuf> = None;

    loop {
        chain.push(current.clone());

        if git_root.is_none() && is_git_marker(&current.join(".git")) {
            git_root = Some(current.clone());
        }

        if !current.pop() {
            break;
        }
    }

    // Choose the set of directories to search based on the agent's documented behavior.
    //
    // - Claude Code: walks all the way up to filesystem root, regardless of `.git`.
    //   See https://code.claude.com/docs/en/memory.
    // - Codex / Gemini: walk up only as far as the git root; if no git root exists,
    //   look at cwd only.
    // - Unknown agent: same as Codex/Gemini (conservative).
    let search_dirs: Vec<PathBuf> = match agent_kind {
        Some(AgentKindSimple::Claude) => chain.iter().rev().cloned().collect(),
        Some(AgentKindSimple::Codex) | Some(AgentKindSimple::Gemini) | None => {
            if let Some(root) = &git_root {
                let mut dirs: Vec<PathBuf> = Vec::new();
                let mut saw_root = false;
                for p in chain.iter().rev() {
                    if !saw_root {
                        if p == root {
                            saw_root = true;
                        } else {
                            continue;
                        }
                    }
                    dirs.push(p.clone());
                }
                dirs
            } else {
                vec![cwd.to_path_buf()]
            }
        }
    };

    let mut found: Vec<InstructionFile> = Vec::new();

    // Track which directories have override files (for Codex algorithm)
    let mut dirs_with_override: std::collections::HashSet<PathBuf> =
        std::collections::HashSet::new();

    // First pass: discover all files and detect overrides
    let mut discovered: Vec<(PathBuf, PathBuf)> = Vec::new(); // (file_path, parent_dir)

    for dir in &search_dirs {
        // Check for all instruction file types in this directory
        let candidates = [
            ("CLAUDE.md", true),
            ("CLAUDE.local.md", true),
            ("AGENTS.md", true),
            ("AGENTS.override.md", true),
            ("GEMINI.md", true),
        ];

        for (filename, _) in candidates {
            let file_path = dir.join(filename);
            if file_path.is_file() {
                if filename == "AGENTS.override.md" {
                    dirs_with_override.insert(dir.clone());
                }
                discovered.push((file_path, dir.clone()));
            }
        }

        // Check hidden .claude directory
        let claude_dir = dir.join(".claude");
        if claude_dir.is_dir() {
            let hidden_claude = claude_dir.join("CLAUDE.md");
            if hidden_claude.is_file() {
                discovered.push((hidden_claude, dir.clone()));
            }
        }
    }

    // Discover home directory config files
    // These are user-level configs that apply globally:
    // - ~/.claude/CLAUDE.md for Claude
    // - ~/.codex/AGENTS.md for Codex
    // - ~/.gemini/GEMINI.md for Gemini
    let mut home_configs: Vec<(PathBuf, PathBuf)> = Vec::new();
    if let Some(home) = home_dir {
        // Check for Claude home config: ~/.claude/CLAUDE.md
        let claude_home = home.join(".claude").join("CLAUDE.md");
        if claude_home.is_file() {
            home_configs.push((claude_home, home.join(".claude")));
        }

        // Check for Codex home config: ~/.codex/AGENTS.md
        let codex_home = home.join(".codex").join("AGENTS.md");
        if codex_home.is_file() {
            home_configs.push((codex_home, home.join(".codex")));
        }

        // Check for Gemini home config: ~/.gemini/GEMINI.md
        let gemini_home = home.join(".gemini").join("GEMINI.md");
        if gemini_home.is_file() {
            home_configs.push((gemini_home, home.join(".gemini")));
        }
    }

    // Discover managed-policy CLAUDE.md (Claude Code only).
    // This is a system-level CLAUDE.md that the Claude Code agent loads in addition
    // to user/project files (e.g. /etc/claude-code/CLAUDE.md on Linux).
    let mut policy_configs: Vec<(PathBuf, PathBuf)> = Vec::new();
    if matches!(agent_kind, Some(AgentKindSimple::Claude))
        && let Some(policy_dir) = managed_policy_dir
    {
        let policy_file = policy_dir.join("CLAUDE.md");
        if policy_file.is_file() {
            policy_configs.push((policy_file, policy_dir.to_path_buf()));
        }
    }

    // Build the final list, lowest-precedence first: managed-policy, then home, then ancestor walk.
    let mut all_discovered = policy_configs;
    all_discovered.extend(home_configs);
    all_discovered.extend(discovered);

    // Deduplicate by path so a file discovered both via the ancestor walk and the
    // home-config / managed-policy passes appears exactly once.
    let mut seen_paths: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    all_discovered.retain(|(path, _)| seen_paths.insert(path.clone()));

    // Second pass: apply activation algorithm
    for (file_path, parent_dir) in all_discovered {
        let filename = file_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let active = match agent_kind {
            Some(AgentKindSimple::Claude) => {
                // Claude activates: CLAUDE.md, CLAUDE.local.md (basename match covers
                // both `<dir>/CLAUDE.md` and `<dir>/.claude/CLAUDE.md`).
                filename == "CLAUDE.md" || filename == "CLAUDE.local.md"
            }
            Some(AgentKindSimple::Codex) => {
                // Codex activates: AGENTS.override.md OR AGENTS.md (prefer override)
                if filename == "AGENTS.override.md" {
                    true
                } else if filename == "AGENTS.md" {
                    // Only active if no override exists in this directory
                    !dirs_with_override.contains(&parent_dir)
                } else {
                    false
                }
            }
            Some(AgentKindSimple::Gemini) => {
                // Gemini activates: only GEMINI.md (no hidden, no overrides)
                filename == "GEMINI.md"
            }
            None => {
                // Unknown agent: nothing is active
                false
            }
        };

        let token_count = if active {
            std::fs::read_to_string(&file_path)
                .ok()
                .map(|contents| count_tokens(&contents, agent_kind))
        } else {
            None
        };

        found.push(InstructionFile {
            path: file_path,
            active,
            token_count,
        });
    }

    found
}

#[cfg(test)]
mod tests;
