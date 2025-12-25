//! Session transcript parsing for Claude, Codex, and Gemini agents.
//!
//! This module provides parsers for extracting token usage and metadata from
//! agent session transcript files. Each agent stores session data in different
//! formats and locations:
//!
//! - **Codex**: `~/.codex/sessions/<YEAR>/<MM>/<DD>/rollout-<ISODATE>T<HH-MM-SS>-<SESSION_GUID>.jsonl`
//! - **Gemini**: `~/.gemini/tmp/<HASHED_PATHS>/chats/session-<ISODATE>T<HH-MM>-<SESSIONID>.json`
//! - **Claude**: `~/.claude/projects/<PROJECT_PATH>/<SESSIONID>.jsonl`
//!
//! Session discovery logic for finding these files is out of scope for this module
//! and will be implemented when integrating with the TUI/status command.

use codex_protocol::protocol::TokenUsage;
use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;
use thiserror::Error;

/// Agent type identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentKind {
    Claude,
    Codex,
    Gemini,
}

/// Token usage report extracted from a session transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsageReport {
    /// The agent that created this session.
    pub agent_kind: AgentKind,
    /// Session identifier (UUID or derived from filename).
    pub session_id: String,
    /// Path to the transcript file.
    pub transcript_path: PathBuf,
    /// Aggregated token usage across the session.
    pub token_usage: TokenUsage,
    /// Model context window size, if available in transcript metadata.
    pub model_context_window: Option<i64>,
}

/// Errors that can occur during session transcript parsing.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("empty session file")]
    EmptyFile,

    #[error("session ID not found in transcript")]
    MissingSessionId,

    #[error("token arithmetic overflow")]
    TokenOverflow,
}

/// Parse a Codex session transcript file.
///
/// Codex sessions are stored as JSONL files with events including token count updates.
/// The session ID is derived from the filename pattern since it's not embedded in the content.
pub async fn parse_codex_session(path: &std::path::Path) -> Result<TokenUsageReport, ParseError> {
    let text = tokio::fs::read_to_string(path).await?;

    if text.trim().is_empty() {
        return Err(ParseError::EmptyFile);
    }

    let mut last_token_usage: Option<TokenUsage> = None;
    let mut model_context_window: Option<i64> = None;

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to parse line as JSON: {line:?}, error: {e}");
                continue;
            }
        };

        // Look for event_msg with type token_count
        if v.get("type").and_then(|t| t.as_str()) == Some("event_msg")
            && let Some(payload) = v.get("payload")
            && payload.get("type").and_then(|t| t.as_str()) == Some("token_count")
            && let Some(info) = payload.get("info")
        {
            // Extract total_token_usage
            if let Some(total_usage) = info.get("total_token_usage") {
                let usage: TokenUsage = serde_json::from_value(total_usage.clone())?;
                last_token_usage = Some(usage);
            }

            // Extract model_context_window
            if let Some(mcw) = info.get("model_context_window") {
                model_context_window = mcw.as_i64();
            }
        }
    }

    let token_usage = last_token_usage.ok_or(ParseError::EmptyFile)?;

    // Derive session ID from filename (e.g., "session-codex.jsonl" -> "codex")
    let session_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();

    Ok(TokenUsageReport {
        agent_kind: AgentKind::Codex,
        session_id,
        transcript_path: path.to_path_buf(),
        token_usage,
        model_context_window,
    })
}

/// Parse a Gemini session transcript file.
///
/// Gemini sessions are stored as a single JSON file with a messages array.
/// The session ID is in the root-level metadata.
pub async fn parse_gemini_session(path: &std::path::Path) -> Result<TokenUsageReport, ParseError> {
    let text = tokio::fs::read_to_string(path).await?;

    if text.trim().is_empty() {
        return Err(ParseError::EmptyFile);
    }

    let root: serde_json::Value = serde_json::from_str(&text)?;

    // Extract session ID from root
    let session_id = root
        .get("sessionId")
        .and_then(|s| s.as_str())
        .ok_or(ParseError::MissingSessionId)?
        .to_string();

    // Aggregate tokens from all messages
    let mut total_input = 0i64;
    let mut total_output = 0i64;
    let mut total_cached = 0i64;
    let mut total_thoughts = 0i64;

    if let Some(messages) = root.get("messages").and_then(|m| m.as_array()) {
        for message in messages {
            if let Some(tokens) = message.get("tokens") {
                total_input = total_input
                    .checked_add(
                        tokens
                            .get("input")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(0),
                    )
                    .ok_or(ParseError::TokenOverflow)?;
                total_output = total_output
                    .checked_add(
                        tokens
                            .get("output")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(0),
                    )
                    .ok_or(ParseError::TokenOverflow)?;
                total_cached = total_cached
                    .checked_add(
                        tokens
                            .get("cached")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(0),
                    )
                    .ok_or(ParseError::TokenOverflow)?;
                total_thoughts = total_thoughts
                    .checked_add(
                        tokens
                            .get("thoughts")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(0),
                    )
                    .ok_or(ParseError::TokenOverflow)?;
            }
        }
    }

    let total_tokens = total_input
        .checked_add(total_output)
        .and_then(|t| t.checked_add(total_thoughts))
        .ok_or(ParseError::TokenOverflow)?;

    let token_usage = TokenUsage {
        input_tokens: total_input,
        cached_input_tokens: total_cached,
        output_tokens: total_output,
        reasoning_output_tokens: total_thoughts, // Gemini's "thoughts" map to reasoning tokens
        total_tokens,
    };

    Ok(TokenUsageReport {
        agent_kind: AgentKind::Gemini,
        session_id,
        transcript_path: path.to_path_buf(),
        token_usage,
        model_context_window: None, // Gemini format doesn't include this
    })
}

