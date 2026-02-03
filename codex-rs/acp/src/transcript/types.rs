//! Schema types for Nori transcript persistence.
//!
//! Each line in a session transcript file is a self-contained entry.
//! The schema is designed for the client-side view of conversations.

use std::path::PathBuf;

use chrono::SecondsFormat;
use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

/// Get current time as ISO 8601 string (e.g., "2025-01-27T12:30:45.123Z").
pub fn now_iso8601() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

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
    /// Patch operation (file edit/write/delete)
    PatchApply(PatchApplyEntry),
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
    /// ACP agent used for the session (e.g., "claude-code", "codex", "gemini")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// CLI version
    pub cli_version: String,
    /// Git repository information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git: Option<GitInfo>,
    /// The ACP agent's session ID, used for resuming sessions via `session/load`.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub acp_session_id: Option<String>,
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
    /// Thinking/reasoning content (from extended thinking feature)
    Thinking { thinking: String },
}

/// Complete assistant message entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AssistantEntry {
    /// Unique message ID
    pub id: String,
    /// Content blocks (mirrors Anthropic API structure)
    pub content: Vec<ContentBlock>,
    /// ACP agent that generated this response (e.g., "claude-code", "codex", "gemini")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
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

/// Type of patch operation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatchOperationType {
    /// Edit an existing file
    Edit,
    /// Write/create a file
    Write,
    /// Delete a file
    Delete,
}

/// Patch operation entry (file edit/write/delete).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PatchApplyEntry {
    /// Unique call ID (for correlating with approval)
    pub call_id: String,
    /// Type of operation (edit, write, delete)
    pub operation: PatchOperationType,
    /// File path being modified
    pub path: PathBuf,
    /// Whether the operation succeeded
    pub success: bool,
    /// Error message if operation failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl TranscriptLine {
    /// Create a new transcript line with the current timestamp.
    pub fn new(entry: TranscriptEntry) -> Self {
        Self {
            ts: now_iso8601(),
            v: SCHEMA_VERSION,
            entry,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn session_meta_entry_serializes_agent_field() {
        let entry = SessionMetaEntry {
            session_id: "test-session".to_string(),
            project_id: "test-project".to_string(),
            started_at: "2025-01-27T12:00:00.000Z".to_string(),
            cwd: PathBuf::from("/tmp/test"),
            agent: Some("claude-code".to_string()),
            cli_version: "0.1.0".to_string(),
            git: None,
            acp_session_id: None,
        };

        let json = serde_json::to_string(&entry).unwrap();

        assert!(
            json.contains(r#""agent":"claude-code""#),
            "Expected 'agent' field in JSON, got: {json}"
        );
    }

    #[test]
    fn assistant_entry_serializes_agent_field() {
        let entry = AssistantEntry {
            id: "msg-001".to_string(),
            content: vec![ContentBlock::Text {
                text: "Hello".to_string(),
            }],
            agent: Some("claude-code".to_string()),
        };

        let json = serde_json::to_string(&entry).unwrap();

        assert!(
            json.contains(r#""agent":"claude-code""#),
            "Expected 'agent' field in JSON, got: {json}"
        );
    }

    #[test]
    fn session_meta_entry_deserializes_agent_field() {
        let json = r#"{
            "session_id": "test-session",
            "project_id": "test-project",
            "started_at": "2025-01-27T12:00:00.000Z",
            "cwd": "/tmp/test",
            "agent": "claude-code",
            "cli_version": "0.1.0"
        }"#;

        let entry: SessionMetaEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.agent, Some("claude-code".to_string()));
    }

    #[test]
    fn assistant_entry_deserializes_agent_field() {
        let json = r#"{
            "id": "msg-001",
            "content": [{"type": "text", "text": "Hello"}],
            "agent": "claude-code"
        }"#;

        let entry: AssistantEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.agent, Some("claude-code".to_string()));
    }

    #[test]
    fn session_meta_entry_serializes_acp_session_id() {
        let entry = SessionMetaEntry {
            session_id: "test-session".to_string(),
            project_id: "test-project".to_string(),
            started_at: "2025-01-27T12:00:00.000Z".to_string(),
            cwd: PathBuf::from("/tmp/test"),
            agent: Some("claude-code".to_string()),
            cli_version: "0.1.0".to_string(),
            git: None,
            acp_session_id: Some("acp-sess-abc123".to_string()),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(
            json.contains(r#""acp_session_id":"acp-sess-abc123""#),
            "Expected 'acp_session_id' field in JSON, got: {json}"
        );
    }

    #[test]
    fn session_meta_entry_deserializes_acp_session_id() {
        let json = r#"{
            "session_id": "test-session",
            "project_id": "test-project",
            "started_at": "2025-01-27T12:00:00.000Z",
            "cwd": "/tmp/test",
            "agent": "claude-code",
            "cli_version": "0.1.0",
            "acp_session_id": "acp-sess-abc123"
        }"#;

        let entry: SessionMetaEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.acp_session_id, Some("acp-sess-abc123".to_string()));
    }

    #[test]
    fn session_meta_entry_deserializes_without_acp_session_id() {
        // Backward compatibility: old transcripts without the field should still parse
        let json = r#"{
            "session_id": "test-session",
            "project_id": "test-project",
            "started_at": "2025-01-27T12:00:00.000Z",
            "cwd": "/tmp/test",
            "cli_version": "0.1.0"
        }"#;

        let entry: SessionMetaEntry = serde_json::from_str(json).unwrap();
        assert_eq!(entry.acp_session_id, None);
    }

    #[test]
    fn session_meta_entry_omits_acp_session_id_when_none() {
        let entry = SessionMetaEntry {
            session_id: "test-session".to_string(),
            project_id: "test-project".to_string(),
            started_at: "2025-01-27T12:00:00.000Z".to_string(),
            cwd: PathBuf::from("/tmp/test"),
            agent: None,
            cli_version: "0.1.0".to_string(),
            git: None,
            acp_session_id: None,
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(
            !json.contains("acp_session_id"),
            "Should not contain 'acp_session_id' when None, got: {json}"
        );
    }
}
