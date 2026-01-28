//! Transcript location discovery for external ACP agents.
//!
//! This module provides functions to detect the current running transcript file
//! for Claude Code, Codex, and Gemini agents. This enables session statistics
//! display (e.g., token usage) in the TUI footer.
//!
//! ## Agent Transcript Locations
//!
//! Each agent stores session transcripts in different locations:
//!
//! - **Claude Code**: `~/.claude/projects/<transformed-path>/<session-uuid>.jsonl`
//!   - Path is transformed by replacing non-alphanumeric chars with dashes
//!   - Example: `/home/user/project` → `-home-user-project`
//!
//! - **Codex**: `~/.codex/sessions/YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl`
//!   - Sessions matched by comparing CWD in first JSON line
//!
//! - **Gemini**: `~/.gemini/tmp/<sha256-hash>/chats/<session>.json`
//!   - Hash is SHA256 of the canonical working directory path

use crate::AgentKind;
use sha2::Digest;
use sha2::Sha256;
use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;
use thiserror::Error;

/// Token usage breakdown extracted from a transcript.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TranscriptTokenUsage {
    /// Total input tokens.
    pub input_tokens: i64,
    /// Total output tokens.
    pub output_tokens: i64,
    /// Cached input tokens (subset of input_tokens).
    pub cached_tokens: i64,
}

impl TranscriptTokenUsage {
    /// Returns the total tokens (input + output).
    pub fn total(&self) -> i64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

/// Information about a discovered transcript location.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptLocation {
    /// The agent that created this transcript.
    pub agent_kind: AgentKind,
    /// Path to the transcript file.
    pub transcript_path: PathBuf,
    /// Session identifier (UUID or derived from filename).
    pub session_id: String,
    /// Detailed token usage breakdown, if available.
    pub token_breakdown: Option<TranscriptTokenUsage>,
}

/// Errors that can occur during transcript discovery.
#[derive(Debug, Error)]
pub enum DiscoveryError {
    /// Agent home directory not found.
    #[error("agent home directory not found: {0}")]
    HomeNotFound(String),

    /// No sessions found for the current working directory.
    #[error("no sessions found for working directory: {0}")]
    NoSessionsFound(PathBuf),

