use super::AgentBackend;
use crate::conversation::ConversationEvent;
use async_stream::stream;
use futures::stream::Stream;
use serde_json::Value;
use std::pin::Pin;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub struct ClaudeBackend {
    pub session_id: Option<String>,
}

impl Default for ClaudeBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeBackend {
    pub fn new() -> Self {
        Self { session_id: None }
    }

    fn parse_jsonl_event(line: &str) -> Option<ConversationEvent> {
        let json: Value = serde_json::from_str(line).ok()?;
        let event_type = json.get("type")?.as_str()?;

        match event_type {
            "assistant" => {
                let message = json.get("message")?;
                let content_array = message.get("content")?.as_array()?;

                let mut text = String::new();
                for item in content_array {
                    if let Some("text") = item.get("type").and_then(|v| v.as_str())
                        && let Some(t) = item.get("text").and_then(|v| v.as_str())
                    {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                }

                Some(ConversationEvent::AssistantMessage { text })
            }
            "system" => {
                let subtype = json
                    .get("subtype")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let details = json
                    .get("cwd")
                    .or_else(|| json.get("session_id"))
                    .and_then(|v| v.as_str())
                    .map(String::from);

                Some(ConversationEvent::SystemEvent { subtype, details })
            }
            "result" => {
                let subtype = json.get("subtype").and_then(|v| v.as_str());
                let success = subtype == Some("success");

                let details = json
                    .get("result")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Completed")
                    .to_string();

                Some(ConversationEvent::ResultSummary { success, details })
            }
            _ => Some(ConversationEvent::UnknownEvent {
                raw: line.to_string(),
            }),
        }
    }
}

impl AgentBackend for ClaudeBackend {
    fn spawn_stream(
        &self,
        prompt: String,
        _cancel_token: tokio_util::sync::CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = ConversationEvent> + Send>> {
        let session_id = self.session_id.clone();

        let stream = stream! {
            // Spawn Claude process
            let mut cmd = Command::new("claude");
            cmd.arg("--print")
                .arg("--output-format")
                .arg("stream-json")
                .arg("--include-partial-messages")
                .arg("--verbose");

            if let Some(ref sid) = session_id {
                cmd.arg("--resume").arg(sid);
            }

            cmd.arg(prompt)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    yield ConversationEvent::SystemEvent {
                        subtype: "error".to_string(),
                        details: Some("Claude CLI is not installed. Install from https://code.claude.com".to_string()),
                    };
                    return;
                }
                Err(e) => {
                    yield ConversationEvent::UnknownEvent {
                        raw: format!("Failed to spawn claude: {}", e),
                    };
                    return;
                }
            };

            // Stream stdout events
            if let Some(stdout) = child.stdout.take() {
                let mut reader = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    if let Some(event) = Self::parse_jsonl_event(&line) {
                        // Close stream immediately when result is received
                        let is_result = matches!(event, ConversationEvent::ResultSummary { .. });
                        yield event;
                        if is_result {
                            return;
                        }
                    }
                }
            }

            // Stream stderr events
            if let Some(stderr) = child.stderr.take() {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    yield ConversationEvent::StderrOutput { line };
                }
            }

            // Wait for process to complete and yield result
            match child.wait().await {
                Ok(status) if status.success() => {
                    yield ConversationEvent::ResultSummary {
                        success: true,
                        details: "Completed".to_string(),
                    };
                }
                Ok(status) => {
                    yield ConversationEvent::ResultSummary {
                        success: false,
                        details: format!("Process exited with status: {}", status),
                    };
                }
                Err(e) => {
                    yield ConversationEvent::UnknownEvent {
                        raw: format!("Failed to wait for process: {}", e),
                    };
                }
            }
        };

        Box::pin(stream)
    }

    fn name(&self) -> &str {
        "Claude Code"
    }

    fn command_name(&self) -> &str {
        "claude"
    }

    fn install_url(&self) -> &str {
        "https://code.claude.com"
    }

    fn install_command(&self) -> Option<Vec<String>> {
        None // Claude is installed via download, not command
    }
}
