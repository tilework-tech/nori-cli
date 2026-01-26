//! Schema types for Nori transcript persistence.

use serde::Deserialize;
use serde::Serialize;
use std::path::PathBuf;

/// Schema version for forward compatibility.
pub const TRANSCRIPT_SCHEMA_VERSION: u8 = 1;

/// A single line in a transcript JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptLine {
    /// ISO 8601 timestamp
    pub ts: String,
    /// Schema version
    pub v: u8,
    /// The entry payload
    #[serde(flatten)]
    pub entry: TranscriptEntry,
}

/// Entry types in a transcript.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptEntry {
    /// Session metadata (first line of file)
    SessionMeta(SessionMetaEntry),
    /// User message
    User(UserEntry),
    /// Complete assistant turn
    Assistant(AssistantEntry),
    /// Tool execution start
    ToolCall(ToolCallEntry),
    /// Tool execution result
    ToolResult(ToolResultEntry),
}

/// Session metadata entry (first line of transcript).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionMetaEntry {
    pub session_id: String,
    pub project_id: String,
    pub started_at: String,
    pub cwd: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub cli_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
}

/// Git repository information.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GitInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
}

/// User message entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserEntry {
    /// Unique message ID
    pub id: String,
    /// The user's input text
    pub content: String,
}

/// Assistant message entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssistantEntry {
    /// Unique message ID
    pub id: String,
    /// Content blocks
    pub content: Vec<ContentBlock>,
    /// Model that generated this response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Content block types for assistant messages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Thinking { thinking: String },
}

/// Tool call entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallEntry {
    /// Unique call ID (for correlating with result)
    pub call_id: String,
    /// Tool name
    pub name: String,
    /// Tool input
    pub input: serde_json::Value,
}

/// Tool result entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResultEntry {
    /// Correlates with ToolCallEntry.call_id
    pub call_id: String,
    /// Tool output
    pub output: String,
    /// Whether output was truncated
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub truncated: bool,
    /// Exit code for shell commands
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}