    /// I/O error during discovery.
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// JSON parse error (for Codex CWD matching).
    #[error("JSON parse error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Discover the transcript location for a specific agent kind.
///
/// This function is useful when you already know which agent is running
/// (e.g., from the ACP backend configuration) and don't need to detect
/// it from environment variables.
///
/// **Note:** For Claude Code, this falls back to the most recent file which
/// may not be the correct transcript. Use `discover_transcript_for_agent_with_message`
/// with the first user message for more accurate matching.
///
/// # Arguments
///
/// * `cwd` - The current working directory to find transcripts for
/// * `agent` - The agent kind to search for transcripts
///
/// # Returns
///
/// Returns the discovered transcript location, or an error if no transcript
/// could be found.
pub fn discover_transcript_for_agent(
    cwd: &Path,
    agent: AgentKind,
) -> Result<TranscriptLocation, DiscoveryError> {
    match agent {
        AgentKind::ClaudeCode => find_current_transcript_claude(cwd),
        AgentKind::Codex => find_current_transcript_codex(cwd),
        AgentKind::Gemini => find_current_transcript_gemini(cwd),
    }
}

/// Discover the transcript location for a specific agent kind with first-message matching.
///
/// This is the preferred method for Claude Code as it uses the first user message
/// to accurately identify the correct transcript file. For other agents (Codex, Gemini),
/// the first_message parameter is ignored as they use different matching strategies.
///
/// # Arguments
///
/// * `cwd` - The current working directory to find transcripts for
/// * `agent` - The agent kind to search for transcripts
/// * `first_message` - The first user message of the current session (required for Claude Code)
///
/// # Returns
///
/// Returns the discovered transcript location, or an error if no transcript
/// could be found. For Claude Code, returns an error if no first_message is provided
/// or no matching transcript is found (does NOT fall back to most recent file).
pub fn discover_transcript_for_agent_with_message(
    cwd: &Path,
    agent: AgentKind,
    first_message: Option<&str>,
) -> Result<TranscriptLocation, DiscoveryError> {
    match agent {
        AgentKind::ClaudeCode => find_current_transcript_claude_with_message(cwd, first_message),
        // Codex and Gemini use CWD/hash matching, not first-message matching
        AgentKind::Codex => find_current_transcript_codex(cwd),
        AgentKind::Gemini => find_current_transcript_gemini(cwd),
    }
}

/// Transform a path to Claude Code's project directory name format.
///
/// Claude Code transforms working directory paths by:
/// 1. Resolving symlinks (if possible)
/// 2. Replacing all non-alphanumeric characters (except `-`) with `-`
/// 3. Adding a leading `-` if not present
///
/// # Example
///
/// `/home/user/my-project` → `-home-user-my-project`
pub fn transform_path_to_claude_project_name(path: &Path) -> String {
    // Try to resolve symlinks, fall back to original path if not possible
    let resolved = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

    // Convert to string
    let path_str = resolved.to_string_lossy();

    // Replace all non-alphanumeric characters (except -) with -
    let mut transformed: String = path_str
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    // Add leading dash if not present
    if !transformed.starts_with('-') {
        transformed.insert(0, '-');
    }

    transformed
}

/// Find the current transcript for Claude Code.
///
/// Looks in `~/.claude/projects/<transformed-path>/` and returns the most
/// recently modified `.jsonl` file.
pub fn find_current_transcript_claude(cwd: &Path) -> Result<TranscriptLocation, DiscoveryError> {
    let home = dirs::home_dir().ok_or_else(|| DiscoveryError::HomeNotFound("~".to_string()))?;

    let project_name = transform_path_to_claude_project_name(cwd);
    let project_dir = home.join(".claude").join("projects").join(&project_name);

    if !project_dir.exists() {
        return Err(DiscoveryError::NoSessionsFound(cwd.to_path_buf()));
    }

    let transcript_path = most_recent_file(&project_dir, "jsonl")?
        .ok_or_else(|| DiscoveryError::NoSessionsFound(cwd.to_path_buf()))?;

    // Extract session ID from filename (UUID before .jsonl)
    let session_id = transcript_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Parse token usage from the transcript
    let token_breakdown = parse_transcript_tokens(&transcript_path, AgentKind::ClaudeCode);
    Ok(TranscriptLocation {
        agent_kind: AgentKind::ClaudeCode,
        transcript_path,
        session_id,
        token_breakdown,
    })
}

/// Find the current transcript for Codex.
///
/// Traverses `~/.codex/sessions/YYYY/MM/DD/` and finds sessions where the
/// `cwd` field in the first JSON line matches the provided working directory.
/// Returns the most recently modified matching session.
pub fn find_current_transcript_codex(cwd: &Path) -> Result<TranscriptLocation, DiscoveryError> {
    let home = dirs::home_dir().ok_or_else(|| DiscoveryError::HomeNotFound("~".to_string()))?;
    let sessions_root = home.join(".codex").join("sessions");

    if !sessions_root.exists() {
        return Err(DiscoveryError::NoSessionsFound(cwd.to_path_buf()));
    }

    // Normalize the CWD for comparison
    let normalized_cwd = normalize_path(cwd);

    let mut most_recent: Option<(PathBuf, String, SystemTime)> = None;

    // Traverse year/month/day structure
    for year_entry in read_dir_sorted_desc(&sessions_root)? {
        if !year_entry.path().is_dir() {
            continue;
        }

        for month_entry in read_dir_sorted_desc(&year_entry.path())? {
            if !month_entry.path().is_dir() {
                continue;
            }

            for day_entry in read_dir_sorted_desc(&month_entry.path())? {
                if !day_entry.path().is_dir() {
                    continue;
                }

                for session_entry in read_dir_sorted_desc(&day_entry.path())? {
                    let path = session_entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }

                    // Read first line to get session metadata
                    if let Ok(meta) = read_codex_session_meta(&path) {
                        let session_cwd = normalize_path(Path::new(&meta.cwd));

                        if session_cwd == normalized_cwd {
                            let modified = session_entry.metadata()?.modified()?;

                            match &most_recent {
                                None => {
                                    most_recent = Some((path, meta.id, modified));
                                }
                                Some((_, _, prev_time)) if modified > *prev_time => {
                                    most_recent = Some((path, meta.id, modified));
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }

    let (transcript_path, session_id, _) =
        most_recent.ok_or_else(|| DiscoveryError::NoSessionsFound(cwd.to_path_buf()))?;

    // Parse token usage from the transcript
    let token_breakdown = parse_transcript_tokens(&transcript_path, AgentKind::Codex);
    Ok(TranscriptLocation {
        agent_kind: AgentKind::Codex,
        transcript_path,
        session_id,
        token_breakdown,
    })
}

/// Session metadata extracted from the first line of a Codex session file.
#[derive(Debug)]
struct CodexSessionMeta {
    id: String,
    cwd: String,
}

/// Read the session metadata from the first line of a Codex JSONL file.
fn read_codex_session_meta(path: &Path) -> Result<CodexSessionMeta, DiscoveryError> {
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);

    if let Some(first_line) = reader.lines().next() {
        let line = first_line?;
        let v: serde_json::Value = serde_json::from_str(&line)?;

        // Extract payload.id and payload.cwd
        let id = v
            .get("payload")
            .and_then(|p| p.get("id"))
            .and_then(|i| i.as_str())
            .unwrap_or("")
            .to_string();

        let cwd = v
            .get("payload")
            .and_then(|p| p.get("cwd"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        Ok(CodexSessionMeta { id, cwd })
    } else {
        Err(DiscoveryError::IoError(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "empty session file",
        )))
    }
}

/// Find the current transcript for Gemini.
///
/// Computes SHA256 hash of the canonical path, then looks in
/// `~/.gemini/tmp/<hash>/chats/` for the most recently modified `.json` file.
pub fn find_current_transcript_gemini(cwd: &Path) -> Result<TranscriptLocation, DiscoveryError> {
    let home = dirs::home_dir().ok_or_else(|| DiscoveryError::HomeNotFound("~".to_string()))?;

    // Compute SHA256 hash of the canonical path
    let canonical = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let path_str = canonical.to_string_lossy();
    let hash = format!("{:x}", Sha256::digest(path_str.as_bytes()));

    let chats_dir = home.join(".gemini").join("tmp").join(&hash).join("chats");

    if !chats_dir.exists() {
        return Err(DiscoveryError::NoSessionsFound(cwd.to_path_buf()));
    }

    let transcript_path = most_recent_file(&chats_dir, "json")?
        .ok_or_else(|| DiscoveryError::NoSessionsFound(cwd.to_path_buf()))?;

    // Extract session ID from filename
    let session_id = transcript_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Parse token usage from the transcript
    let token_breakdown = parse_transcript_tokens(&transcript_path, AgentKind::Gemini);
    Ok(TranscriptLocation {
        agent_kind: AgentKind::Gemini,
        transcript_path,
        session_id,
        token_breakdown,
    })
}

/// Normalize a path for comparison.
///
/// Cleans the path, converts to absolute, and resolves symlinks if possible.
fn normalize_path(path: &Path) -> PathBuf {
    let cleaned = path.to_path_buf();

    // Try to get absolute path
    let absolute = if cleaned.is_absolute() {
        cleaned
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&cleaned))
            .unwrap_or(cleaned)
    };

    // Try to resolve symlinks
    fs::canonicalize(&absolute).unwrap_or(absolute)
}

/// Read directory entries sorted in descending order by name.
fn read_dir_sorted_desc(path: &Path) -> std::io::Result<Vec<fs::DirEntry>> {
    let mut entries: Vec<_> = fs::read_dir(path)?
        .filter_map(std::result::Result::ok)
        .collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
    Ok(entries)
}

/// Find the most recently modified file with the given extension in a directory.
fn most_recent_file(path: &Path, extension: &str) -> std::io::Result<Option<PathBuf>> {
    let mut most_recent: Option<(PathBuf, SystemTime)> = None;

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let entry_path = entry.path();

        if entry_path.extension().and_then(|e| e.to_str()) == Some(extension)
            && entry_path.is_file()
        {
            let metadata = entry.metadata()?;
            let modified = metadata.modified()?;

            match &most_recent {
                None => most_recent = Some((entry_path, modified)),
                Some((_, prev_time)) if modified > *prev_time => {
                    most_recent = Some((entry_path, modified));
                }
                _ => {}
            }
        }
    }

    Ok(most_recent.map(|(path, _)| path))
}

/// Maximum age for transcript files to be considered (2 days in seconds).
const MAX_TRANSCRIPT_AGE_SECS: u64 = 2 * 24 * 60 * 60;

/// Maximum number of lines to search for the first user message.
const MAX_LINES_TO_SEARCH: usize = 10;

/// Length to truncate normalized messages to for matching.
const NORMALIZED_MESSAGE_LENGTH: usize = 20;

/// Normalize a message for matching by stripping all whitespace and truncating.
///
/// This creates a "fingerprint" of the message that can be used to match
/// transcripts by their first user message.
fn normalize_message_for_matching(message: &str) -> String {
    let stripped: String = message.chars().filter(|c| !c.is_whitespace()).collect();
    stripped.chars().take(NORMALIZED_MESSAGE_LENGTH).collect()
}

/// Extract the first user text message from a Claude transcript file.
///
/// Reads up to the first 10 lines OR until the first `"type":"user"` entry
/// with a text content is found. Returns `None` if no user text message
/// is found within these constraints.
///
/// This handles the Claude Code transcript format where:
/// - Lines 1-N may be progress/hook entries
/// - User entries have `"type":"user"` with `message.content[0].text`
/// - Tool result entries have `"type":"user"` but `message.content[0].type` is "tool_result"
fn extract_first_user_message(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let reader = BufReader::new(file);

    for (line_num, line_result) in reader.lines().enumerate() {
        // Stop after MAX_LINES_TO_SEARCH lines
        if line_num >= MAX_LINES_TO_SEARCH {
            break;
        }

        let line = match line_result {
            Ok(l) => l,
            Err(_) => continue, // Skip problematic lines, don't fail entirely
        };
        if line.trim().is_empty() {
            continue;
        }

        let v: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Check if this is a user entry
        if v.get("type").and_then(serde_json::Value::as_str) != Some("user") {
            continue;
        }

        // Found a user entry - now check if it has text content
        // (not a tool_result which also has type:"user")
        if let Some(message) = v.get("message")
            && let Some(content) = message.get("content")
            && let Some(content_array) = content.as_array()
        {
            for item in content_array {
                // Check for text type specifically
                if item.get("type").and_then(serde_json::Value::as_str) == Some("text")
                    && let Some(text) = item.get("text").and_then(serde_json::Value::as_str)
                {
                    return Some(text.to_string());
                }
            }
        }

        // This was a user entry but didn't have text content (e.g., tool_result)
        // Continue searching for an actual text message
    }

    None
}

/// Find a Claude transcript file by matching the first user message.
///
/// Searches through all `.jsonl` files in the given directory, extracts the
/// first user message from each, and returns the path of the file whose
/// normalized first message matches the provided search query.
///
/// Only considers files modified within the last 2 days to avoid matching
/// stale transcripts. If multiple files match, returns the most recently
/// modified one.
///
/// # Arguments
///
/// * `project_dir` - The Claude project directory to search in
/// * `first_message` - The first user message to search for
///
/// # Returns
///
/// The path to the matching transcript file, or `None` if no match is found.
fn find_claude_transcript_by_first_message(
    project_dir: &Path,
    first_message: &str,
) -> Option<PathBuf> {
    let normalized_search = normalize_message_for_matching(first_message);
    let now = SystemTime::now();
    let max_age = std::time::Duration::from_secs(MAX_TRANSCRIPT_AGE_SECS);

    let mut best_match: Option<(PathBuf, SystemTime)> = None;

    let entries = fs::read_dir(project_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();

        // Only consider .jsonl files
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") || !path.is_file() {
            continue;
        }

        // Check file age - skip files older than 2 days
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified = match metadata.modified() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if let Ok(age) = now.duration_since(modified)
            && age > max_age
        {
            continue;
        }

        // Extract first user message and compare
        if let Some(file_first_message) = extract_first_user_message(&path) {
            let normalized_file_message = normalize_message_for_matching(&file_first_message);
            if normalized_file_message == normalized_search {
                // Found a match - track it if it's more recent than previous matches
                match &best_match {
                    None => best_match = Some((path, modified)),
                    Some((_, prev_time)) if modified > *prev_time => {
                        best_match = Some((path, modified));
                    }
                    _ => {}
                }
            }
        }
    }

    best_match.map(|(path, _)| path)
}

/// Find the current transcript for Claude Code using first-message matching.
///
/// This function requires a first_message to match against transcript files.
/// Unlike the basic `find_current_transcript_claude`, this does NOT fall back
/// to the most recent file if no match is found - it returns an error instead.
/// This prevents returning the wrong transcript.
///
/// # Arguments
///
/// * `cwd` - The current working directory (used to find the project directory)
/// * `first_message` - The first user message of the current session
///
/// # Returns
///
/// Returns the discovered transcript location, or an error if no matching
/// transcript could be found.
pub fn find_current_transcript_claude_with_message(
    cwd: &Path,
    first_message: Option<&str>,
) -> Result<TranscriptLocation, DiscoveryError> {
    let home = dirs::home_dir().ok_or_else(|| DiscoveryError::HomeNotFound("~".to_string()))?;

    let project_name = transform_path_to_claude_project_name(cwd);
    let project_dir = home.join(".claude").join("projects").join(&project_name);

    if !project_dir.exists() {
        return Err(DiscoveryError::NoSessionsFound(cwd.to_path_buf()));
    }

    // Require first_message - don't fall back to most_recent_file
    let first_message =
        first_message.ok_or_else(|| DiscoveryError::NoSessionsFound(cwd.to_path_buf()))?;

    let transcript_path = find_claude_transcript_by_first_message(&project_dir, first_message)
        .ok_or_else(|| DiscoveryError::NoSessionsFound(cwd.to_path_buf()))?;

    // Extract session ID from filename (UUID before .jsonl)
    let session_id = transcript_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Parse token usage from the transcript
    let token_breakdown = parse_transcript_tokens(&transcript_path, AgentKind::ClaudeCode);
    Ok(TranscriptLocation {
        agent_kind: AgentKind::ClaudeCode,
        transcript_path,
        session_id,
        token_breakdown,
    })
}

/// Parse token usage from a transcript file (synchronous).
///
/// This function reads the transcript file and extracts token usage breakdown.
/// It dispatches to agent-specific parsers based on the agent kind.
///
/// Returns `None` if the file cannot be read or contains no token data.
pub fn parse_transcript_tokens(path: &Path, agent: AgentKind) -> Option<TranscriptTokenUsage> {
    match agent {
        AgentKind::ClaudeCode => parse_claude_tokens(path),
        AgentKind::Codex => parse_codex_tokens(path),
        AgentKind::Gemini => parse_gemini_tokens(path),
    }
}

/// Parse total token usage from a transcript file (synchronous).
///
/// This function reads the transcript file and extracts total token usage.
/// It dispatches to agent-specific parsers based on the agent kind.
///
/// Returns `None` if the file cannot be read or contains no token data.
pub fn parse_transcript_total_tokens(path: &Path, agent: AgentKind) -> Option<i64> {
    parse_transcript_tokens(path, agent).map(|t| t.total())
}

/// Parse tokens from a Claude Code transcript file.
///
/// Claude sessions are JSONL files with per-message usage objects. Claude Code logs
/// multiple JSONL entries per API request (one per streaming delta), each containing
/// the same usage data. We deduplicate by `requestId` to avoid overcounting.
///
/// The usage object contains:
/// - `input_tokens`: Non-cached input tokens (typically small, 1-10)
/// - `cache_creation_input_tokens`: Tokens that were sent and cached for future use
/// - `cache_read_input_tokens`: Tokens read from cache (discounted/free)
/// - `output_tokens`: Output tokens generated
///
/// We calculate:
/// - `input_tokens` = `input_tokens` + `cache_creation_input_tokens` (total tokens sent)
/// - `cached_tokens` = `cache_read_input_tokens` (tokens read from cache)
///
/// Malformed lines are skipped to handle partially written transcripts.
fn parse_claude_tokens(path: &Path) -> Option<TranscriptTokenUsage> {
    let text = fs::read_to_string(path).ok()?;

    let mut total_input = 0i64;
    let mut total_output = 0i64;
    let mut total_cached = 0i64;
    let mut seen_request_ids = std::collections::HashSet::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }

        // Skip malformed lines instead of failing entirely
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        // Deduplicate by requestId - Claude Code logs multiple entries per request (streaming)
        // If an entry has a requestId we've seen before, skip it to avoid double-counting
        if let Some(request_id) = v.get("requestId").and_then(serde_json::Value::as_str)
            && !seen_request_ids.insert(request_id.to_string())
        {
            continue; // Already processed this request
        }

        // Look for messages with usage field
        if let Some(message) = v.get("message")
            && let Some(usage) = message.get("usage")
        {
            // input_tokens is only the non-cached portion
            total_input = total_input.saturating_add(
                usage
                    .get("input_tokens")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
            );
            // cache_creation_input_tokens are tokens that were sent and cached for future use
            // These count as input tokens (they were processed by the model)
            total_input = total_input.saturating_add(
                usage
                    .get("cache_creation_input_tokens")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
            );
            total_output = total_output.saturating_add(
                usage
                    .get("output_tokens")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
            );
            // cache_read_input_tokens are tokens read from cache (discounted/free)
            total_cached = total_cached.saturating_add(
                usage
                    .get("cache_read_input_tokens")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0),
            );
        }
    }

