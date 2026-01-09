//! Session storage for ACP agents
//!
//! This module provides persistent storage for ACP sessions, enabling:
//! - Session resume after restart
//! - History browsing in the TUI
//! - Session forking capabilities
//!
//! Sessions are stored in JSONL format at:
//! `~/.nori/cli/sessions/acp/<agent>/<year>/<month>/<day>/<session_id>.jsonl`

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use agent_client_protocol as acp;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::registry::AgentKind;

/// Storage location for ACP sessions relative to nori home
pub const ACP_SESSIONS_DIR: &str = "sessions/acp";

/// Metadata for an ACP session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpSessionMeta {
    /// Unique session identifier
    pub session_id: String,
    /// The kind of agent used for this session
    pub agent_kind: AgentKind,
    /// Model name/ID used in this session
    pub model_name: String,
    /// Working directory for the session
    pub cwd: PathBuf,
    /// Git branch if available
    pub git_branch: Option<String>,
    /// When the session was created (Unix timestamp)
    pub created_at: i64,
    /// When the session was last updated (Unix timestamp)
    pub updated_at: i64,
    /// First user message (for preview in resume picker)
    pub preview: Option<String>,
}

impl AcpSessionMeta {
    /// Create new session metadata
    pub fn new(
        session_id: String,
        agent_kind: AgentKind,
        model_name: String,
        cwd: PathBuf,
    ) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        Self {
            session_id,
            agent_kind,
            model_name,
            cwd,
            git_branch: None,
            created_at: now,
            updated_at: now,
            preview: None,
        }
    }

    /// Set the git branch
    pub fn with_git_branch(mut self, branch: Option<String>) -> Self {
        self.git_branch = branch;
        self
    }

    /// Set the preview text
    pub fn with_preview(mut self, preview: Option<String>) -> Self {
        self.preview = preview;
        self
    }
}

/// Role of a turn in the session
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnRole {
    /// User input
    User,
    /// Agent response
    Agent,
}

/// Record of a tool call within a turn
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    /// Unique identifier for this tool call
    pub call_id: String,
    /// Tool name/title
    pub title: String,
    /// Tool kind if available
    pub kind: Option<String>,
    /// Raw input to the tool
    pub raw_input: Option<serde_json::Value>,
    /// Raw output from the tool
    pub raw_output: Option<serde_json::Value>,
    /// Status of the tool call
    pub status: String,
}

/// A single turn in the session history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpSessionTurn {
    /// When this turn occurred (Unix timestamp)
    pub timestamp: i64,
    /// Role (user or agent)
    pub role: TurnRole,
    /// Content blocks in this turn
    pub content: Vec<SerializableContentBlock>,
    /// Tool calls made during this turn (agent turns only)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallRecord>,
}

impl AcpSessionTurn {
    /// Create a new turn
    pub fn new(role: TurnRole, content: Vec<SerializableContentBlock>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        Self {
            timestamp: now,
            role,
            content,
            tool_calls: Vec::new(),
        }
    }

    /// Add tool calls to this turn
    pub fn with_tool_calls(mut self, tool_calls: Vec<ToolCallRecord>) -> Self {
        self.tool_calls = tool_calls;
        self
    }
}

/// Serializable version of ACP ContentBlock
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SerializableContentBlock {
    /// Text content
    Text { text: String },
    /// Image content (base64 encoded)
    Image { data: String, media_type: String },
    /// Other content (stored as JSON)
    Other { data: serde_json::Value },
}

impl From<&acp::ContentBlock> for SerializableContentBlock {
    fn from(block: &acp::ContentBlock) -> Self {
        match block {
            acp::ContentBlock::Text(text) => SerializableContentBlock::Text {
                text: text.text.clone(),
            },
            acp::ContentBlock::Image(image) => SerializableContentBlock::Image {
                data: image.data.clone(),
                media_type: image.media_type.clone(),
            },
        }
    }
}

