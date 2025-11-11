use super::AgentBackend;
use crate::conversation::ConversationEvent;
use async_stream::stream;
use futures::stream::Stream;
use std::pin::Pin;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub struct CodexBackend {
    pub thread_id: Option<String>,
}

impl Default for CodexBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexBackend {
    pub fn new() -> Self {
        Self { thread_id: None }
    }
}

impl AgentBackend for CodexBackend {
    fn spawn_stream(
        &self,
        prompt: String,
    ) -> Pin<Box<dyn Stream<Item = ConversationEvent> + Send>> {
        let thread_id = self.thread_id.clone();

        let stream = stream! {
            // Spawn Codex process
            let mut cmd = Command::new("codex");
            cmd.arg("exec");

            if let Some(ref tid) = thread_id {
                cmd.arg("resume").arg(tid);
            }

            cmd.arg("--json");
            cmd.arg(prompt)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    yield ConversationEvent::SystemEvent {
                        subtype: "error".to_string(),
                        details: Some("Codex CLI is not installed. Install from https://developers.openai.com/codex/cli/".to_string()),
                    };
                    return;
                }
                Err(e) => {
                    yield ConversationEvent::UnknownEvent {
                        raw: format!("Failed to spawn codex: {}", e),
                    };
                    return;
                }
            };

            // Stream stdout - TODO: Codex might have different JSON format
            // For now, just stream raw lines as unknown events
            if let Some(stdout) = child.stdout.take() {
                let mut reader = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    // Codex-specific parsing would go here
                    yield ConversationEvent::UnknownEvent { raw: line };
                }
            }

            // Stream stderr events
            if let Some(stderr) = child.stderr.take() {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    yield ConversationEvent::StderrOutput { line };
                }
            }

            // Wait for process to complete
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
        "GPT Codex"
    }

    fn command_name(&self) -> &str {
        "codex"
    }

    fn install_url(&self) -> &str {
        "" // Codex is installed via npm, not URL
    }

    fn install_command(&self) -> Option<Vec<String>> {
        Some(vec![
            "npm".to_string(),
            "install".to_string(),
            "-g".to_string(),
            "@openai/codex".to_string(),
        ])
    }
}
