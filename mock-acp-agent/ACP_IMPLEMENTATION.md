# ACP Phase 2 Implementation Plan (FINAL)

Goal: Implement the spawn_stream() method in AcpAgentRunner using ClientSideConnection from agent-client-protocol crate,
 which provides complete JSON-RPC transport over stdin/stdout.

Architecture: Spawn agent subprocess with piped stdio. Create ClientSideConnection::new() with our AcpClientHandler
(implements Client trait) and the subprocess's stdin/stdout. The connection implements the Agent trait, allowing us to
call .initialize(), .new_session(), .prompt() directly. Subscribe to the connection's StreamReceiver to receive session
updates, translate them to ConversationEvent, and stream via mpsc channel.

Tech Stack:
- agent-client-protocol 0.7.0 (already in deps)
- tokio with LocalSet (ACP futures are !Send)
- tokio_util::compat for stdio compatibility adapters
- tokio-stream for mpsc → Stream conversion

Key Insight: The agent-client-protocol crate provides the complete bidirectional transport. We don't implement JSON-RPC
manually or use jsonrpsee. We just wire up the connection and call trait methods.

---
Testing Plan

Integration Test 1: ACP Handshake and Session Creation
- File: /home/clifford/Documents/source/nori-cli/.worktrees/acp-implementation-phase2/tests/acp_runner_test.rs
- Behavior: Verify that spawn_stream() successfully completes initialization → session creation
- Mock: Use modified example agent that implements basic ACP protocol
- Assertion: spawn_stream() returns Ok(stream)

Integration Test 2: Session Update Notifications
- Behavior: Verify that agent's session/update notifications become ConversationEvents in stream
- Mock: Agent sends AgentMessageChunk updates during prompt processing
- Assertion: Stream yields ConversationEvent::AssistantMessage events

Integration Test 3: Agent Calls Client Methods
- Behavior: Verify that when agent needs to read a file, our AcpClientHandler responds correctly
- Mock: Agent sends read_text_file request during prompt processing
- Assertion: Agent receives file content, continues processing

Integration Test 4: Cancellation
- Behavior: Verify cancellation stops stream and notifies agent
- Mock: Agent that sends continuous updates
- Assertion: After cancel_token.cancel(), stream terminates

Integration Test 5: Error Handling
- Behavior: Verify spawn failures and protocol errors are reported
- Mock: Invalid command, broken agent
- Assertion: Returns Err with descriptive message

Integration Test 6: Initialization Timeout
- Behavior: Verify timeout if agent doesn't respond to initialize
- Mock: Agent that hangs on initialize
- Assertion: Returns Err("Initialization timeout after 30s")

NOTE: I will write all tests before I add any implementation behavior.

---
Implementation Steps

Step 1: Add Required Dependencies

File: /home/clifford/Documents/source/nori-cli/.worktrees/acp-implementation-phase2/Cargo.toml:30

Add after agent-client-protocol line:

tokio-stream = "0.1"
tokio-util = { version = "0.7", features = ["compat"] }

Why:
- tokio-stream - convert mpsc channel to Stream
- tokio-util compat - needed for .compat() and .compat_write() on stdin/stdout

Command: cargo build

Step 2: Create Mock ACP Agent Binary

File: /home/clifford/Documents/source/nori-cli/.worktrees/acp-implementation-phase2/tests/mock_acp_agent/Cargo.toml
(new)

[package]
name = "mock_acp_agent"
version = "0.1.0"
edition = "2024"

[[bin]]
name = "mock_acp_agent"
path = "src/main.rs"

[dependencies]
agent-client-protocol = "0.7.0"
tokio = { version = "1", features = ["full"] }
tokio-util = { version = "0.7", features = ["compat"] }
async-trait = "0.1"
serde_json = "1.0"
env_logger = "0.11"

File: /home/clifford/Documents/source/nori-cli/.worktrees/acp-implementation-phase2/tests/mock_acp_agent/src/main.rs
(new)

Copy from the example agent with modifications:

//! Mock ACP agent for testing nori-cli

use std::cell::Cell;
use agent_client_protocol::{self as acp, Client as _};
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

struct MockAgent {
    session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
    next_session_id: Cell<u64>,
}

impl MockAgent {
    fn new(
        session_update_tx: mpsc::UnboundedSender<(acp::SessionNotification, oneshot::Sender<()>)>,
    ) -> Self {
        Self {
            session_update_tx,
            next_session_id: Cell::new(0),
        }
    }

    async fn send_update(&self, session_id: acp::SessionId, update: acp::SessionUpdate) -> Result<(), acp::Error> {
        let (tx, rx) = oneshot::channel();
        self.session_update_tx
            .send((
                acp::SessionNotification {
                    session_id,
                    update,
                    meta: None,
                },
                tx,
            ))
            .map_err(|_| acp::Error::internal_error())?;
        rx.await.map_err(|_| acp::Error::internal_error())?;
        Ok(())
    }
}

