use super::javascript_runtime::{JavaScriptRuntime, detect_javascript_runtime};
use super::{AgentBackend, BackendEvent};
use crate::acp_runner::{AcpAgentConfig, AcpAgentRunner};
use crate::conversation::ConversationEvent;
use async_stream::stream;
use futures::{Stream, StreamExt};
use std::path::PathBuf;
use std::pin::Pin;

pub struct GeminiAcpBackend {
    runtime: Option<JavaScriptRuntime>,
}

impl GeminiAcpBackend {
    pub fn new() -> Self {
        Self {
            runtime: detect_javascript_runtime(),
        }
    }
}

impl Default for GeminiAcpBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentBackend for GeminiAcpBackend {
    fn spawn_stream(
        &self,
        prompt: String,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> Pin<Box<dyn Stream<Item = BackendEvent> + Send>> {
        let runtime = self.runtime;

        let stream = stream! {
            // If no runtime available, emit error
            let Some(runtime) = runtime else {
                yield BackendEvent::Conversation(ConversationEvent::SystemEvent {
                    subtype: "error".to_string(),
                    details: Some(
                        "No JavaScript runtime found. Install Node.js (npm/npx) or Bun.".to_string()
                    ),
                });
                return;
            };

            // Configure ACP agent to run via bunx/npx
            let config = AcpAgentConfig {
                name: "Gemini ACP",
                command: runtime.command(),
                args: vec!["@google/gemini-cli".to_string(), "--experimental-acp".to_string()],
                install_url: "https://www.npmjs.com/package/@google/gemini-cli",
                install_command: Some(vec![
                    "npm".to_string(),
                    "install".to_string(),
                    "-g".to_string(),
                    "@google/gemini-cli".to_string(),
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
        "Gemini ACP"
    }

    fn command_name(&self) -> &str {
        self.runtime.map(|r| r.command()).unwrap_or("npx")
    }

    fn install_url(&self) -> &str {
        "https://www.npmjs.com/package/@google/gemini-cli"
    }

    fn install_command(&self) -> Option<Vec<String>> {
        Some(vec![
            "npm".to_string(),
            "install".to_string(),
            "-g".to_string(),
            "@google/gemini-cli".to_string(),
        ])
    }

    fn is_available(&self) -> bool {
        self.runtime.is_some()
    }
}