impl From<SerializableContentBlock> for acp::ContentBlock {
    fn from(block: SerializableContentBlock) -> Self {
        match block {
            SerializableContentBlock::Text { text } => {
                acp::ContentBlock::Text(acp::TextContent::new(text))
            }
            SerializableContentBlock::Image { data, media_type } => {
                acp::ContentBlock::Image(acp::ImageContent::new(data, media_type))
            }
            SerializableContentBlock::Other { .. } => {
                // Fallback to empty text for unsupported content
                acp::ContentBlock::Text(acp::TextContent::new("[unsupported content]"))
            }
        }
    }
}

/// Line types in the session file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SessionLine {
    /// Session metadata (first line)
    Meta(AcpSessionMeta),
    /// A turn in the conversation
    Turn(AcpSessionTurn),
}

/// Storage manager for ACP sessions
pub struct AcpSessionStorage {
    /// Base directory for session storage (~/.nori/cli)
    nori_home: PathBuf,
}

impl AcpSessionStorage {
    /// Create a new session storage manager
    pub fn new(nori_home: &Path) -> Self {
        Self {
            nori_home: nori_home.to_path_buf(),
        }
    }

    /// Get the base directory for ACP sessions
    fn sessions_dir(&self) -> PathBuf {
        self.nori_home.join(ACP_SESSIONS_DIR)
    }

    /// Get the directory for a specific agent
    fn agent_sessions_dir(&self, agent_kind: &AgentKind) -> PathBuf {
        self.sessions_dir().join(agent_kind.name())
    }

    /// Get the path for a session file
    fn session_path(&self, session_id: &str, agent_kind: &AgentKind, timestamp: i64) -> PathBuf {
        // Convert timestamp to date components
        let secs = if timestamp > 0 {
            timestamp as u64
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        };

        // Calculate year/month/day from unix timestamp
        // This is a simplified calculation - for production we'd use chrono
        let days_since_epoch = secs / 86400;
        let years_since_epoch = days_since_epoch / 365;
        let year = 1970 + years_since_epoch;
        let day_of_year = days_since_epoch % 365;
        let month = (day_of_year / 30) + 1;
        let day = (day_of_year % 30) + 1;

        self.agent_sessions_dir(agent_kind)
            .join(format!("{year}"))
            .join(format!("{month:02}"))
            .join(format!("{day:02}"))
            .join(format!("{session_id}.jsonl"))
    }

    /// List all ACP sessions, optionally filtered by working directory
    pub async fn list_sessions(
        &self,
        filter_cwd: Option<&Path>,
        limit: usize,
    ) -> Result<Vec<AcpSessionMeta>> {
        let sessions_dir = self.sessions_dir();
        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        let mut session_paths = Vec::new();

        // Collect all session files
        for agent_entry in std::fs::read_dir(&sessions_dir)? {
            let agent_entry = agent_entry?;
            if !agent_entry.file_type()?.is_dir() {
                continue;
            }

            // Walk the year/month/day structure
            Self::collect_session_files(&agent_entry.path(), &mut session_paths)?;
        }

        // Sort by modification time (newest first)
        session_paths.sort_by(|a, b| {
            let a_time = std::fs::metadata(a)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            let b_time = std::fs::metadata(b)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            b_time.cmp(&a_time)
        });

        // Load metadata from each session file
        for path in session_paths.into_iter().take(limit * 2) {
            // Load extra to handle filtering
            if let Ok(Some(meta)) = self.load_session_meta(&path) {
                // Apply cwd filter if specified
                if let Some(filter) = filter_cwd {
                    if meta.cwd != filter {
                        continue;
                    }
                }

                sessions.push(meta);
                if sessions.len() >= limit {
                    break;
                }
            }
        }

        Ok(sessions)
    }

