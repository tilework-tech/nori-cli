use super::{AgentBackend, BackendEvent};
use crate::acp_runner::{AcpAgentConfig, AcpAgentRunner};
use crate::conversation::ConversationEvent;
use async_stream::stream;
use futures::{Stream, StreamExt};
use std::path::PathBuf;
use std::pin::Pin;

#[cfg(windows)]
const MOCK_ACP_AGENT_COMMAND: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "\\target\\debug\\mock_acp_agent.exe"
);
#[cfg(not(windows))]
const MOCK_ACP_AGENT_COMMAND: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/target/debug/mock_acp_agent");

fn mock_agent_config() -> AcpAgentConfig {
    AcpAgentConfig {
        name: "Mock ACP Agent",
        command: MOCK_ACP_AGENT_COMMAND,
        args: Vec::new(),
        install_url: "https://github.com/anthropics/nori-cli",
        install_command: Some(vec![
            "cargo".to_string(),
            "build".to_string(),
            "--manifest-path".to_string(),
            "tests/mock_acp_agent/Cargo.toml".to_string(),
        ]),
    }
}

pub struct MockBackend;

impl MockBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentBackend for MockBackend {
    fn spawn_stream(
        &self,
        prompt: String,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = BackendEvent> + Send>> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let config = mock_agent_config();

        let stream = stream! {
            let mut runner = AcpAgentRunner::new(config, cwd);
            match runner.spawn_stream(prompt, cancel_token).await {
                Ok(mut inner_stream) => {
                    while let Some(event) = inner_stream.next().await {
                        yield event;
                    }
                }
                Err(err) => {
                    yield BackendEvent::Conversation(ConversationEvent::SystemEvent {
                        subtype: "acp_error".to_string(),
                        details: Some(err),
                    });
                }
            }
        };

        Box::pin(stream)
    }

    fn name(&self) -> &str {
        "Mock ACP Agent"
    }

    fn command_name(&self) -> &str {
        MOCK_ACP_AGENT_COMMAND
    }

    fn install_url(&self) -> &str {
        "https://github.com/anthropics/nori-cli"
    }

    fn install_command(&self) -> Option<Vec<String>> {
        Some(vec![
            "cargo".to_string(),
            "build".to_string(),
            "--manifest-path".to_string(),
            "tests/mock_acp_agent/Cargo.toml".to_string(),
        ])
    }

    fn is_available(&self) -> bool {
        super::is_available(MOCK_ACP_AGENT_COMMAND)
    }
}
