use super::AgentBackend;
use super::javascript_runtime::{JavaScriptRuntime, detect_javascript_runtime};
use crate::acp_runner::{AcpAgentConfig, AcpAgentRunner};
use crate::conversation::ConversationEvent;
use async_stream::stream;
use futures::{Stream, StreamExt};
use std::path::PathBuf;
use std::pin::Pin;

pub struct CodexAcpBackend {
    runtime: Option<JavaScriptRuntime>,
}

impl CodexAcpBackend {
    pub fn new() -> Self {
        Self {
            runtime: detect_javascript_runtime(),
        }
    }
}

impl Default for CodexAcpBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentBackend for CodexAcpBackend {
    fn spawn_stream(
        &self,
        prompt: String,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = ConversationEvent> + Send>> {
        let runtime = self.runtime;

        let stream = stream! {
            // If no runtime available, emit error
            let Some(runtime) = runtime else {
                yield ConversationEvent::SystemEvent {
                    subtype: "error".to_string(),
                    details: Some(
                        "No JavaScript runtime found. Install Node.js (npm/npx) or Bun.".to_string()
                    ),
                };
                return;
            };

            // Configure ACP agent to run via bunx/npx
            let config = AcpAgentConfig {
                name: "Codex ACP",
                command: runtime.command(),
                args: vec!["@zed-industries/codex-acp".to_string()],
                install_url: "https://www.npmjs.com/package/@zed-industries/codex-acp",
                install_command: Some(vec![
                    "npm".to_string(),
                    "install".to_string(),
                    "-g".to_string(),
                    "@zed-industries/codex-acp".to_string(),
                ]),
            };

            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let mut runner = AcpAgentRunner::new(config, cwd);

            match runner.spawn_stream(prompt, cancel_token).await {
                Ok(mut inner_stream) => {
                    while let Some(event) = inner_stream.next().await {
                        yield event;
                    }
                }
                Err(err) => {
                    yield ConversationEvent::SystemEvent {
                        subtype: "acp_error".to_string(),
                        details: Some(err),
                    };
                }
            }
        };

        Box::pin(stream)
    }

    fn name(&self) -> &str {
        "Codex ACP"
    }

    fn command_name(&self) -> &str {
        self.runtime.map(|r| r.command()).unwrap_or("npx")
    }

    fn install_url(&self) -> &str {
        "https://www.npmjs.com/package/@zed-industries/codex-acp"
    }

    fn install_command(&self) -> Option<Vec<String>> {
        Some(vec![
            "npm".to_string(),
            "install".to_string(),
            "-g".to_string(),
            "@zed-industries/codex-acp".to_string(),
        ])
    }
}