    /// Recursively collect session files from directory structure
    fn collect_session_files(dir: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
        if !dir.exists() || !dir.is_dir() {
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                Self::collect_session_files(&path, paths)?;
            } else if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                paths.push(path);
            }
        }

        Ok(())
    }

    /// Load session metadata from a file
    fn load_session_meta(&self, path: &Path) -> Result<Option<AcpSessionMeta>> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);

        // Read just the first line to get metadata
        if let Some(line) = reader.lines().next() {
            let line = line?;
            if let Ok(SessionLine::Meta(meta)) = serde_json::from_str(&line) {
                return Ok(Some(meta));
            }
        }

        Ok(None)
    }

    /// Load session history for resume
    ///
    /// Returns the content blocks in ACP format for sending to the agent
    pub async fn load_session_history(
        &self,
        session_id: &str,
    ) -> Result<(AcpSessionMeta, Vec<acp::ContentBlock>)> {
        let (meta, turns) = self.load_session_turns(session_id).await?;

        // Convert turns to content blocks for the agent
        let mut history: Vec<acp::ContentBlock> = Vec::new();

        for turn in turns {
            for block in turn.content {
                history.push(block.into());
            }
        }

        Ok((meta, history))
    }

    /// Load all turns from a session
    pub async fn load_session_turns(
        &self,
        session_id: &str,
    ) -> Result<(AcpSessionMeta, Vec<AcpSessionTurn>)> {
        // Find the session file
        let path = self.find_session_file(session_id).await?;
        let file =
            std::fs::File::open(&path).with_context(|| format!("Failed to open {path:?}"))?;
        let reader = std::io::BufReader::new(file);

        let mut meta: Option<AcpSessionMeta> = None;
        let mut turns: Vec<AcpSessionTurn> = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<SessionLine>(&line) {
                Ok(SessionLine::Meta(m)) => {
                    meta = Some(m);
                }
                Ok(SessionLine::Turn(turn)) => {
                    turns.push(turn);
                }
                Err(e) => {
                    warn!("Failed to parse session line: {}", e);
                }
            }
        }

        let meta = meta.ok_or_else(|| anyhow::anyhow!("Session file missing metadata"))?;
        Ok((meta, turns))
    }

    /// Find a session file by session ID
    async fn find_session_file(&self, session_id: &str) -> Result<PathBuf> {
        let sessions_dir = self.sessions_dir();
        let mut paths = Vec::new();
        Self::collect_session_files(&sessions_dir, &mut paths)?;

        for path in paths {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem == session_id {
                    return Ok(path);
                }
            }
        }

        Err(anyhow::anyhow!("Session not found: {session_id}"))
    }

    /// Create a new session file and write initial metadata
    pub fn create_session(&self, meta: &AcpSessionMeta) -> Result<PathBuf> {
        let path = self.session_path(&meta.session_id, &meta.agent_kind, meta.created_at);

        // Create parent directories
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {parent:?}"))?;
        }

        // Write metadata as first line
        let mut file = std::fs::File::create(&path)
            .with_context(|| format!("Failed to create session file: {path:?}"))?;

        let line = SessionLine::Meta(meta.clone());
        serde_json::to_writer(&mut file, &line)?;
        writeln!(file)?;

        debug!("Created session file: {:?}", path);
        Ok(path)
    }

    /// Append a turn to the session file
    pub async fn append_turn(&self, session_id: &str, turn: &AcpSessionTurn) -> Result<()> {
        let path = self.find_session_file(session_id).await?;

        // Append to file
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .with_context(|| format!("Failed to open session file: {path:?}"))?;

        let line = SessionLine::Turn(turn.clone());
        serde_json::to_writer(&mut file, &line)?;
        writeln!(file)?;

        // Update metadata's updated_at timestamp
        self.update_session_timestamp(&path)?;

        debug!("Appended turn to session: {:?}", path);
        Ok(())
    }

    /// Update the session's updated_at timestamp
    fn update_session_timestamp(&self, path: &Path) -> Result<()> {
        // Read all lines
        let content = std::fs::read_to_string(path)?;
        let mut lines: Vec<String> = content.lines().map(String::from).collect();

        if lines.is_empty() {
            return Ok(());
        }

        // Parse and update metadata
        if let Ok(SessionLine::Meta(mut meta)) = serde_json::from_str(&lines[0]) {
            meta.updated_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            lines[0] = serde_json::to_string(&SessionLine::Meta(meta))?;

            // Write back
            std::fs::write(path, lines.join("\n") + "\n")?;
        }

        Ok(())
    }

    /// Get session metadata by ID
    pub async fn get_session_meta(&self, session_id: &str) -> Result<Option<AcpSessionMeta>> {
        match self.find_session_file(session_id).await {
            Ok(path) => self.load_session_meta(&path),
            Err(_) => Ok(None),
        }
    }

    /// Delete a session
    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let path = self.find_session_file(session_id).await?;
        std::fs::remove_file(&path)?;
        debug!("Deleted session: {:?}", path);
        Ok(())
    }
}

