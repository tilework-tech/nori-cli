//! Schema types for Nori transcript persistence.
//!
//! Each line in a session transcript file is a self-contained entry.
//! The schema is designed for the client-side view of conversations.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

/// Current schema version for forward compatibility.
pub const SCHEMA_VERSION: u8 = 1;

/// Wrapper for each line in the transcript JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptLine {
    /// ISO 8601 timestamp
    pub ts: String,
    /// Schema version for forward compatibility
    pub v: u8,
    /// The entry payload
    #[serde(flatten)]
    pub entry: TranscriptEntry,
}

/// Entry types that can appear in a transcript.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TranscriptEntry {
    /// Session metadata (first line of file)
    SessionMeta(SessionMetaEntry),
    /// User message
    User(UserEntry),
    /// Complete assistant turn
    Assistant(AssistantEntry),
    /// Tool execution (stored like core rollout for consistency)
    ToolCall(ToolCallEntry),
    /// Tool result
    ToolResult(ToolResultEntry),
}

/// Git repository information captured at session start.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GitInfo {
    /// Current branch name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Current commit hash
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
}

/// Session metadata entry (first line of transcript file).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionMetaEntry {
    /// Unique session identifier (UUID)
    pub session_id: String,
    /// Project identifier (hash-based)
    pub project_id: String,
    /// ISO 8601 timestamp when session started
    pub started_at: String,
    /// Working directory for the session
    pub cwd: PathBuf,
    /// Model used for the session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// CLI version
    pub cli_version: String,
    /// Git repository information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
}

/// Attachment type for user messages (images, files, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Attachment {
    /// File path reference
    FilePath { path: PathBuf },
    /// Base64 encoded data
    Base64 { data: String, mime_type: String },
}

/// User message entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserEntry {
    /// Unique message ID
    pub id: String,
    /// The user's input text
    pub content: String,
    /// Optional: images or other attachments
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub attachments: Vec<Attachment>,
}

/// Content block in an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text content
    Text { text: String },
    /// Thinking content (extended thinking)
    Thinking { thinking: String },
}

/// Complete assistant message entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssistantEntry {
    /// Unique message ID
    pub id: String,
    /// Content blocks (mirrors Anthropic API structure)
    pub content: Vec<ContentBlock>,
    /// Model that generated this response
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Tool call entry (when tool execution begins).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallEntry {
    /// Unique call ID (for correlating with result)
    pub call_id: String,
    /// Tool name (e.g., "shell", "read", "edit")
    pub name: String,
    /// Tool input (JSON-serialized arguments)
    pub input: serde_json::Value,
}

/// Tool result entry (when tool execution completes).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResultEntry {
    /// Correlates with ToolCallEntry.call_id
    pub call_id: String,
    /// Tool output (may be truncated for large outputs)
    pub output: String,
    /// Whether output was truncated
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub truncated: bool,
    /// Exit code for shell commands
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

impl TranscriptLine {
    /// Create a new transcript line with the current timestamp.
    pub fn new(entry: TranscriptEntry) -> Self {
        Self {
            ts: Self::now_iso8601(),
            v: SCHEMA_VERSION,
            entry,
        }
    }

    /// Get current time as ISO 8601 string.
    fn now_iso8601() -> String {
        use std::time::SystemTime;
        use std::time::UNIX_EPOCH;

        let duration = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = duration.as_secs();
        let millis = duration.subsec_millis();

        // Format as ISO 8601: YYYY-MM-DDTHH:MM:SS.mmmZ
        // Using a simple calculation (not accounting for leap seconds, etc.)
        let days = secs / 86400;
        let time_secs = secs % 86400;
        let hours = time_secs / 3600;
        let mins = (time_secs % 3600) / 60;
        let secs_in_min = time_secs % 60;

        // Calculate year, month, day from days since epoch
        // This is a simplified calculation
        let mut year = 1970;
        let mut remaining_days = days as i64;

        loop {
            let days_in_year = if is_leap_year(year) { 366 } else { 365 };
            if remaining_days < days_in_year {
                break;
            }
            remaining_days -= days_in_year;
            year += 1;
        }

        let days_in_months: &[i64] = if is_leap_year(year) {
            &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };

        let mut month = 1;
        for &days_in_month in days_in_months {
            if remaining_days < days_in_month {
                break;
            }
            remaining_days -= days_in_month;
            month += 1;
        }
        let day = remaining_days + 1;

        format!("{year:04}-{month:02}-{day:02}T{hours:02}:{mins:02}:{secs_in_min:02}.{millis:03}Z")
    }
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}