#[async_trait::async_trait(?Send)]
impl acp::Agent for MockAgent {
    async fn initialize(
        &self,
        arguments: acp::InitializeRequest,
    ) -> Result<acp::InitializeResponse, acp::Error> {
        // Check env var for hang simulation
        if std::env::var("MOCK_AGENT_HANG").is_ok() {
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        }

        eprintln!("Mock agent: initialize");
        Ok(acp::InitializeResponse {
            protocol_version: acp::V1,
            agent_capabilities: acp::AgentCapabilities::default(),
            auth_methods: Vec::new(),
            agent_info: Some(acp::Implementation {
                name: "mock-agent".to_string(),
                title: Some("Mock Agent".to_string()),
                version: "0.1.0".to_string(),
            }),
            meta: None,
        })
    }

    async fn authenticate(
        &self,
        _arguments: acp::AuthenticateRequest,
    ) -> Result<acp::AuthenticateResponse, acp::Error> {
        Ok(acp::AuthenticateResponse::default())
    }

    async fn new_session(
        &self,
        _arguments: acp::NewSessionRequest,
    ) -> Result<acp::NewSessionResponse, acp::Error> {
        let session_id = self.next_session_id.get();
        self.next_session_id.set(session_id + 1);
        eprintln!("Mock agent: new_session id={}", session_id);
        Ok(acp::NewSessionResponse {
            session_id: acp::SessionId(session_id.to_string().into()),
            modes: None,
            meta: None,
        })
    }

    async fn load_session(
        &self,
        _arguments: acp::LoadSessionRequest,
    ) -> Result<acp::LoadSessionResponse, acp::Error> {
        Ok(acp::LoadSessionResponse {
            modes: None,
            meta: None,
        })
    }

    async fn prompt(
        &self,
        arguments: acp::PromptRequest,
    ) -> Result<acp::PromptResponse, acp::Error> {
        eprintln!("Mock agent: prompt");

        // Send a few test messages
        self.send_update(
            arguments.session_id.clone(),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk {
                content: acp::ContentBlock::Text(acp::TextContent {
                    annotations: None,
                    text: "Test message 1".to_string(),
                    meta: None,
                }),
                meta: None,
            })
        ).await?;