/// Convert ACP SessionUpdates from a prompt into an AcpSessionTurn
pub fn session_updates_to_turn(
    updates: &[acp::SessionUpdate],
    role: TurnRole,
) -> Option<AcpSessionTurn> {
    let mut content: Vec<SerializableContentBlock> = Vec::new();
    let mut tool_calls: Vec<ToolCallRecord> = Vec::new();
    let mut tool_call_map: HashMap<String, ToolCallRecord> = HashMap::new();

    for update in updates {
        match update {
            acp::SessionUpdate::AgentMessageChunk(chunk) => {
                content.push(SerializableContentBlock::from(&chunk.content));
            }
            acp::SessionUpdate::UserMessageChunk(chunk) => {
                content.push(SerializableContentBlock::from(&chunk.content));
            }
            acp::SessionUpdate::ToolCall(call) => {
                let record = ToolCallRecord {
                    call_id: call.tool_call_id.to_string(),
                    title: call.title.clone(),
                    kind: call.kind.as_ref().map(|k| format!("{k:?}")),
                    raw_input: call.raw_input.clone(),
                    raw_output: None,
                    status: call
                        .status
                        .as_ref()
                        .map(|s| format!("{s:?}"))
                        .unwrap_or_else(|| "unknown".to_string()),
                };
                tool_call_map.insert(call.tool_call_id.to_string(), record);
            }
            acp::SessionUpdate::ToolCallUpdate(update) => {
                let call_id = update.tool_call_id.to_string();
                if let Some(record) = tool_call_map.get_mut(&call_id) {
                    if let Some(status) = &update.fields.status {
                        record.status = format!("{status:?}");
                    }
                    if let Some(output) = &update.fields.raw_output {
                        record.raw_output = Some(output.clone());
                    }
                }
            }
            _ => {}
        }
    }

    // Collect tool calls from map
    tool_calls.extend(tool_call_map.into_values());

    if content.is_empty() && tool_calls.is_empty() {
        return None;
    }

    Some(AcpSessionTurn::new(role, content).with_tool_calls(tool_calls))
}

