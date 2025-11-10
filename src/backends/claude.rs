use super::AgentBackend;
use async_trait::async_trait;
use color_eyre::Result;
use tokio::process::{Child, Command};

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
}

#[async_trait]
impl AgentBackend for ClaudeBackend {
    async fn spawn_process(&self, prompt: String) -> Result<Child> {
        let mut cmd = Command::new("claude");
        cmd.arg("--print")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--include-partial-messages");

        // Add session resumption if we have a session ID
        if let Some(ref session_id) = self.session_id {
            cmd.arg("--resume").arg(session_id);
        }

        cmd.arg(prompt)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = cmd.spawn()?;
        Ok(child)
    }

    fn name(&self) -> &str {
        "Claude Code"
    }
}
