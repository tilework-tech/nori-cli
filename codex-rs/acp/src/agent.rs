//! Agent subprocess management

use agent_client_protocol::{
    Agent, ClientSideConnection, InitializeRequest, NewSessionRequest, PromptRequest,
};
use anyhow::{Context, Result};
use serde_json::Value;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info, warn};

use crate::client_handler::{AcpClientHandler, ClientEvent};

/// Maximum number of stderr lines to buffer
const STDERR_BUFFER_CAPACITY: usize = 500;

/// Maximum length of a single stderr line in bytes (10KB)
const STDERR_LINE_MAX_LENGTH: usize = 10240;

/// ACP agent subprocess
pub struct AgentProcess {
    child: Child,
    connection: ClientSideConnection,
    _io_task: JoinHandle<agent_client_protocol::Result<()>>,
    capabilities: Option<Value>,
    /// Buffer for captured stderr lines
    stderr_lines: Arc<Mutex<Vec<String>>>,
    /// Channel receiver for client events
    client_event_rx: Arc<Mutex<tokio::sync::mpsc::Receiver<ClientEvent>>>,
}

impl AgentProcess {
    /// Spawn a new ACP agent subprocess
    ///
    /// # Arguments
    /// * `command` - Command to execute (e.g., "npx")
    /// * `args` - Arguments (e.g., ["@zed-industries/claude-code-acp"])
    /// * `env` - Additional environment variables
    pub async fn spawn(command: &str, args: &[String], env: &[(String, String)]) -> Result<Self> {
        info!("Spawning ACP agent: {} {:?}", command, args);

        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()) // Capture stderr for programmatic access
            .kill_on_drop(true);

        for (key, value) in env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().context("Failed to spawn ACP agent")?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let stderr = child.stderr.take().context("Failed to get stderr")?;

        // Create channel for client events
        let (client_event_tx, client_event_rx) = tokio::sync::mpsc::channel(16);

        // Create client handler
        let client_handler = AcpClientHandler::new(client_event_tx);

        // Create ClientSideConnection
        // Convert tokio AsyncRead/Write to futures AsyncRead/Write using compat layer
        let (connection, io_task) = ClientSideConnection::new(
            client_handler,
            stdin.compat_write(),
            stdout.compat(),
            |fut| {
                // Use spawn_local for !Send futures
                tokio::task::spawn_local(fut);
            },
        );

        // Spawn IO task
        let io_task = tokio::spawn(io_task);

        // Create shared buffer for stderr lines
        let stderr_lines = Arc::new(Mutex::new(Vec::with_capacity(STDERR_BUFFER_CAPACITY)));
        let stderr_lines_clone = Arc::clone(&stderr_lines);

