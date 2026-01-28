//! Transcript location discovery for external ACP agents.
//!
//! This module provides functions to detect the current running transcript file
//! for Claude Code, Codex, and Gemini agents. This enables session statistics
//! display (e.g., token usage) in the TUI footer.
//!
//! ## Discovery Method
//!
//! Transcript discovery uses a unified approach that searches for the session's
//! first user message within transcript files. This is done using shell tools
//! (`rg` if available, falling back to `grep`) to avoid coupling to any specific
//! agent's JSON schema.
//!
//! Each agent stores transcripts in its own base directory:
//! - **Claude Code**: `~/.claude/projects/`
//! - **Codex**: `~/.codex/sessions/`
//! - **Gemini**: `~/.gemini/tmp/`
//!
//! Discovery requires a first_message to match against; without it, no transcript
//! will be detected. This prevents returning the wrong transcript.

use crate::AgentKind;
use std::fs;
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
/// **Deprecated:** This function always returns an error because transcript
/// discovery now requires a first_message to avoid returning the wrong transcript.
/// Use `discover_transcript_for_agent_with_message` instead.
///
/// # Arguments
///
/// * `cwd` - The current working directory to find transcripts for
/// * `agent` - The agent kind to search for transcripts
///
/// # Returns
///
/// Always returns `NoSessionsFound` error. Use `discover_transcript_for_agent_with_message`
/// with a first_message parameter instead.
pub fn discover_transcript_for_agent(
    cwd: &Path,
    _agent: AgentKind,
) -> Result<TranscriptLocation, DiscoveryError> {
    // No fallback - require first_message to avoid wrong transcript
    Err(DiscoveryError::NoSessionsFound(cwd.to_path_buf()))
}

/// Discover the transcript location for a specific agent kind with first-message matching.
///
/// This is the unified discovery method that searches for transcripts by matching the
/// first user message across all agent types. It uses shell tools (rg or grep) to
/// search recursively through the agent's transcript directory, avoiding coupling
/// to any specific JSON schema.
///
/// # Arguments
///
/// * `cwd` - The current working directory (unused, kept for API compatibility)
/// * `agent` - The agent kind to search for transcripts
/// * `first_message` - The first user message of the current session (required)
///
/// # Returns
///
/// Returns the discovered transcript location, or an error if:
/// - No first_message is provided (returns NoSessionsFound)
/// - No matching transcript is found (returns NoSessionsFound)
/// - Home directory cannot be determined (returns HomeNotFound)
///
/// **Note:** Unlike previous implementations, this does NOT fall back to most recent
/// file when no match is found. It's better to show no tokens than wrong tokens.
pub fn discover_transcript_for_agent_with_message(
    cwd: &Path,
    agent: AgentKind,
    first_message: Option<&str>,
) -> Result<TranscriptLocation, DiscoveryError> {
    let home = dirs::home_dir().ok_or_else(|| DiscoveryError::HomeNotFound("~".to_string()))?;
    let base_dir = home.join(agent.transcript_base_dir());

    if !base_dir.exists() {
        return Err(DiscoveryError::NoSessionsFound(cwd.to_path_buf()));
    }

    // Require first_message - no fallback to avoid wrong transcript
    let first_message =
        first_message.ok_or_else(|| DiscoveryError::NoSessionsFound(cwd.to_path_buf()))?;

    // Normalize the message for searching (strip whitespace, truncate)
    let search_pattern = normalize_message_for_matching(first_message);

    let transcript_path = find_transcript_by_shell_search(&base_dir, &search_pattern)
        .ok_or_else(|| DiscoveryError::NoSessionsFound(cwd.to_path_buf()))?;

    // Extract session ID from filename
    let session_id = transcript_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Parse token usage from the transcript
    let token_breakdown = parse_transcript_tokens(&transcript_path, agent);

    Ok(TranscriptLocation {
        agent_kind: agent,
        transcript_path,
        session_id,
        token_breakdown,
    })
}

