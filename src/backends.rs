pub mod claude;
pub mod codex;
pub mod mock;

use async_trait::async_trait;
use color_eyre::Result;
use tokio::process::Child;

#[async_trait]
pub trait AgentBackend {
    async fn spawn_process(&self, prompt: String) -> Result<Child>;
    fn name(&self) -> &str;
}