/// Parse a Claude session transcript file.
///
/// Claude sessions are stored as JSONL files with per-message usage objects.
/// The session ID is in the message metadata.
pub async fn parse_claude_session(path: &std::path::Path) -> Result<TokenUsageReport, ParseError> {
    let text = tokio::fs::read_to_string(path).await?;

    if text.trim().is_empty() {
        return Err(ParseError::EmptyFile);
    }

    let mut session_id: Option<String> = None;
    let mut total_input = 0i64;
    let mut total_output = 0i64;
    let mut total_cache_read = 0i64;

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }

        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to parse line as JSON: {line:?}, error: {e}");
                continue;
            }
        };

        // Extract session ID from first message that has it
        if session_id.is_none()
            && let Some(sid) = v.get("sessionId").and_then(|s| s.as_str())
        {
            session_id = Some(sid.to_string());
        }

        // Look for messages with usage field
        if let Some(message) = v.get("message")
            && let Some(usage) = message.get("usage")
        {
            total_input = total_input
                .checked_add(
                    usage
                        .get("input_tokens")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0),
                )
                .ok_or(ParseError::TokenOverflow)?;
            total_output = total_output
                .checked_add(
                    usage
                        .get("output_tokens")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0),
                )
                .ok_or(ParseError::TokenOverflow)?;
            total_cache_read = total_cache_read
                .checked_add(
                    usage
                        .get("cache_read_input_tokens")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0),
                )
                .ok_or(ParseError::TokenOverflow)?;
        }
    }

    let session_id = session_id.ok_or(ParseError::MissingSessionId)?;

    let total_tokens = total_input
        .checked_add(total_output)
        .and_then(|t| t.checked_add(total_cache_read))
        .ok_or(ParseError::TokenOverflow)?;

    let token_usage = TokenUsage {
        input_tokens: total_input,
        cached_input_tokens: total_cache_read, // Map cache_read to cached_input
        output_tokens: total_output,
        reasoning_output_tokens: 0, // Claude doesn't separate reasoning tokens
        total_tokens,
    };

    Ok(TokenUsageReport {
        agent_kind: AgentKind::Claude,
        session_id,
        transcript_path: path.to_path_buf(),
        token_usage,
        model_context_window: None, // Claude format doesn't include this
    })
}

/// Parse a session transcript file based on the specified agent format.
///
/// This is a convenience wrapper that dispatches to the appropriate format-specific
/// parser based on the `format` parameter.
///
/// # Arguments
///
/// * `format` - The agent type that created the session transcript
/// * `path` - Path to the session transcript file
///
/// # Returns
///
/// Returns a [`TokenUsageReport`] containing aggregated token usage and metadata,
/// or a [`ParseError`] if the file cannot be read or parsed.
///
/// # Examples
///
/// ```no_run
/// use codex_acp::session_parser::{parse_session_transcript, AgentKind};
/// use std::path::Path;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let report = parse_session_transcript(
///     AgentKind::Claude,
///     Path::new("~/.claude/projects/my-project/session-123.jsonl")
/// ).await?;
///
/// println!("Total tokens: {}", report.token_usage.total_tokens);
/// # Ok(())
/// # }
/// ```
pub async fn parse_session_transcript(
    format: AgentKind,
    path: &std::path::Path,
) -> Result<TokenUsageReport, ParseError> {
    match format {
        AgentKind::Claude => parse_claude_session(path).await,
        AgentKind::Codex => parse_codex_session(path).await,
        AgentKind::Gemini => parse_gemini_session(path).await,
    }
}
