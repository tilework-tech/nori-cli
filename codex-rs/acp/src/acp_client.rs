//! ACP Model Client implementation
//!
//! Provides AcpModelClient for communicating with ACP-compliant agent subprocesses.

use crate::AgentProcess;
use crate::client_handler::ClientEvent;
use anyhow::{Context, Result};
use futures::Stream;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio::sync::mpsc;
use tracing::{debug, error};

/// Events emitted by AcpModelClient during streaming
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// Text delta from agent message
    TextDelta(String),
    /// Reasoning/thought delta
    ReasoningDelta(String),
    /// Stream completed
    Completed { stop_reason: String },
    /// Error during streaming
    Error(String),
}

/// Stream of ACP events
pub struct AcpStream {
    rx: mpsc::Receiver<Result<AcpEvent>>,
}

impl Stream for AcpStream {
    type Item = Result<AcpEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        self.rx.poll_recv(cx)
    }
}

/// Client for communicating with ACP-compliant agents
pub struct AcpModelClient {
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    cwd: PathBuf,
}

impl AcpModelClient {
    /// Create a new ACP model client
    pub fn new(command: String, args: Vec<String>, cwd: PathBuf) -> Self {
        Self {
            command,
            args,
            env: vec![],
            cwd,
        }
    }

    /// Stream responses from the agent for a given prompt
    pub async fn stream(&self, prompt: &str) -> Result<AcpStream> {
        debug!("Starting ACP stream for prompt");

        // Create channel for events
        let (tx, rx) = mpsc::channel(16);

        // Clone values for the LocalSet task
        let command = self.command.clone();
        let args = self.args.clone();
        let env = self.env.clone();
        let cwd = self.cwd.clone();
        let prompt = prompt.to_string();

        // Spawn a dedicated thread with its own runtime and LocalSet for !Send futures
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    std::mem::drop(tx.send(Err(anyhow::anyhow!("Failed to build runtime: {e}"))));
                    return;
                }
            };

            let local = tokio::task::LocalSet::new();

            local.block_on(&rt, async move {
                // Spawn agent
                let mut agent = match AgentProcess::spawn(&command, &args, &env).await {
                    Ok(a) => a,
                    Err(e) => {
                        error!("Failed to spawn agent: {}", e);
                        let _ = tx.send(Err(e)).await;
                        return;
                    }
                };

                // Initialize
                let client_caps = json!({
                    "fs": { "readTextFile": true, "writeTextFile": true },
                    "terminal": true
                });
                if let Err(e) = agent.initialize(client_caps).await {
                    error!("Failed to initialize agent: {}", e);
                    let _ = tx.send(Err(e)).await;
                    return;
                }

                // Run session
                if let Err(e) = run_session(agent, &prompt, &cwd, tx.clone()).await {
                    error!("Session error: {}", e);
                    let _ = tx.send(Err(e)).await;
                }
            });
        });

        Ok(AcpStream { rx })
    }
}

/// Run a single session: create, prompt, stream events
async fn run_session(
    mut agent: AgentProcess,
    prompt: &str,
    cwd: &Path,
    tx: mpsc::Sender<Result<AcpEvent>>,
) -> Result<()> {
    // Create new session
    let session_id = agent
        .new_session(cwd.to_string_lossy().to_string(), vec![])
        .await
        .context("Failed to create session")?;

    debug!("Created session: {}", session_id);

    // Send prompt
    let prompt_content = vec![json!({
        "type": "text",
        "text": prompt
    })];

    // Create a future for processing events
    let tx_clone = tx.clone();
    let event_processor = async {
        loop {
            match agent.next_client_event().await {
                Some(ClientEvent::SessionUpdate(notification)) => {
                    process_session_update(notification.update, &tx_clone).await;
                }
                Some(ClientEvent::PermissionRequest(_)) => {
                    // Auto-approved in client_handler
                    continue;
                }
                None => break,
            }
        }
    };

    // Run prompt and event processing concurrently using tokio::select!
    // This works because both use the same agent without moving it
    let response = tokio::select! {
        result = agent.prompt(session_id, prompt_content) => {
            result?
        }
        _ = event_processor => {
            anyhow::bail!("Event processor ended unexpectedly")
        }
    };

    // Extract stop reason
    let stop_reason = response
        .get("stopReason")
        .and_then(|s| s.as_str())
        .unwrap_or("end_turn")
        .to_string();

    // Send completed event
    tx.send(Ok(AcpEvent::Completed { stop_reason })).await.ok();

    // Kill agent
    agent.kill().await.ok();

    Ok(())
}

/// Extract text from content if it's a text content block
fn extract_text_content(content: &Value) -> Option<&str> {
    if content.get("type").and_then(|t| t.as_str()) == Some("text") {
        content.get("text").and_then(|t| t.as_str())
    } else {
        None
    }
}

/// Process a session/update notification and emit appropriate events
async fn process_session_update(
    update: agent_client_protocol::SessionUpdate,
    tx: &mpsc::Sender<Result<AcpEvent>>,
) {
    let update_json = match serde_json::to_value(&update) {
        Ok(v) => v,
        Err(_) => return,
    };

    let update_type = update_json.get("sessionUpdate").and_then(|t| t.as_str());

    match update_type {
        Some("agent_message_chunk") => {
            if let Some(text) = update_json.get("content").and_then(extract_text_content) {
                let _ = tx.send(Ok(AcpEvent::TextDelta(text.to_string()))).await;
            }
        }
        Some("agent_thought_chunk") => {
            if let Some(text) = update_json.get("content").and_then(extract_text_content) {
                let _ = tx
                    .send(Ok(AcpEvent::ReasoningDelta(text.to_string())))
                    .await;
            }
        }
        _ => {
            // Ignore other update types for now
        }
    }
}
