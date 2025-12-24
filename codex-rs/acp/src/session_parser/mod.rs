//! Session transcript parsing for Claude, Codex, and Gemini agents
//!
//! This module provides parsers for extracting token usage metadata from
//! agent-specific JSONL session transcripts. Each agent has its own format:
//!
//! - **Codex**: JSONL with `event_msg` events containing `token_count` payloads
//! - **Gemini**: Single JSON object with tokens tracked per message
//! - **Claude**: JSONL with `usage` objects in assistant message entries
//!
//! # Example
//!
//! ```no_run
//! use codex_acp::session_parser::{parse_session_transcript, AgentSessionFormat};
//! use std::path::PathBuf;
//!
//! let path = PathBuf::from("session-codex.jsonl");
//! let report = parse_session_transcript(AgentSessionFormat::Codex, path)?;
//!
//! if let Some(report) = report {
//!     println!("Input tokens: {}", report.token_usage.input_tokens);
//!     println!("Output tokens: {}", report.token_usage.output_tokens);
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod claude;
mod codex;
mod gemini;

pub use claude::parse_claude_session;
pub use codex::parse_codex_session;
pub use gemini::parse_gemini_session;

use codex_protocol::protocol::TokenUsage;
use std::io;
use std::path::PathBuf;

/// Token usage report extracted from a session transcript
#[derive(Debug, Clone)]
pub struct TokenUsageReport {
    /// Token usage statistics from the agent
    pub token_usage: TokenUsage,
}

/// Identifies the agent format for session transcript parsing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSessionFormat {
    /// Codex agent format (JSONL with event_msg/token_count)
    Codex,
    /// Gemini agent format (single JSON with messages array)
    Gemini,
    /// Claude agent format (JSONL with usage in assistant messages)
    Claude,
}

/// Parse a session transcript file and extract token usage metadata
///
/// Returns `Ok(Some(report))` if the session contains token usage data,
/// `Ok(None)` if the session is valid but contains no token data,
/// or `Err` if the file cannot be parsed.
///
/// # Arguments
///
/// * `format` - The agent format to use for parsing
/// * `path` - Path to the session transcript file
///
/// # Errors
///
/// Returns an error if:
/// - The file cannot be opened or read
/// - The file content is malformed for the specified format
pub fn parse_session_transcript(
    format: AgentSessionFormat,
    path: PathBuf,
) -> io::Result<Option<TokenUsageReport>> {
    match format {
        AgentSessionFormat::Codex => parse_codex_session(path),
        AgentSessionFormat::Gemini => parse_gemini_session(path),
        AgentSessionFormat::Claude => parse_claude_session(path),
    }
}
