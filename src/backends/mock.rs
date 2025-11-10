use super::AgentBackend;
use async_trait::async_trait;
use color_eyre::Result;
use tokio::process::{Child, Command};

pub struct MockBackend;

#[async_trait]
impl AgentBackend for MockBackend {
    async fn spawn_process(&self, _prompt: String) -> Result<Child> {
        // Use printf with \n to output JSONL (newline-delimited JSON)
        // Using actual Claude CLI event format
        let child = Command::new("printf")
            .arg("{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Hello from mock\"}]}}\n{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"This is a test\"}]}}\n")
            .stdout(std::process::Stdio::piped())
            .spawn()?;

        Ok(child)
    }

    fn name(&self) -> &str {
        "Mock Backend"
    }
}
