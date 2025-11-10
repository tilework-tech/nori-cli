use super::AgentBackend;
use async_trait::async_trait;
use color_eyre::Result;
use tokio::process::{Child, Command};

pub struct CodexBackend {
    pub thread_id: Option<String>,
}

impl CodexBackend {
    pub fn new() -> Self {
        Self { thread_id: None }
    }
}

#[async_trait]
impl AgentBackend for CodexBackend {
    async fn spawn_process(&self, prompt: String) -> Result<Child> {
        let mut cmd = Command::new("codex");

        // Use exec for headless mode
        cmd.arg("exec");

        // Add session resumption if we have a thread ID
        if let Some(ref thread_id) = self.thread_id {
            cmd.arg("resume").arg(thread_id);
        }

        // Always use JSON output
        cmd.arg("--json");

        // Add the prompt
        cmd.arg(prompt)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let child = cmd.spawn()?;
        Ok(child)
    }

    fn name(&self) -> &str {
        "GPT Codex"
    }
}