        self.send_update(
            arguments.session_id.clone(),
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk {
                content: acp::ContentBlock::Text(acp::TextContent {
                    annotations: None,
                    text: "Test message 2".to_string(),
                    meta: None,
                }),
                meta: None,
            })
        ).await?;

        // Check if we should request a file read (for testing client callbacks)
        if let Ok(file_path) = std::env::var("MOCK_AGENT_REQUEST_FILE") {
            eprintln!("Mock agent: requesting file read: {}", file_path);
            // This would be done via ClientSideConnection back to the client
            // For now, just log it
        }

        Ok(acp::PromptResponse {
            stop_reason: acp::StopReason::EndTurn,
            meta: None,
        })
    }

    async fn cancel(&self, _args: acp::CancelNotification) -> Result<(), acp::Error> {
        eprintln!("Mock agent: cancel");
        Ok(())
    }

    async fn set_session_mode(
        &self,
        _args: acp::SetSessionModeRequest,
    ) -> Result<acp::SetSessionModeResponse, acp::Error> {
        Ok(acp::SetSessionModeResponse::default())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> Result<acp::ExtResponse, acp::Error> {
        Ok(serde_json::value::to_raw_value(&json!({}))?.into())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> Result<(), acp::Error> {
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> acp::Result<()> {
    env_logger::init();

    let outgoing = tokio::io::stdout().compat_write();
    let incoming = tokio::io::stdin().compat();

    let local_set = tokio::task::LocalSet::new();
    local_set
        .run_until(async move {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            let (conn, handle_io) =
                acp::AgentSideConnection::new(MockAgent::new(tx), outgoing, incoming, |fut| {
                    tokio::task::spawn_local(fut);
                });

            tokio::task::spawn_local(async move {
                while let Some((session_notification, tx)) = rx.recv().await {
                    let result = conn.session_notification(session_notification).await;
                    if let Err(e) = result {
                        eprintln!("Mock agent error: {e}");
                        break;
                    }
                    tx.send(()).ok();
                }
            });

            handle_io.await
        })
        .await
}

Why: Based on the official example, this gives us a realistic test agent that implements the full ACP protocol.

Step 3: Build Mock Agent

Command:
cargo build --manifest-path tests/mock_acp_agent/Cargo.toml

Expected: Compiles successfully, binary at target/debug/mock_acp_agent

Step 4: Write Failing Test - Handshake

File: /home/clifford/Documents/source/nori-cli/.worktrees/acp-implementation-phase2/tests/acp_runner_test.rs (new)

use nori_cli::acp_runner::{AcpAgentRunner, AcpAgentConfig};
use tokio_util::sync::CancellationToken;
use std::path::PathBuf;

#[tokio::test]
async fn test_acp_handshake_succeeds() {
    // Ensure mock agent is built
    let build = std::process::Command::new("cargo")
        .args(&["build", "--manifest-path", "tests/mock_acp_agent/Cargo.toml"])
        .output()
        .expect("Failed to build mock agent");

    assert!(build.status.success(), "Mock agent build failed");

    let config = AcpAgentConfig {
        name: "mock",
        command: "target/debug/mock_acp_agent",
        args: vec![],
        install_url: "",
        install_command: None,
    };

    let mut runner = AcpAgentRunner::new(config, PathBuf::from("/tmp"));
    let cancel_token = CancellationToken::new();

    let result = runner.spawn_stream("test prompt".to_string(), cancel_token).await;

    assert!(result.is_ok(), "spawn_stream should succeed: {:?}", result);
}

Expected: Test fails with "Not implemented"

Command: cargo test test_acp_handshake_succeeds

Step 5-8: Write Other Failing Tests

Similar structure for:
- test_session_updates_are_streamed
- test_agent_calls_read_text_file (will be tricky - might need to enhance mock agent)
- test_cancellation_stops_stream
- test_spawn_failure_returns_error
- test_initialization_timeout

Step 9: Run All Tests - Verify Failures

Command: cargo test acp_runner_test

Expected: All 6 tests fail with "Not implemented"

Step 10: Implement spawn_stream() - COMPLETE

File: /home/clifford/Documents/source/nori-cli/.worktrees/acp-implementation-phase2/src/acp_runner.rs:251-258

Replace the stub implementation:

use agent_client_protocol::{self as acp, Agent, InitializeRequest, NewSessionRequest, PromptRequest,
ClientSideConnection, ContentBlock, TextContent};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

pub async fn spawn_stream(
    &mut self,
    prompt: String,
    cancel_token: CancellationToken,
) -> Result<Pin<Box<dyn Stream<Item = ConversationEvent> + Send>>, String> {
    // Step 1: Spawn subprocess
    let mut child = Command::new(&self.config.command)
        .args(&self.config.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to spawn agent: {}", e))?;

    let stdin = child.stdin.take().ok_or("Failed to get stdin")?;
    let stdout = child.stdout.take().ok_or("Failed to get stdout")?;

    // Store child for lifecycle management
    self._agent_process = Some(child);

    // Step 2: Create event channel for streaming to user
    let (event_tx, event_rx) = mpsc::unbounded_channel::<ConversationEvent>();

    // Step 3: Create another channel for session updates from agent
    let (session_update_tx, mut session_update_rx) = mpsc::unbounded_channel();

    // Step 4: Create client handler
    let client_handler = AcpClientHandler::new(
        self.cwd.clone(),
        session_update_tx.clone(),
        cancel_token.clone()
    );

    // Step 5: Use LocalSet because ACP futures are !Send
    let local = tokio::task::LocalSet::new();

    // Step 6: Create ClientSideConnection
    let (connection, io_future) = ClientSideConnection::new(
        client_handler,
        stdin.compat_write(),  // Convert tokio AsyncWrite to futures AsyncWrite
        stdout.compat(),        // Convert tokio AsyncRead to futures AsyncRead
        |fut| {
            tokio::task::spawn_local(fut);
        }
    );

    // Step 7: Spawn the I/O handler in LocalSet
    local.spawn_local(async move {
        if let Err(e) = io_future.await {
            eprintln!("ACP connection error: {:?}", e);
        }
    });

    // Step 8: Spawn task to translate SessionUpdates to ConversationEvents
    local.spawn_local(async move {
        while let Some(update) = session_update_rx.recv().await {
            if let Some(event) = translate_session_update(update) {
                let _ = event_tx.send(event);
            }
        }
    });

    // Step 9: Subscribe to session updates from connection
    let mut stream_receiver = connection.subscribe();
    local.spawn_local(async move {
        while let Some(msg) = stream_receiver.recv().await {
            // StreamMessage contains various update types
            // Forward to session_update_tx for translation
            // TODO: Figure out StreamMessage structure
        }
    });

    // Step 10: Initialize with timeout
    let init_request = InitializeRequest {
        protocol_version: acp::V1,
        client_capabilities: acp::ClientCapabilities {
            fs: Some(acp::FsCapabilities {
                read_text_file: true,
                write_text_file: true,
            }),
            terminal: None,
        },
        client_info: Some(acp::Implementation {
            name: "nori-cli".to_string(),
            title: Some("Nori CLI".to_string()),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }),
        meta: None,
    };

    let connection_clone = connection.clone(); // Check if Clone is implemented
    let init_response = tokio::time::timeout(
        tokio::time::Duration::from_secs(30),
        local.run_until(async move {
            connection_clone.initialize(init_request).await
        })
    )
    .await
    .map_err(|_| "Initialization timeout after 30s".to_string())?
    .map_err(|e| format!("Initialize failed: {:?}", e))?;

    // Validate protocol version
    if init_response.protocol_version != acp::V1 {
        return Err(format!("Unsupported protocol version: {:?}", init_response.protocol_version));
    }

    // Step 11: Create session
    let session_request = NewSessionRequest {
        cwd: self.cwd.clone(),
        mcp_servers: vec![],
    };

    let connection_clone2 = connection.clone();
    let session_response = local.run_until(async move {
        connection_clone2.new_session(session_request).await
    })
    .await
    .map_err(|e| format!("Session creation failed: {:?}", e))?;

    let session_id = session_response.session_id;

    // Step 12: Send prompt in background
    let connection_clone3 = connection.clone();
    local.spawn_local(async move {
        let prompt_request = PromptRequest {
            session_id,
            prompt: vec![ContentBlock::Text(TextContent {
                annotations: None,
                text: prompt,
                meta: None,
            })],
        };

        let _ = connection_clone3.prompt(prompt_request).await;
    });

    // Step 13: Keep LocalSet running
    tokio::task::spawn_local(async move {
        local.await;
    });

    // Step 14: Return stream
    let stream = UnboundedReceiverStream::new(event_rx);
    Ok(Box::pin(stream))
}

Note: This pseudocode has several issues that need to be resolved:
1. ClientSideConnection might not be Clone
2. LocalSet needs to keep running but also return immediately
3. Need to understand StreamMessage structure
4. Session update routing needs work

Step 11: Resolve LocalSet and Clone Issues

This is the tricky part. The ACP futures are !Send, so they need to run on a LocalSet. But we need to return immediately
 from spawn_stream(), not block on local.await.

Solution: Spawn a background tokio task that runs the LocalSet:

// Spawn background task to run LocalSet
tokio::task::spawn(async move {
    local.await;
});

For the Clone issue, wrap connection in Rc:

use std::rc::Rc;

let connection = Rc::new(connection);
let connection_init = connection.clone();
let connection_session = connection.clone();
let connection_prompt = connection.clone();

Note: Rc is not Send, so this might not work with tokio::task::spawn. Might need to use channels to communicate with the
 LocalSet task.

Step 12-20: Test, Debug, Iterate

This is where the real work happens. The ACP integration has some tricky async lifetime issues. Expect to spend several
hours debugging:
- LocalSet lifecycle management
- Connection cloning or alternative patterns
- StreamMessage translation
- Session update routing

Strategy:
1. Get basic test passing first (handshake)
2. Add logging everywhere
3. Read ACP crate source code if needed
4. Check for examples in ACP repo
5. Iterate until all tests pass

Step 21: Run Full Test Suite

Command: cargo test

Expected: All 71 existing + 6 new = 77 tests pass

---
Key Implementation Challenges

1. LocalSet Lifecycle: ACP futures are !Send, must run on LocalSet, but we need to return immediately from
spawn_stream()
  - Solution: Background task that runs LocalSet indefinitely
2. Connection is not Clone: Can't clone ClientSideConnection to use in multiple places
  - Solution: Use channels to communicate with LocalSet-bound tasks, or wrap in Rc (not Send!)
3. StreamMessage structure: Need to understand what StreamReceiver yields
  - Solution: Check docs or source code for StreamMessage type
4. Session updates from ClientHandler: Our AcpClientHandler gets session updates via session_notification(), needs to
forward to stream
  - Already handled - we pass session_update_tx channel

---
Testing Details

Tests verify BEHAVIOR:
- Handshake completes successfully
- Session updates appear in stream
- Agent can call client methods
- Cancellation stops stream
- Errors are reported properly
- Timeout works

NOT testing: Internal ACP protocol details, JSON framing, connection internals.

Implementation Details

- Use agent-client-protocol's ClientSideConnection for all transport
- Mock agent based on official example from rust-sdk repo
- LocalSet required for !Send futures
- tokio-util compat adapters for stdin/stdout
- Event translation via existing translate_session_update() function
- Subprocess stored in _agent_process field
- Stream created from mpsc channel via UnboundedReceiverStream
- 30 second timeout on initialization
- Cancellation monitoring via background task