/// Maximum age for transcript files to be considered (2 days in seconds).
const MAX_TRANSCRIPT_AGE_SECS: u64 = 2 * 24 * 60 * 60;

/// Find a transcript file by searching for the normalized first message using shell tools.
///
/// This function uses `rg` (ripgrep) if available, falling back to `grep`, to search
/// recursively through `.json` and `.jsonl` files in the given directory. This approach
/// avoids coupling to any specific agent's JSON schema and works across different
/// transcript formats.
///
/// The search pattern is a normalized message fingerprint (whitespace stripped, truncated
/// to 20 characters) which should uniquely identify a session by its first user message.
///
/// # Arguments
///
/// * `base_dir` - The base directory to search in (recursively)
/// * `normalized_message` - The normalized message fingerprint to search for
///
/// # Returns
///
/// The path to the matching transcript file, or `None` if no match is found.
/// If multiple files match, returns the most recently modified one.
fn find_transcript_by_shell_search(base_dir: &Path, normalized_message: &str) -> Option<PathBuf> {
    // Try rg first (faster), fall back to grep
    let matching_files = search_with_rg(base_dir, normalized_message)
        .or_else(|| search_with_grep(base_dir, normalized_message))?;

    if matching_files.is_empty() {
        return None;
    }

    // Find the most recently modified file among matches
    let now = SystemTime::now();
    let max_age = std::time::Duration::from_secs(MAX_TRANSCRIPT_AGE_SECS);

    let mut best_match: Option<(PathBuf, SystemTime)> = None;

    for file_path in matching_files {
        let path = PathBuf::from(&file_path);
        if !path.exists() {
            continue;
        }

        let metadata = match fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let modified = match metadata.modified() {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Skip files older than max age
        if let Ok(age) = now.duration_since(modified)
            && age > max_age
        {
            continue;
        }

        match &best_match {
            None => best_match = Some((path, modified)),
            Some((_, prev_time)) if modified > *prev_time => {
                best_match = Some((path, modified));
            }
            _ => {}
        }
    }

    best_match.map(|(path, _)| path)
}

/// Search for matching files using ripgrep (rg).
fn search_with_rg(base_dir: &Path, pattern: &str) -> Option<Vec<String>> {
    use std::process::Command;

    let output = Command::new("rg")
        .args([
            "--files-with-matches",
            "--glob",
            "*.json",
            "--glob",
            "*.jsonl",
            "--fixed-strings",
            pattern,
            base_dir.to_str()?,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        // rg returns exit code 1 when no matches found, which is not an error
        // Exit code 2+ indicates actual errors
        if output.status.code() == Some(1) {
            return Some(vec![]);
        }
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<String> = stdout.lines().map(String::from).collect();
    Some(files)
}

/// Search for matching files using grep (fallback).
fn search_with_grep(base_dir: &Path, pattern: &str) -> Option<Vec<String>> {
    use std::process::Command;

    // Use find + grep combination for recursive search
    let output = Command::new("grep")
        .args([
            "-r",
            "-l",
            "--include=*.json",
            "--include=*.jsonl",
            "-F",
            pattern,
            base_dir.to_str()?,
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        // grep returns exit code 1 when no matches found
        if output.status.code() == Some(1) {
            return Some(vec![]);
        }
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<String> = stdout.lines().map(String::from).collect();
    Some(files)
}

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
    fn test_find_transcript_by_shell_search_finds_matching_file_in_flat_directory() {
        use pretty_assertions::assert_eq;
        use std::thread;
        use std::time::Duration;

        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        // Create three transcript files with different first messages
        let file1 = base_dir.join("session-aaa.jsonl");
        let file2 = base_dir.join("session-bbb.jsonl");
        let file3 = base_dir.join("session-ccc.jsonl");

        // File 1: "Help me debug this"
        {
            let mut f = fs::File::create(&file1).unwrap();
            writeln!(f, "Help me debug this problem").unwrap();
        }

        thread::sleep(Duration::from_millis(50));

        // File 2: "Implement the feature" (the one we're looking for)
        {
            let mut f = fs::File::create(&file2).unwrap();
            writeln!(f, "Implement the feature for users").unwrap();
        }

        thread::sleep(Duration::from_millis(50));

        // File 3: "Write some tests" - most recent but wrong message
        {
            let mut f = fs::File::create(&file3).unwrap();
            writeln!(f, "Write some tests for this").unwrap();
        }

        // Search for a substring that appears in file2 - shell search uses literal matching
        let result = find_transcript_by_shell_search(base_dir, "Implement the feature");

        assert!(result.is_some(), "Should find a matching transcript");
        let found_path = result.unwrap();
        assert_eq!(found_path.file_name().unwrap(), "session-bbb.jsonl");
    }

    #[test]
    fn test_find_transcript_by_shell_search_finds_file_in_nested_directories() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        // Create nested directory structure like Codex: YYYY/MM/DD/
        let nested_dir = base_dir.join("2026").join("01").join("28");
        fs::create_dir_all(&nested_dir).unwrap();

        let file = nested_dir.join("session.jsonl");
        {
            let mut f = fs::File::create(&file).unwrap();
            writeln!(f, "Find me in nested directories please").unwrap();
        }

        // Search should find file in nested directory
        let result = find_transcript_by_shell_search(base_dir, "Find me in nested");

        assert!(
            result.is_some(),
            "Should find transcript in nested directory"
        );
        let found_path = result.unwrap();
        assert!(
            found_path.to_string_lossy().contains("2026/01/28"),
            "Found path should be in nested directory"
        );
    }

    #[test]
    fn test_find_transcript_by_shell_search_returns_none_without_match() {
        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        // Create a file with a different message
        let file = base_dir.join("session.jsonl");
        {
            let mut f = fs::File::create(&file).unwrap();
            writeln!(f, "Some completely different content").unwrap();
        }

        // Search for non-existent message
        let result = find_transcript_by_shell_search(base_dir, "This string does not exist");

        assert!(result.is_none(), "Should return None when no match found");
    }

    #[test]
    fn test_find_transcript_by_shell_search_searches_both_jsonl_and_json() {
        use pretty_assertions::assert_eq;

        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        // Create a .json file (like Gemini uses)
        let json_file = base_dir.join("session.json");
        {
            let mut f = fs::File::create(&json_file).unwrap();
            writeln!(f, "Find the json file content here").unwrap();
        }

        // Search should find .json file
        let result = find_transcript_by_shell_search(base_dir, "Find the json file");

        assert!(result.is_some(), "Should find .json file");
        let found_path = result.unwrap();
        assert_eq!(found_path.file_name().unwrap(), "session.json");
    }

    #[test]
    fn test_find_transcript_by_shell_search_picks_most_recent_on_multiple_matches() {
        use pretty_assertions::assert_eq;
        use std::thread;
        use std::time::Duration;

        let temp_dir = TempDir::new().unwrap();
        let base_dir = temp_dir.path();

        // Create two files with the SAME content
        let file1 = base_dir.join("session-older.jsonl");
        let file2 = base_dir.join("session-newer.jsonl");

        // Older file
        {
            let mut f = fs::File::create(&file1).unwrap();
            writeln!(f, "Duplicate content here for testing").unwrap();
        }

        thread::sleep(Duration::from_millis(50));

        // Newer file with same content
        {
            let mut f = fs::File::create(&file2).unwrap();
            writeln!(f, "Duplicate content here for testing").unwrap();
        }

        let result = find_transcript_by_shell_search(base_dir, "Duplicate content here");

        assert!(result.is_some());
        let found_path = result.unwrap();
        assert_eq!(
            found_path.file_name().unwrap(),
            "session-newer.jsonl",
            "Should pick the most recently modified file"
        );
    }
}
