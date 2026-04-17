use super::*;

impl fmt::Display for SessionSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionSource::Cli => f.write_str("cli"),
            SessionSource::VSCode => f.write_str("vscode"),
            SessionSource::Exec => f.write_str("exec"),
            SessionSource::Mcp => f.write_str("mcp"),
            SessionSource::SubAgent(sub_source) => write!(f, "subagent_{sub_source}"),
            SessionSource::Unknown => f.write_str("unknown"),
        }
    }
}

impl fmt::Display for SubAgentSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubAgentSource::Compact => f.write_str("compact"),
            SubAgentSource::Other(other) => f.write_str(other),
        }
    }
}

impl Default for SessionMeta {
    fn default() -> Self {
        SessionMeta {
            id: ConversationId::default(),
            timestamp: String::new(),
            cwd: PathBuf::new(),
            originator: String::new(),
            cli_version: String::new(),
            instructions: None,
            source: SessionSource::default(),
            model_provider: None,
        }
    }
}

impl From<CompactedItem> for ResponseItem {
    fn from(value: CompactedItem) -> Self {
        ResponseItem::Message {
            id: None,
            role: "assistant".to_string(),
            content: vec![ContentItem::OutputText {
                text: value.message,
            }],
        }
    }
}

impl McpToolCallEndEvent {
    pub fn is_success(&self) -> bool {
        match &self.result {
            Ok(result) => !result.is_error.unwrap_or(false),
            Err(_) => false,
        }
    }
}

impl fmt::Display for McpAuthStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            McpAuthStatus::Unsupported => "Unsupported",
            McpAuthStatus::NotLoggedIn => "Not logged in",
            McpAuthStatus::BearerToken => "Bearer token",
            McpAuthStatus::OAuth => "OAuth",
        };
        f.write_str(text)
    }
}