        // Spawn task to read stderr lines
        tokio::spawn(async move {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        // Remove trailing newline
                        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');

                        // Truncate long lines to 10KB
                        let truncated = if trimmed.len() > STDERR_LINE_MAX_LENGTH {
                            &trimmed[..STDERR_LINE_MAX_LENGTH]
                        } else {
                            trimmed
                        };

                        let mut buffer = stderr_lines_clone.lock().await;

                        // If buffer is full, remove oldest line
                        if buffer.len() >= STDERR_BUFFER_CAPACITY {
                            buffer.remove(0);
                        }

                        buffer.push(truncated.to_string());
                    }
                    Err(e) => {
                        warn!("Error reading stderr: {}", e);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            child,
            connection,
            _io_task: io_task,
            capabilities: None,
            stderr_lines,
            client_event_rx: Arc::new(Mutex::new(client_event_rx)),
        })
    }

    /// Initialize the ACP agent with protocol handshake
    pub async fn initialize(&mut self, client_capabilities: Value) -> Result<Value> {
        debug!("Initializing ACP agent");

        let request = InitializeRequest {
            protocol_version: agent_client_protocol::V0,  // Gemini uses protocol version 0
            client_capabilities: serde_json::from_value(client_capabilities.clone())
                .context("Invalid client capabilities")?,
            client_info: None,
            meta: None,
        };

        // Log the initialization request
        match serde_json::to_string_pretty(&request) {
            Ok(json) => debug!("=== INITIALIZE REQUEST JSON ===\n{}\n=== END REQUEST ===", json),
            Err(e) => debug!("Failed to serialize init request to JSON: {}", e),
        }

        let response = self
            .connection
            .initialize(request)
            .await
            .map_err(|e| anyhow::anyhow!("Agent initialization failed: {e}"))?;

        // Log the full initialization response
        match serde_json::to_string_pretty(&response) {
            Ok(json) => debug!("=== INITIALIZE RESPONSE JSON ===\n{}\n=== END RESPONSE ===", json),
            Err(e) => debug!("Failed to serialize init response to JSON: {}", e),
        }

        let result = serde_json::to_value(&response.agent_capabilities)
            .context("Failed to serialize capabilities")?;

        self.capabilities = Some(result.clone());

        debug!("Agent initialized with capabilities: {:?}", result);
        Ok(result)
    }

    /// Create a new session
    pub async fn new_session(&self, cwd: String, _mcp_servers: Vec<Value>) -> Result<String> {
        let request = NewSessionRequest {
            cwd: std::path::PathBuf::from(cwd.clone()),
            mcp_servers: vec![],
            // mcp_servers
            //     .into_iter()
            //     .map(|v| serde_json::from_value(v))
            //     .collect::<Result<Vec<_>, _>>()
            //     .context("Invalid MCP server config")?,
            meta: None,
        };

        // Serialize request to JSON for debugging
        match serde_json::to_string_pretty(&request) {
            Ok(json) => debug!("=== NEW_SESSION REQUEST JSON ===\n{}\n=== END REQUEST ===", json),
            Err(e) => debug!("Failed to serialize request to JSON: {}", e),
        }

        debug!("Sending new_session request with cwd: {}", cwd);
        let response = self.connection.new_session(request).await.map_err(|e| {
            // Log the full error with all details
            error!("Protocol error creating session: {:?}", e);

            // Try to extract and log error details as JSON if available
            if let Some(err_str) = format!("{:?}", e).split("data:").nth(1) {
                error!("=== ERROR RESPONSE DETAILS ===\n{}\n=== END ERROR ===", err_str);
            }

            anyhow::anyhow!("Failed to create session: {e}")
        })?;

        // Log successful response as JSON
        match serde_json::to_string_pretty(&response) {
            Ok(json) => debug!("=== NEW_SESSION RESPONSE JSON ===\n{}\n=== END RESPONSE ===", json),
            Err(e) => debug!("Failed to serialize response to JSON: {}", e),
        }

        debug!("Received session_id: {}", response.session_id);
        Ok(response.session_id.to_string())
    }

    /// Send a prompt to the agent
    pub async fn prompt(&self, session_id: String, prompt: Vec<Value>) -> Result<Value> {
        let request = PromptRequest {
            session_id: serde_json::from_str(&format!("\"{session_id}\""))
                .context("Invalid session ID")?,
            prompt: prompt
                .into_iter()
                .map(serde_json::from_value)
                .collect::<Result<Vec<_>, _>>()
                .context("Invalid prompt content")?,
            meta: None,
        };

        let response = self
            .connection
            .prompt(request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send prompt: {e}"))?;

        serde_json::to_value(&response).context("Failed to serialize response")
    }

    /// Get the next client event (session update or permission request)
    pub async fn next_client_event(&self) -> Option<ClientEvent> {
        self.client_event_rx.lock().await.recv().await
    }

    /// Kill the agent subprocess
    pub async fn kill(&mut self) -> Result<()> {
        self.child.kill().await.context("Failed to kill agent")
    }

    /// Get agent capabilities (available after initialization)
    pub fn capabilities(&self) -> Option<&Value> {
        self.capabilities.as_ref()
    }

    /// Get captured stderr lines
    ///
    /// Returns a clone of all stderr lines captured so far from the agent subprocess.
    /// Lines are stored in order of receipt, with oldest first. The buffer is capped
    /// at 500 lines; when full, oldest lines are dropped.
    pub async fn get_stderr_lines(&self) -> Vec<String> {
        self.stderr_lines.lock().await.clone()
    }

    /// Get access to the underlying connection for subscribing to stream messages
    pub fn connection(&self) -> &ClientSideConnection {
        &self.connection
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::time::timeout;

    #[tokio::test]
    async fn test_agent_spawn() {
        // Test that we can spawn a simple subprocess (using cat as a stand-in)
        // Real testing requires the mock ACP agent from /mock-acp-agent
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let result = AgentProcess::spawn("cat", &[], &[]).await;
                assert!(result.is_ok());

                let mut agent = result.unwrap();
                agent.kill().await.ok();
            })
            .await;
    }

    #[tokio::test]
    #[ignore] // Requires mock-acp-agent to be available
    async fn test_agent_initialize_with_mock() {
        // This test assumes the mock-acp-agent package is available
        // In CI, we'd need to ensure it's installed first
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let args = vec!["mock-acp-agent".to_string()];
                let mut agent = AgentProcess::spawn("npx", &args, &[])
                    .await
                    .expect("Failed to spawn mock agent");

                let client_caps = serde_json::json!({
                    "tools": true,
                    "streaming": true,
                });

                let init_result = timeout(Duration::from_secs(5), agent.initialize(client_caps))
                    .await
                    .expect("Initialize timed out")
                    .expect("Initialize failed");

                assert!(init_result.is_object());
                assert!(agent.capabilities().is_some());

                agent.kill().await.ok();
            })
            .await;
    }

    #[tokio::test]
    async fn test_stderr_capture_basic() {
        // Spawn a shell command that writes to stderr then exits
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let args = vec![
                    "-c".to_string(),
                    "echo 'error line 1' >&2 && echo 'error line 2' >&2 && sleep 0.1".to_string(),
                ];
                let mut agent = AgentProcess::spawn("sh", &args, &[])
                    .await
                    .expect("Failed to spawn");

                // Give time for stderr to be written
                tokio::time::sleep(Duration::from_millis(200)).await;

                let stderr_lines = agent.get_stderr_lines().await;
                assert_eq!(stderr_lines.len(), 2);
                assert_eq!(stderr_lines[0], "error line 1");
                assert_eq!(stderr_lines[1], "error line 2");

                agent.kill().await.ok();
            })
            .await;
    }

    #[tokio::test]
    async fn test_stderr_capture_empty() {
        // Spawn a command that writes nothing to stderr
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let args = vec!["-c".to_string(), "sleep 0.1".to_string()];
                let mut agent = AgentProcess::spawn("sh", &args, &[])
                    .await
                    .expect("Failed to spawn");

                tokio::time::sleep(Duration::from_millis(200)).await;

                let stderr_lines = agent.get_stderr_lines().await;
                assert!(stderr_lines.is_empty());

                agent.kill().await.ok();
            })
            .await;
    }

    #[tokio::test]
    async fn test_stderr_capture_overflow() {
        // Spawn a command that writes more than buffer capacity (500 lines)
        // Write 600 lines to test that only the last 500 are retained
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let args = vec![
                    "-c".to_string(),
                    "for i in $(seq 1 600); do echo \"stderr line $i\" >&2; done && sleep 0.1"
                        .to_string(),
                ];
                let mut agent = AgentProcess::spawn("sh", &args, &[])
                    .await
                    .expect("Failed to spawn");

                // Give time for all stderr to be written
                tokio::time::sleep(Duration::from_millis(500)).await;

                let stderr_lines = agent.get_stderr_lines().await;
                assert_eq!(
                    stderr_lines.len(),
                    500,
                    "Buffer should be capped at 500 lines"
                );
                // First line in buffer should be line 101 (lines 1-100 dropped)
                assert_eq!(stderr_lines[0], "stderr line 101");
                // Last line should be line 600
                assert_eq!(stderr_lines[499], "stderr line 600");

                agent.kill().await.ok();
            })
            .await;
    }

    #[tokio::test]
    async fn test_stderr_line_truncation() {
        // Spawn a command that writes a line longer than 10KB
        // Create a line of 15000 characters (15KB) using head -c which is POSIX compliant
        let local = tokio::task::LocalSet::new();
        local.run_until(async {
            let args = vec![
                "-c".to_string(),
                "head -c 15000 < /dev/zero | tr '\\0' 'X' >&2 && echo '' >&2 && echo 'normal line' >&2 && sleep 0.1".to_string(),
            ];
            let mut agent = AgentProcess::spawn("sh", &args, &[])
                .await
                .expect("Failed to spawn");

            tokio::time::sleep(Duration::from_millis(300)).await;

            let stderr_lines = agent.get_stderr_lines().await;
            assert_eq!(stderr_lines.len(), 2);
            // First line should be truncated to 10KB (10240 bytes)
            assert_eq!(
                stderr_lines[0].len(),
                10240,
                "Long line should be truncated to 10KB"
            );
            // Second line should be normal
            assert_eq!(stderr_lines[1], "normal line");

            agent.kill().await.ok();
        }).await;
    }
}