    let total = total_input.saturating_add(total_output);
    if total > 0 {
        Some(TranscriptTokenUsage {
            input_tokens: total_input,
            output_tokens: total_output,
            cached_tokens: total_cached,
        })
    } else {
        None
    }
}

/// Parse tokens from a Codex transcript file.
///
/// Codex sessions are JSONL files with token_count events that include
/// total_token_usage. We take the last one as the final total.
/// Malformed lines are skipped to handle partially written transcripts.
fn parse_codex_tokens(path: &Path) -> Option<TranscriptTokenUsage> {
    let text = fs::read_to_string(path).ok()?;

    let mut last_usage: Option<TranscriptTokenUsage> = None;

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }

        // Skip malformed lines instead of failing entirely
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };

        // Look for event_msg with type token_count
        if v.get("type").and_then(|t| t.as_str()) == Some("event_msg")
            && let Some(payload) = v.get("payload")
            && payload.get("type").and_then(|t| t.as_str()) == Some("token_count")
            && let Some(info) = payload.get("info")
            && let Some(total_usage) = info.get("total_token_usage")
        {
            let input = total_usage
                .get("input_tokens")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let output = total_usage
                .get("output_tokens")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            let cached = total_usage
                .get("cached_input_tokens")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);

            last_usage = Some(TranscriptTokenUsage {
                input_tokens: input,
                output_tokens: output,
                cached_tokens: cached,
            });
        }
    }

    last_usage.filter(|t| t.total() > 0)
}