/// Create a user turn from input text
pub fn user_input_to_turn(text: &str) -> AcpSessionTurn {
    AcpSessionTurn::new(
        TurnRole::User,
        vec![SerializableContentBlock::Text {
            text: text.to_string(),
        }],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_create_and_load_session() {
        let temp_dir = tempdir().unwrap();
        let storage = AcpSessionStorage::new(temp_dir.path());

        // Create a session
        let meta = AcpSessionMeta::new(
            "test-session-123".to_string(),
            AgentKind::ClaudeCode,
            "claude-code".to_string(),
            PathBuf::from("/home/user/project"),
        )
        .with_git_branch(Some("main".to_string()))
        .with_preview(Some("Hello world".to_string()));

        let path = storage.create_session(&meta).unwrap();
        assert!(path.exists());

        // Load metadata
        let loaded = storage.get_session_meta("test-session-123").await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.session_id, "test-session-123");
        assert_eq!(loaded.agent_kind, AgentKind::ClaudeCode);
        assert_eq!(loaded.git_branch, Some("main".to_string()));
    }

    #[tokio::test]
    async fn test_append_and_load_turns() {
        let temp_dir = tempdir().unwrap();
        let storage = AcpSessionStorage::new(temp_dir.path());

        // Create a session
        let meta = AcpSessionMeta::new(
            "test-turns-456".to_string(),
            AgentKind::Gemini,
            "gemini-2.5-flash".to_string(),
            PathBuf::from("/home/user/project"),
        );

        storage.create_session(&meta).unwrap();

        // Append a user turn
        let user_turn = user_input_to_turn("Hello, how are you?");
        storage.append_turn("test-turns-456", &user_turn).await.unwrap();

        // Append an agent turn
        let agent_turn = AcpSessionTurn::new(
            TurnRole::Agent,
            vec![SerializableContentBlock::Text {
                text: "I'm doing well, thank you!".to_string(),
            }],
        );
        storage.append_turn("test-turns-456", &agent_turn).await.unwrap();

        // Load turns
        let (loaded_meta, turns) = storage.load_session_turns("test-turns-456").await.unwrap();
        assert_eq!(loaded_meta.session_id, "test-turns-456");
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].role, TurnRole::User);
        assert_eq!(turns[1].role, TurnRole::Agent);
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let temp_dir = tempdir().unwrap();
        let storage = AcpSessionStorage::new(temp_dir.path());

        // Create multiple sessions
        for i in 1..=5 {
            let meta = AcpSessionMeta::new(
                format!("session-{i}"),
                AgentKind::ClaudeCode,
                "claude-code".to_string(),
                PathBuf::from("/home/user/project"),
            );
            storage.create_session(&meta).unwrap();
        }

        // List sessions
        let sessions = storage.list_sessions(None, 10).await.unwrap();
        assert_eq!(sessions.len(), 5);
    }

    #[tokio::test]
    async fn test_list_sessions_with_cwd_filter() {
        let temp_dir = tempdir().unwrap();
        let storage = AcpSessionStorage::new(temp_dir.path());

        // Create sessions in different directories
        let meta1 = AcpSessionMeta::new(
            "session-a".to_string(),
            AgentKind::ClaudeCode,
            "claude-code".to_string(),
            PathBuf::from("/home/user/project-a"),
        );
        storage.create_session(&meta1).unwrap();

        let meta2 = AcpSessionMeta::new(
            "session-b".to_string(),
            AgentKind::ClaudeCode,
            "claude-code".to_string(),
            PathBuf::from("/home/user/project-b"),
        );
        storage.create_session(&meta2).unwrap();

        // List with filter
        let filter = PathBuf::from("/home/user/project-a");
        let sessions = storage.list_sessions(Some(&filter), 10).await.unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "session-a");
    }

    #[tokio::test]
    async fn test_load_session_history() {
        let temp_dir = tempdir().unwrap();
        let storage = AcpSessionStorage::new(temp_dir.path());

        // Create a session with turns
        let meta = AcpSessionMeta::new(
            "history-test".to_string(),
            AgentKind::ClaudeCode,
            "claude-code".to_string(),
            PathBuf::from("/home/user/project"),
        );
        storage.create_session(&meta).unwrap();

        storage
            .append_turn("history-test", &user_input_to_turn("What is 2+2?"))
            .await
            .unwrap();

        let agent_turn = AcpSessionTurn::new(
            TurnRole::Agent,
            vec![SerializableContentBlock::Text {
                text: "2+2 equals 4".to_string(),
            }],
        );
        storage.append_turn("history-test", &agent_turn).await.unwrap();

        // Load history
        let (_, history) = storage.load_session_history("history-test").await.unwrap();
        assert_eq!(history.len(), 2);

        // Verify content
        if let acp::ContentBlock::Text(text) = &history[0] {
            assert_eq!(text.text, "What is 2+2?");
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_delete_session() {
        let temp_dir = tempdir().unwrap();
        let storage = AcpSessionStorage::new(temp_dir.path());

        // Create a session
        let meta = AcpSessionMeta::new(
            "to-delete".to_string(),
            AgentKind::ClaudeCode,
            "claude-code".to_string(),
            PathBuf::from("/home/user/project"),
        );
        let path = storage.create_session(&meta).unwrap();
        assert!(path.exists());

        // Delete it
        storage.delete_session("to-delete").await.unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn test_serializable_content_block_roundtrip() {
        let original = acp::ContentBlock::Text(acp::TextContent::new("Hello world"));
        let serializable = SerializableContentBlock::from(&original);
        let restored: acp::ContentBlock = serializable.into();

        if let acp::ContentBlock::Text(text) = restored {
            assert_eq!(text.text, "Hello world");
        } else {
            panic!("Expected text content");
        }
    }

    #[test]
    fn test_user_input_to_turn() {
        let turn = user_input_to_turn("Hello");
        assert_eq!(turn.role, TurnRole::User);
        assert_eq!(turn.content.len(), 1);
        assert!(turn.tool_calls.is_empty());
    }
}