/// Parse tokens from a Gemini transcript file.
///
/// Gemini sessions are JSON files with a messages array. Each message has
/// a tokens object with input, output, cached, and thoughts fields.
fn parse_gemini_tokens(path: &Path) -> Option<TranscriptTokenUsage> {
    let text = fs::read_to_string(path).ok()?;
    let root: serde_json::Value = serde_json::from_str(&text).ok()?;

    let mut total_input = 0i64;
    let mut total_output = 0i64;
    let mut total_cached = 0i64;

    if let Some(messages) = root.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            if let Some(tokens) = message.get("tokens") {
                total_input = total_input.saturating_add(
                    tokens
                        .get("input")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0),
                );
                // Include thoughts in output (reasoning tokens)
                total_output = total_output.saturating_add(
                    tokens
                        .get("output")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0),
                );
                total_output = total_output.saturating_add(
                    tokens
                        .get("thoughts")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0),
                );
                total_cached = total_cached.saturating_add(
                    tokens
                        .get("cached")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0),
                );
            }
        }
    }

    let total = total_input.saturating_add(total_output);
    if total > 0 {
        Some(TranscriptTokenUsage {
            input_tokens: total_input,
            output_tokens: total_output,
            cached_tokens: total_cached,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_transform_path_to_claude_project_name_basic() {
        let path = Path::new("/home/user/my-project");
        let result = transform_path_to_claude_project_name(path);
        assert_eq!(result, "-home-user-my-project");
    }

    #[test]
    fn test_transform_path_to_claude_project_name_special_chars() {
        // Path with special characters should have them replaced with dashes
        let path = Path::new("/home/user/My Projects(1)/app");
        let result = transform_path_to_claude_project_name(path);
        assert_eq!(result, "-home-user-My-Projects-1--app");
    }

    #[test]
    fn test_transform_path_to_claude_project_name_preserves_existing_dashes() {
        let path = Path::new("/home/user/my-cool-project");
        let result = transform_path_to_claude_project_name(path);
        assert_eq!(result, "-home-user-my-cool-project");
    }

    #[test]
    fn test_normalize_path_handles_relative() {
        let path = Path::new("./some/path");
        let normalized = normalize_path(path);
        assert!(normalized.is_absolute());
    }

    #[test]
    fn test_read_codex_session_meta_extracts_fields() {
        let temp_dir = TempDir::new().unwrap();
        let session_file = temp_dir.path().join("test-session.jsonl");

        {
            let mut f = fs::File::create(&session_file).unwrap();
            writeln!(
                f,
                r#"{{"type": "session_meta", "payload": {{"id": "test-id-123", "cwd": "/path/to/project"}}}}"#
            )
            .unwrap();
        }

        let meta = read_codex_session_meta(&session_file).unwrap();
        assert_eq!(meta.id, "test-id-123");
        assert_eq!(meta.cwd, "/path/to/project");
    }

    #[test]
    fn test_parse_claude_total_tokens() {
        let temp_dir = TempDir::new().unwrap();
        let transcript_file = temp_dir.path().join("session.jsonl");

        {
            let mut f = fs::File::create(&transcript_file).unwrap();
            // Two messages with token usage
            writeln!(
                f,
                r#"{{"message": {{"usage": {{"input_tokens": 100, "output_tokens": 50}}}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"message": {{"usage": {{"input_tokens": 200, "output_tokens": 75}}}}}}"#
            )
            .unwrap();
        }

        let tokens = parse_transcript_total_tokens(&transcript_file, AgentKind::ClaudeCode);
        assert_eq!(tokens, Some(425)); // 100 + 50 + 200 + 75
    }

    #[test]
    fn test_parse_claude_total_tokens_empty_file() {
        let temp_dir = TempDir::new().unwrap();
        let transcript_file = temp_dir.path().join("empty.jsonl");
        fs::File::create(&transcript_file).unwrap();

        let tokens = parse_transcript_total_tokens(&transcript_file, AgentKind::ClaudeCode);
        assert_eq!(tokens, None);
    }

    #[test]
    fn test_parse_codex_total_tokens() {
        let temp_dir = TempDir::new().unwrap();
        let transcript_file = temp_dir.path().join("session.jsonl");

        {
            let mut f = fs::File::create(&transcript_file).unwrap();
            // Token count events - we take the last one
            writeln!(
                f,
                r#"{{"type": "event_msg", "payload": {{"type": "token_count", "info": {{"total_token_usage": {{"input_tokens": 50, "output_tokens": 50}}}}}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type": "event_msg", "payload": {{"type": "token_count", "info": {{"total_token_usage": {{"input_tokens": 300, "output_tokens": 200}}}}}}}}"#
            )
            .unwrap();
        }

        let tokens = parse_transcript_total_tokens(&transcript_file, AgentKind::Codex);
        assert_eq!(tokens, Some(500)); // Last value: 300 + 200
    }

    #[test]
    fn test_parse_gemini_total_tokens() {
        let temp_dir = TempDir::new().unwrap();
        let transcript_file = temp_dir.path().join("session.json");

        {
            let mut f = fs::File::create(&transcript_file).unwrap();
            writeln!(
                f,
                r#"{{"messages": [{{"tokens": {{"input": 100, "output": 50, "thoughts": 25}}}}, {{"tokens": {{"input": 200, "output": 100, "thoughts": 50}}}}]}}"#
            )
            .unwrap();
        }

        let tokens = parse_transcript_total_tokens(&transcript_file, AgentKind::Gemini);
        assert_eq!(tokens, Some(525)); // 100 + 50 + 25 + 200 + 100 + 50
    }

    #[test]
    fn test_parse_transcript_total_tokens_dispatches_correctly() {
        let temp_dir = TempDir::new().unwrap();

        // Test Claude dispatch
        let claude_file = temp_dir.path().join("claude.jsonl");
        {
            let mut f = fs::File::create(&claude_file).unwrap();
            writeln!(
                f,
                r#"{{"message": {{"usage": {{"input_tokens": 100, "output_tokens": 50}}}}}}"#
            )
            .unwrap();
        }
        let tokens = parse_transcript_total_tokens(&claude_file, AgentKind::ClaudeCode);
        assert_eq!(tokens, Some(150));

        // Test Codex dispatch
        let codex_file = temp_dir.path().join("codex.jsonl");
        {
            let mut f = fs::File::create(&codex_file).unwrap();
            writeln!(
                f,
                r#"{{"type": "event_msg", "payload": {{"type": "token_count", "info": {{"total_token_usage": {{"input_tokens": 200, "output_tokens": 100}}}}}}}}"#
            )
            .unwrap();
        }
        let tokens = parse_transcript_total_tokens(&codex_file, AgentKind::Codex);
        assert_eq!(tokens, Some(300)); // 200 + 100

        // Test Gemini dispatch
        let gemini_file = temp_dir.path().join("gemini.json");
        {
            let mut f = fs::File::create(&gemini_file).unwrap();
            writeln!(
                f,
                r#"{{"messages": [{{"tokens": {{"input": 200, "output": 100}}}}]}}"#
            )
            .unwrap();
        }
        let tokens = parse_transcript_total_tokens(&gemini_file, AgentKind::Gemini);
        assert_eq!(tokens, Some(300));
    }

    #[test]
    fn test_parse_claude_tokens_deduplicates_by_request_id() {
        use pretty_assertions::assert_eq;

        let temp_dir = TempDir::new().unwrap();
        let transcript_file = temp_dir.path().join("session.jsonl");

        {
            let mut f = fs::File::create(&transcript_file).unwrap();
            // Simulate streaming: same requestId appears multiple times (this is how Claude Code works)
            // First request: 3 entries with same usage (streaming deltas)
            writeln!(f, r#"{{"requestId": "req_001", "message": {{"usage": {{"input_tokens": 3, "cache_creation_input_tokens": 1000, "cache_read_input_tokens": 0, "output_tokens": 50}}}}}}"#).unwrap();
            writeln!(f, r#"{{"requestId": "req_001", "message": {{"usage": {{"input_tokens": 3, "cache_creation_input_tokens": 1000, "cache_read_input_tokens": 0, "output_tokens": 50}}}}}}"#).unwrap();
            writeln!(f, r#"{{"requestId": "req_001", "message": {{"usage": {{"input_tokens": 3, "cache_creation_input_tokens": 1000, "cache_read_input_tokens": 0, "output_tokens": 50}}}}}}"#).unwrap();
            // Second request: 2 entries with same usage (streaming deltas)
            writeln!(f, r#"{{"requestId": "req_002", "message": {{"usage": {{"input_tokens": 5, "cache_creation_input_tokens": 500, "cache_read_input_tokens": 1000, "output_tokens": 25}}}}}}"#).unwrap();
            writeln!(f, r#"{{"requestId": "req_002", "message": {{"usage": {{"input_tokens": 5, "cache_creation_input_tokens": 500, "cache_read_input_tokens": 1000, "output_tokens": 25}}}}}}"#).unwrap();
        }

        let usage = parse_transcript_tokens(&transcript_file, AgentKind::ClaudeCode).unwrap();

        // Should deduplicate: only count each requestId once
        // input = (3 + 1000) + (5 + 500) = 1508 (input_tokens + cache_creation per unique request)
        // output = 50 + 25 = 75
        // cached = 0 + 1000 = 1000
        assert_eq!(usage.input_tokens, 1508);
        assert_eq!(usage.output_tokens, 75);
        assert_eq!(usage.cached_tokens, 1000);
    }

    #[test]
    fn test_parse_claude_tokens_includes_cache_creation_in_input() {
        use pretty_assertions::assert_eq;

        let temp_dir = TempDir::new().unwrap();
        let transcript_file = temp_dir.path().join("session.jsonl");

        {
            let mut f = fs::File::create(&transcript_file).unwrap();
            // Single request with cache_creation_input_tokens
            // This tests that cache_creation is added to input total
            writeln!(f, r#"{{"requestId": "req_001", "message": {{"usage": {{"input_tokens": 10, "cache_creation_input_tokens": 5000, "cache_read_input_tokens": 2000, "output_tokens": 100}}}}}}"#).unwrap();
        }

        let usage = parse_transcript_tokens(&transcript_file, AgentKind::ClaudeCode).unwrap();

        // input = 10 + 5000 = 5010 (input_tokens + cache_creation_input_tokens)
        // output = 100
        // cached = 2000 (cache_read_input_tokens)
        assert_eq!(usage.input_tokens, 5010);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.cached_tokens, 2000);
        // Total context = input + output = 5110
        assert_eq!(usage.total(), 5110);
    }

    #[test]
    fn test_parse_claude_tokens_handles_entries_without_request_id() {
        use pretty_assertions::assert_eq;

        let temp_dir = TempDir::new().unwrap();
        let transcript_file = temp_dir.path().join("session.jsonl");

        {
            let mut f = fs::File::create(&transcript_file).unwrap();
            // Mix of entries with and without requestId
            // Entries without requestId should all be counted (backwards compatibility)
            writeln!(
                f,
                r#"{{"message": {{"usage": {{"input_tokens": 100, "output_tokens": 50}}}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"message": {{"usage": {{"input_tokens": 200, "output_tokens": 75}}}}}}"#
            )
            .unwrap();
            // Entry with requestId
            writeln!(f, r#"{{"requestId": "req_001", "message": {{"usage": {{"input_tokens": 10, "cache_creation_input_tokens": 500, "output_tokens": 25}}}}}}"#).unwrap();
        }

        let usage = parse_transcript_tokens(&transcript_file, AgentKind::ClaudeCode).unwrap();

        // Entries without requestId: 100 + 200 = 300 input, 50 + 75 = 125 output
        // Entry with requestId: 10 + 500 = 510 input, 25 output
        // Total: 810 input, 150 output
        assert_eq!(usage.input_tokens, 810);
        assert_eq!(usage.output_tokens, 150);
    }

    #[test]
    fn test_normalize_message_for_matching_strips_whitespace_and_truncates() {
        use pretty_assertions::assert_eq;

        // Basic whitespace stripping
        assert_eq!(
            normalize_message_for_matching("  hello world  "),
            "helloworld"
        );

        // Truncation to 20 characters
        assert_eq!(
            normalize_message_for_matching(
                "this is a very long message that exceeds twenty characters"
            ),
            "thisisaverylongmessa"
        );

        // Mixed: whitespace + truncation
        assert_eq!(
            normalize_message_for_matching("  Currently the transcript detection  "),
            "Currentlythetranscri"
        );

        // Short message stays as-is (minus whitespace)
        assert_eq!(normalize_message_for_matching("short"), "short");

        // Newlines and tabs count as whitespace
        assert_eq!(
            normalize_message_for_matching("hello\n\tworld"),
            "helloworld"
        );
    }

    #[test]
    fn test_extract_first_user_message_finds_user_text_in_first_10_lines() {
        use pretty_assertions::assert_eq;

        let temp_dir = TempDir::new().unwrap();
        let transcript_file = temp_dir.path().join("session.jsonl");

        {
            let mut f = fs::File::create(&transcript_file).unwrap();
            // Lines 1-6: non-user entries (progress, hooks, etc.)
            writeln!(f, r#"{{"type":"queue-operation","operation":"dequeue"}}"#).unwrap();
            writeln!(
                f,
                r#"{{"type":"progress","data":{{"type":"hook_progress"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"progress","data":{{"type":"hook_progress"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"progress","data":{{"type":"hook_progress"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"progress","data":{{"type":"hook_progress"}}}}"#
            )
            .unwrap();
            writeln!(
                f,
                r#"{{"type":"progress","data":{{"type":"hook_progress"}}}}"#
            )
            .unwrap();
            // Line 7: first user message with text content
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"Help me implement a feature"}}]}}}}"#).unwrap();
            // More lines that shouldn't be read
            writeln!(
                f,
                r#"{{"type":"assistant","message":{{"role":"assistant"}}}}"#
            )
            .unwrap();
        }

        let result = extract_first_user_message(&transcript_file);
        assert_eq!(result, Some("Help me implement a feature".to_string()));
    }

    #[test]
    fn test_extract_first_user_message_handles_tool_result_user_entries() {
        use pretty_assertions::assert_eq;

        let temp_dir = TempDir::new().unwrap();
        let transcript_file = temp_dir.path().join("session.jsonl");

        {
            let mut f = fs::File::create(&transcript_file).unwrap();
            // User entry that is a tool_result (not a text message) - should be skipped
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"tool_use_id":"toolu_123","type":"tool_result","content":"some result"}}]}}}}"#).unwrap();
            // Real user text message
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"Actual user prompt"}}]}}}}"#).unwrap();
        }

        let result = extract_first_user_message(&transcript_file);
        assert_eq!(result, Some("Actual user prompt".to_string()));
    }

    #[test]
    fn test_extract_first_user_message_returns_none_if_no_user_in_first_10_lines() {
        let temp_dir = TempDir::new().unwrap();
        let transcript_file = temp_dir.path().join("session.jsonl");

        {
            let mut f = fs::File::create(&transcript_file).unwrap();
            // 15 lines of non-user entries
            for i in 0..15 {
                writeln!(f, r#"{{"type":"progress","line":{i}}}"#).unwrap();
            }
            // User message after line 10 - should NOT be found
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"Too late"}}]}}}}"#).unwrap();
        }

        let result = extract_first_user_message(&transcript_file);
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_claude_transcript_by_first_message_matches_correct_file() {
        use pretty_assertions::assert_eq;
        use std::thread;
        use std::time::Duration;

        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path();

        // Create three transcript files with different first messages
        let file1 = project_dir.join("session-aaa.jsonl");
        let file2 = project_dir.join("session-bbb.jsonl");
        let file3 = project_dir.join("session-ccc.jsonl");

        // File 1: "Help me debug this"
        {
            let mut f = fs::File::create(&file1).unwrap();
            writeln!(f, r#"{{"type":"progress"}}"#).unwrap();
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"Help me debug this"}}]}}}}"#).unwrap();
        }

        // Small delay to ensure different mtimes
        thread::sleep(Duration::from_millis(50));

        // File 2: "Currently the transcript detection" (the one we're looking for)
        {
            let mut f = fs::File::create(&file2).unwrap();
            writeln!(f, r#"{{"type":"progress"}}"#).unwrap();
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"Currently the transcript detection implemented on this branch"}}]}}}}"#).unwrap();
        }

        thread::sleep(Duration::from_millis(50));

        // File 3: "Write some tests" - most recent but wrong message
        {
            let mut f = fs::File::create(&file3).unwrap();
            writeln!(f, r#"{{"type":"progress"}}"#).unwrap();
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"Write some tests"}}]}}}}"#).unwrap();
        }

        // Search for "Currently the transcript" - should match file2
        let result = find_claude_transcript_by_first_message(
            project_dir,
            "Currently the transcript detection",
        );

        assert!(result.is_some(), "Should find a matching transcript");
        let found_path = result.unwrap();
        assert_eq!(found_path.file_name().unwrap(), "session-bbb.jsonl");
    }

    #[test]
    fn test_find_claude_transcript_by_first_message_returns_none_when_no_match() {
        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path();

        // Create a transcript file with a different message
        let file1 = project_dir.join("session-aaa.jsonl");
        {
            let mut f = fs::File::create(&file1).unwrap();
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"Some other message"}}]}}}}"#).unwrap();
        }

        // Search for a message that doesn't exist
        let result = find_claude_transcript_by_first_message(
            project_dir,
            "This message does not exist anywhere",
        );

        assert!(result.is_none(), "Should return None when no match found");
    }

    #[test]
    fn test_find_claude_transcript_by_first_message_picks_most_recent_on_multiple_matches() {
        use pretty_assertions::assert_eq;
        use std::thread;
        use std::time::Duration;

        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path();

        // Create two files with the SAME first message (edge case: duplicate sessions)
        let file1 = project_dir.join("session-older.jsonl");
        let file2 = project_dir.join("session-newer.jsonl");

        // Older file
        {
            let mut f = fs::File::create(&file1).unwrap();
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"Help me implement"}}]}}}}"#).unwrap();
        }

        thread::sleep(Duration::from_millis(50));

        // Newer file with same message
        {
            let mut f = fs::File::create(&file2).unwrap();
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"Help me implement"}}]}}}}"#).unwrap();
        }

        let result = find_claude_transcript_by_first_message(project_dir, "Help me implement");

        assert!(result.is_some());
        let found_path = result.unwrap();
        assert_eq!(
            found_path.file_name().unwrap(),
            "session-newer.jsonl",
            "Should pick the most recently modified file"
        );
    }

    #[test]
    fn test_find_claude_transcript_by_first_message_ignores_old_files() {
        use filetime::FileTime;
        use std::time::Duration;
        use std::time::SystemTime;

        let temp_dir = TempDir::new().unwrap();
        let project_dir = temp_dir.path();

        // Create a file with matching message but set mtime to 3 days ago
        let old_file = project_dir.join("session-old.jsonl");
        {
            let mut f = fs::File::create(&old_file).unwrap();
            writeln!(f, r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"Help me implement"}}]}}}}"#).unwrap();
        }

        // Set modification time to 3 days ago
        let three_days_ago = SystemTime::now() - Duration::from_secs(3 * 24 * 60 * 60);
        let mtime = FileTime::from_system_time(three_days_ago);
        filetime::set_file_mtime(&old_file, mtime).unwrap();

        // Search for the message - should NOT find it because file is too old
        let result = find_claude_transcript_by_first_message(project_dir, "Help me implement");

        assert!(result.is_none(), "Should not match files older than 2 days");
    }

    #[test]
    fn test_discover_transcript_for_agent_with_first_message_returns_error_without_message() {
        // When no first_message is provided for Claude, it should return an error
        // (not fallback to most_recent_file which could be wrong)
        let temp_dir = TempDir::new().unwrap();

        // We can't easily test the full discover_transcript_for_agent flow without
        // mocking home directory, but we can test find_current_transcript_claude_with_message
        // Returns error when first_message is None
        let result = find_current_transcript_claude_with_message(temp_dir.path(), None);

        assert!(
            result.is_err(),
            "Should return error when no first_message provided"
        );
    }
}
