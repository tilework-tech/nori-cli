//! Mock ACP agent for testing nori-cli

mod browser_modify;
mod runaway_search;

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI64;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;

use agent_client_protocol::Agent;
use agent_client_protocol::ByteStreams;
use agent_client_protocol::Client;
use agent_client_protocol::ConnectionTo;
use agent_client_protocol::Responder;
use futures::io::AsyncRead;
use nori_protocol::acp::v1 as acp;
use serde_json::json;
use tokio::time::Duration;
use tokio::time::sleep;
use tokio_util::compat::TokioAsyncReadCompatExt;
use tokio_util::compat::TokioAsyncWriteCompatExt;

/// Incoming byte stream that exits the process when the peer closes stdin.
///
/// The SDK's connection future never resolves on a clean stdin EOF: its
/// background work is a `try_join!` of four actors, and EOF only ends the
/// incoming one while the others stay alive as long as connection handles
/// exist. Without this, the mock would hang after the client disconnects and
/// its parent would have to SIGKILL it instead of letting it exit cleanly.
struct ExitOnEof<R> {
    inner: R,
}

impl<R> ExitOnEof<R> {
    fn new(inner: R) -> Self {
        Self { inner }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for ExitOnEof<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<std::io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(0)) if !buf.is_empty() => {
                // MOCK_AGENT_IGNORE_EOF simulates an agent whose EOF teardown
                // stalls (e.g. a hung broker release): stay alive instead of
                // exiting, so tests can prove the client's bounded child
                // lifecycle never waits indefinitely on a stuck child.
                if std::env::var("MOCK_AGENT_IGNORE_EOF").is_ok() {
                    eprintln!("Mock agent: ignoring stdin EOF (MOCK_AGENT_IGNORE_EOF is set)");
                    std::thread::sleep(std::time::Duration::from_secs(60));
                }
                std::process::exit(0)
            }
            other => other,
        }
    }
}

/// Shared, thread-safe mock agent state.
///
/// The `agent-client-protocol` SDK requires `Send` handlers, so state is held
/// behind atomics and a `std::sync::Mutex` and shared across handler closures
/// via `Arc`.
#[derive(Default)]
struct MockState {
    next_session_id: AtomicI64,
    cancel_requested: AtomicBool,
    session_configs: Mutex<HashMap<String, MockSessionConfig>>,
}

#[derive(Clone)]
struct MockSessionConfig {
    model_id: String,
    thought_level: Option<String>,
    speed: Option<String>,
}

fn default_session_config() -> MockSessionConfig {
    MockSessionConfig {
        model_id: "mock-model-default".to_string(),
        thought_level: Some("medium".to_string()),
        speed: None,
    }
}

fn config_options_for_state(config: &MockSessionConfig) -> Vec<acp::SessionConfigOption> {
    let mut options = vec![
        acp::SessionConfigOption::select(
            "model",
            "Model",
            config.model_id.clone(),
            vec![
                acp::SessionConfigSelectOption::new("mock-model-default", "Mock Default Model")
                    .description("The default mock model"),
                acp::SessionConfigSelectOption::new("mock-model-fast", "Mock Fast Model")
                    .description("A faster mock model variant"),
            ],
        )
        .category(acp::SessionConfigOptionCategory::Model),
    ];

    if config.model_id == "mock-model-fast" {
        options.push(
            acp::SessionConfigOption::select(
                "speed",
                "Speed",
                config.speed.clone().unwrap_or_else(|| "fast".to_string()),
                vec![
                    acp::SessionConfigSelectOption::new("fast", "Fast"),
                    acp::SessionConfigSelectOption::new("balanced", "Balanced"),
                ],
            )
            .description("Latency profile for the active model"),
        );
    } else {
        options.push(
            acp::SessionConfigOption::select(
                "thought_level",
                "Thought Level",
                config
                    .thought_level
                    .clone()
                    .unwrap_or_else(|| "medium".to_string()),
                vec![
                    acp::SessionConfigSelectOption::new("low", "Low"),
                    acp::SessionConfigSelectOption::new("medium", "Medium"),
                    acp::SessionConfigSelectOption::new("high", "High"),
                ],
            )
            .description("Reasoning depth for the active model")
            .category(acp::SessionConfigOptionCategory::ThoughtLevel),
        );
    }

    options
}

/// Per-request context bundling the connection to the client with shared state.
///
/// Constructed inside request handlers from the `ConnectionTo<Client>` supplied
/// by the SDK. The agent reaches the client through `cx` (notifications and
/// requests) and tracks cross-handler state through `state`.
struct MockAgent {
    cx: ConnectionTo<Client>,
    state: Arc<MockState>,
}

impl MockAgent {
    fn cancel_requested(&self) -> bool {
        self.state.cancel_requested.load(Ordering::SeqCst)
    }

    async fn send_update(
        &self,
        session_id: acp::SessionId,
        update: acp::SessionUpdate,
    ) -> Result<(), acp::Error> {
        self.cx
            .send_notification(acp::SessionNotification::new(session_id, update))
    }

    async fn send_text_chunk(
        &self,
        session_id: acp::SessionId,
        text: &str,
    ) -> Result<(), acp::Error> {
        self.send_update(
            session_id,
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
                acp::TextContent::new(text),
            ))),
        )
        .await
    }

    /// Send a tool call notification
    async fn send_tool_call(
        &self,
        session_id: acp::SessionId,
        tool_call: acp::ToolCall,
    ) -> Result<(), acp::Error> {
        self.send_update(session_id, acp::SessionUpdate::ToolCall(tool_call))
            .await
    }

    /// Send a tool call update notification
    async fn send_tool_call_update(
        &self,
        session_id: acp::SessionId,
        update: acp::ToolCallUpdate,
    ) -> Result<(), acp::Error> {
        self.send_update(session_id, acp::SessionUpdate::ToolCallUpdate(update))
            .await
    }

    async fn read_file_via_client(
        &self,
        session_id: acp::SessionId,
        path: PathBuf,
    ) -> Result<String, acp::Error> {
        let response = self
            .cx
            .send_request(acp::ReadTextFileRequest::new(session_id, path))
            .block_task()
            .await?;
        Ok(response.content)
    }

    /// Write a file via the client's fs/write_text_file method
    async fn write_file_via_client(
        &self,
        session_id: acp::SessionId,
        path: PathBuf,
        content: String,
    ) -> Result<(), acp::Error> {
        self.cx
            .send_request(acp::WriteTextFileRequest::new(session_id, path, content))
            .block_task()
            .await?;
        Ok(())
    }

    /// Request permission from the client for a tool call
    async fn request_permission_via_client(
        &self,
        session_id: acp::SessionId,
        tool_call: acp::ToolCallUpdate,
        options: Vec<acp::PermissionOption>,
    ) -> Result<acp::RequestPermissionResponse, acp::Error> {
        self.cx
            .send_request(acp::RequestPermissionRequest::new(
                session_id, tool_call, options,
            ))
            .block_task()
            .await
    }

    async fn run_prompt(
        self,
        arguments: acp::PromptRequest,
    ) -> Result<acp::PromptResponse, acp::Error> {
        eprintln!("Mock agent: prompt");
        if std::env::var("MOCK_AGENT_EXIT_DURING_PROMPT").is_ok() {
            eprintln!("Mock agent: exiting during prompt");
            std::process::exit(17);
        }
        self.state.cancel_requested.store(false, Ordering::SeqCst);
        let session_id = arguments.session_id.clone();

        // Advertise a native `compact` slash command so the harness forwards
        // `/compact` as an ordinary turn instead of summarize-and-swap.
        if std::env::var("MOCK_AGENT_ADVERTISE_COMPACT").is_ok() {
            self.send_update(
                session_id.clone(),
                acp::SessionUpdate::AvailableCommandsUpdate(acp::AvailableCommandsUpdate::new(
                    vec![acp::AvailableCommand::new(
                        "compact",
                        "Summarize conversation",
                    )],
                )),
            )
            .await?;
        }

        // Reply to the summarize-and-swap compaction prompt with a summary
        // message so the fallback path has a `last_agent_message` to stash.
        let prompt_text = arguments
            .prompt
            .iter()
            .filter_map(|block| match block {
                acp::ContentBlock::Text(text) => Some(text.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        if prompt_text.contains("CONTEXT CHECKPOINT COMPACTION") {
            eprintln!("Mock agent: replying to compaction prompt with a summary");
            self.send_text_chunk(session_id.clone(), "MOCK COMPACT SUMMARY")
                .await?;
            return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
        }

        let request_permission = arguments.prompt.iter().any(|block| {
            matches!(
                block,
                acp::ContentBlock::Text(text) if text.text == "mock:request-permission"
            )
        });
        // Support configurable stderr output for testing stderr capture
        if let Ok(count_str) = std::env::var("MOCK_AGENT_STDERR_COUNT")
            && let Ok(count) = count_str.parse::<usize>()
        {
            for i in 0..count {
                eprintln!("MOCK_AGENT_STDERR_LINE:{i}");
            }
        }

        // Check for special test modes first before sending default messages
        // Each special mode should return early to avoid executing default behavior

        // Support simulating prompt failures for testing error propagation
        if std::env::var("MOCK_AGENT_PROMPT_FAIL").is_ok() {
            eprintln!("Mock agent: simulating prompt failure");
            return Err(acp::Error::new(-32001, "Mock prompt failure for testing"));
        }

        // Support simulating a structured prompt failure (JSON-RPC code -32010,
        // "agent unreachable") whose `data` includes both a `detail` string and
        // unrelated noise fields — used to verify the user-facing message
        // surfaces `detail` cleanly instead of the raw pretty-printed JSON blob.
        if std::env::var("MOCK_AGENT_PROMPT_FAIL_JSON").is_ok() {
            eprintln!("Mock agent: simulating structured prompt failure with detail");
            return Err(
                acp::Error::new(-32010, "broker unreachable").data(serde_json::json!({
                    "detail": "connection reset by broker",
                    "retry_after_ms": 5000,
                    "trace_id": "mock-trace-9001"
                })),
            );
        }

        // Support simulating a structured prompt failure whose `data` carries
        // no `detail` field (only unrelated noise) — the message must still
        // avoid dumping the raw JSON blob even when there's no clean detail
        // string to substitute.
        if std::env::var("MOCK_AGENT_PROMPT_FAIL_JSON_NO_DETAIL").is_ok() {
            eprintln!("Mock agent: simulating structured prompt failure without detail");
            return Err(
                acp::Error::new(-32010, "broker unreachable").data(serde_json::json!({
                    "retry_after_ms": 7000,
                    "trace_id": "mock-trace-9002"
                })),
            );
        }

        // Support multi-turn conversations for transcript testing.
        // Extracts markers (ALPHA, BETA, etc.) from user input and echoes them back.
        if std::env::var("MOCK_AGENT_MULTI_TURN").is_ok() {
            let user_text = arguments
                .prompt
                .iter()
                .filter_map(|block| match block {
                    acp::ContentBlock::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");

            let marker = if user_text.contains("ALPHA") {
                "ALPHA"
            } else if user_text.contains("BETA") {
                "BETA"
            } else if user_text.contains("GAMMA") {
                "GAMMA"
            } else {
                "ECHO"
            };

            eprintln!("Mock agent: multi-turn response with marker {marker}");
            self.send_text_chunk(session_id.clone(), &format!("RESPONSE_{marker}"))
                .await?;
            return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
        }

        if std::env::var("MOCK_AGENT_RUNAWAY_SEARCH").is_ok() {
            return runaway_search::run(&self, session_id).await;
        }

        // Reproduce the orphan tool cell bug caused by cascade deferral.
        //
        // The bug sequence:
        // 1. Tool A Begin → handled immediately (no stream active)
        // 2. Text streaming starts → stream_controller = Some
        // 3. Tool A End arrives → DEFERRED (stream active), queue now non-empty
        // 4. Tool B Begin arrives → on_exec_command_begin calls
        //    flush_answer_stream_with_separator() which clears stream_controller,
        //    BUT !interrupts.is_empty() is still true → DEFERRED
        // 5. Tool B End arrives → queue non-empty → DEFERRED
        // 6. Final text + turn ends
        // 7. flush_completions_and_clear: End-A processed (OK), Begin-B
        //    discarded, End-B processed → no running_commands entry →
        //    orphan ExecCell created with raw call_id as command name
        //
        // The orphan cell renders as "• Ran orphan-tool-b / └ No files found"
        // which is the exact user-reported bug.
        if std::env::var("MOCK_AGENT_ORPHAN_TOOL_CELLS").is_ok() {
            eprintln!("Mock agent: sending orphan tool cell reproduction sequence");

            // Step 1: Tool A Begin (handled immediately, no stream active)
            // IMPORTANT: Tool A must be an Execute kind (not Read/Search) so that
            // when its End event creates an ExecCell, the cell gets flushed to
            // history immediately (non-exploring cells don't stay in active_cell).
            // This leaves active_cell = None when End-B arrives, triggering the
            // orphan cell creation path.
            let tool_a = acp::ToolCallId::new("tool-a-001");
            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(tool_a.clone(), "Running tests")
                    .kind(acp::ToolKind::Execute)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"command": "cargo test"})),
            )
            .await?;

            sleep(Duration::from_millis(50)).await;

            // Step 2: Start text streaming (activates stream_controller)
            self.send_text_chunk(session_id.clone(), "Analyzing the code.")
                .await?;

            sleep(Duration::from_millis(50)).await;

            // Step 3: Tool A End (deferred because stream_controller is active)
            // This makes the interrupt queue non-empty.
            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    tool_a.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new("Tests passed")),
                        ))])
                        .raw_output(json!({"exit_code": 0})),
                ),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            // Step 4: Tool B Begin (cascade-deferred due to non-empty queue)
            // on_exec_command_begin calls flush_answer_stream_with_separator()
            // which clears stream_controller, but !interrupts.is_empty() is
            // still true, so this gets deferred.
            let tool_b = acp::ToolCallId::new("orphan-tool-b");
            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(tool_b.clone(), "Running lint")
                    .kind(acp::ToolKind::Execute)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"command": "cargo clippy"})),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            // Step 5: Tool B End (deferred, queue still non-empty)
            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    tool_b.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new("No files found")),
                        ))])
                        .raw_output(json!({"exit_code": 0})),
                ),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            // Step 6: Send final text
            self.send_text_chunk(session_id.clone(), " Here is the final analysis result.")
                .await?;

            // Step 7: Turn ends → flush_completions_and_clear processes:
            //   End-A: processed OK (running_commands has tool-a-001)
            //   Begin-B: discarded
            //   End-B: processed, but no running_commands entry for orphan-tool-b
            //          → creates orphan ExecCell with command = ["orphan-tool-b"]
            return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
        }

        // Reproduce the stuck ExecCell bug: tool calls that Begin but never
        // Complete before the agent sends its final text response. This causes
        // incomplete ExecCells to fill the viewport and block the agent's message
        // from rendering. The fix (finalize_active_cell_as_failed) should clean
        // up these cells so the agent text is visible.
        if std::env::var("MOCK_AGENT_STUCK_TOOL_CALLS").is_ok() {
            eprintln!("Mock agent: sending stuck tool calls (no completion)");

            // Send 3 Read tool calls that will never complete
            let read_1 = acp::ToolCallId::new("stuck-read-001");
            let read_2 = acp::ToolCallId::new("stuck-read-002");
            let read_3 = acp::ToolCallId::new("stuck-read-003");

            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(read_1, "Reading app.rs")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"path": "src/app.rs"})),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(read_2, "Reading config.rs")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"path": "src/config.rs"})),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(read_3, "Reading main.rs")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"path": "src/main.rs"})),
            )
            .await?;

            sleep(Duration::from_millis(50)).await;

            // Send the agent's final text WITHOUT completing any tool calls.
            // The TUI must still render this text by finalizing the stuck cells.
            self.send_text_chunk(
                session_id.clone(),
                "Analysis complete despite incomplete tool calls.",
            )
            .await?;

            return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
        }

        // Reproduce the race condition where tool call completions arrive DURING
        // the final text stream. These get deferred into the interrupt queue, then
        // flushed after the agent's final message - causing a trailing dump of tool
        // output below the response the user needs to see.
        if std::env::var("MOCK_AGENT_TOOL_CALLS_DURING_FINAL_STREAM").is_ok() {
            eprintln!("Mock agent: sending tool calls during final text stream");

            // Phase 1: Initial exploring batch that completes BEFORE text starts.
            // These should render normally above the agent text.
            let read_1 = acp::ToolCallId::new("read-001");
            let read_2 = acp::ToolCallId::new("read-002");

            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(read_1.clone(), "Reading file1.rs")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"path": "src/file1.rs"})),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(read_2.clone(), "Reading file2.rs")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"path": "src/file2.rs"})),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    read_1.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(
                                "file1.rs read successfully",
                            )),
                        ))])
                        .raw_output(json!({"lines": 200})),
                ),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    read_2.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(
                                "file2.rs read successfully",
                            )),
                        ))])
                        .raw_output(json!({"lines": 150})),
                ),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            // Phase 2: Start streaming the final text response.
            // This activates the stream_controller, causing subsequent tool events
            // to be deferred into the interrupt queue.
            self.send_text_chunk(session_id.clone(), "Here is my analysis of the codebase.")
                .await?;

            sleep(Duration::from_millis(50)).await;

            // Phase 3: Send tool call begins + completions DURING text streaming.
            // These get deferred because stream_controller is active.
            // When on_task_complete flushes the queue, they appear AFTER the final text.
            let read_3 = acp::ToolCallId::new("read-003");
            let grep_1 = acp::ToolCallId::new("grep-001");
            let read_4 = acp::ToolCallId::new("read-004");

            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(read_3.clone(), "Reading SKILL.md")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"path": ".claude/skills/using-skills/SKILL.md"})),
            )
            .await?;

            sleep(Duration::from_millis(20)).await;

            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    read_3.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(
                                "SKILL.md read successfully",
                            )),
                        ))])
                        .raw_output(json!({"lines": 80})),
                ),
            )
            .await?;

            sleep(Duration::from_millis(20)).await;

            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(grep_1.clone(), "Searching for undefined")
                    .kind(acp::ToolKind::Search)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"pattern": "undefined"})),
            )
            .await?;

            sleep(Duration::from_millis(20)).await;

            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    grep_1.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new("Found 5 matches")),
                        ))])
                        .raw_output(json!({"matches": 5})),
                ),
            )
            .await?;

            sleep(Duration::from_millis(20)).await;

            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(read_4.clone(), "Reading config.toml")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"path": "config.toml"})),
            )
            .await?;

            sleep(Duration::from_millis(20)).await;

            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    read_4.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(
                                "config.toml read successfully",
                            )),
                        ))])
                        .raw_output(json!({"lines": 25})),
                ),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            // Phase 4: Send final text chunk. Any tool events still in the
            // interrupt queue are discarded by on_task_complete() -> clear().
            self.send_text_chunk(
                session_id.clone(),
                " Let me know if you need anything else.",
            )
            .await?;

            return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
        }

        // Support echoing an environment variable's value for testing env inheritance.
        // Set MOCK_AGENT_ECHO_ENV to the name of the env var to check.
        // The agent will respond with "ENV:<name>=<value>" or "ENV:<name>=<unset>".
        if let Ok(env_name) = std::env::var("MOCK_AGENT_ECHO_ENV") {
            let value = std::env::var(&env_name).unwrap_or_else(|_| "<unset>".to_string());
            let msg = format!("ENV:{env_name}={value}");
            self.send_text_chunk(session_id.clone(), &msg).await?;
            return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
        }

        // Support generic tool call with no raw_input to test the "skipped Begin" path.
        // When a ToolCall has a generic title and no raw_input, the ACP translation layer
        // skips emitting ExecCommandBegin and only emits ExecCommandEnd on completion.
        // This tests that the TUI correctly uses ev.command (not ev.call_id) for display.
        if std::env::var("MOCK_AGENT_GENERIC_TOOL_CALL").is_ok() {
            eprintln!("Mock agent: sending generic tool call (no raw_input)");

            let tool_call_id = acp::ToolCallId::new("toolu_generic_test_001");

            // Send ToolCall with generic title and NO raw_input
            // This will be skipped by the translator (no useful display info)
            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(tool_call_id.clone(), "Terminal")
                    .kind(acp::ToolKind::Execute)
                    .status(acp::ToolCallStatus::Pending),
            )
            .await?;

            sleep(Duration::from_millis(50)).await;

            // Send completion directly (no intermediate update with better title)
            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    tool_call_id.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new("command output here")),
                        ))])
                        .raw_output(json!({"exit_code": 0, "stdout": "command output here"})),
                ),
            )
            .await?;

            self.send_text_chunk(session_id.clone(), "Generic tool call done.")
                .await?;

            return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
        }

        // Echo back the full prompt text for verifying context injection.
        if std::env::var("MOCK_AGENT_ECHO_PROMPT").is_ok() {
            let user_text = arguments
                .prompt
                .iter()
                .filter_map(|block| match block {
                    acp::ContentBlock::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            self.send_text_chunk(session_id.clone(), &user_text).await?;
            return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
        }

        if std::env::var("MOCK_AGENT_BROWSER_MODIFY").is_ok() {
            let user_text = arguments
                .prompt
                .iter()
                .filter_map(|block| match block {
                    acp::ContentBlock::Text(t) => Some(t.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");

            if let Some(cdp_port) = browser_modify::extract_cdp_port_from_prompt(&user_text) {
                eprintln!("Mock agent: detected CDP port {cdp_port}, modifying browser");

                let new_title = "NORI_BROWSER_TEST";

                let tool_call_id = acp::ToolCallId::new("browser-modify-001");
                self.send_tool_call(
                    session_id.clone(),
                    acp::ToolCall::new(tool_call_id.clone(), "Modifying browser page title")
                        .kind(acp::ToolKind::Execute)
                        .status(acp::ToolCallStatus::InProgress)
                        .raw_input(json!({"action": "set document.title", "value": new_title})),
                )
                .await?;

                let result = std::thread::spawn(move || {
                    browser_modify::modify_browser_title(cdp_port, new_title)
                })
                .join()
                .map_err(|_| acp::Error::internal_error())?;

                match result {
                    Ok(title) => {
                        self.send_tool_call_update(
                            session_id.clone(),
                            acp::ToolCallUpdate::new(
                                tool_call_id,
                                acp::ToolCallUpdateFields::new()
                                    .status(acp::ToolCallStatus::Completed)
                                    .content(vec![acp::ToolCallContent::Content(
                                        acp::Content::new(acp::ContentBlock::Text(
                                            acp::TextContent::new(format!(
                                                "Page title set to: {title}"
                                            )),
                                        )),
                                    )]),
                            ),
                        )
                        .await?;
                        self.send_text_chunk(
                            session_id.clone(),
                            &format!("BROWSER_MODIFIED:title={title}"),
                        )
                        .await?;
                    }
                    Err(err) => {
                        self.send_tool_call_update(
                            session_id.clone(),
                            acp::ToolCallUpdate::new(
                                tool_call_id,
                                acp::ToolCallUpdateFields::new()
                                    .status(acp::ToolCallStatus::Completed)
                                    .content(vec![acp::ToolCallContent::Content(
                                        acp::Content::new(acp::ContentBlock::Text(
                                            acp::TextContent::new(format!(
                                                "Failed to modify browser: {err}"
                                            )),
                                        )),
                                    )]),
                            ),
                        )
                        .await?;
                        self.send_text_chunk(
                            session_id.clone(),
                            &format!("BROWSER_MODIFY_FAILED:{err}"),
                        )
                        .await?;
                    }
                }
                return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
            }
        }

        // Support custom response text for TUI testing
        if let Ok(response) = std::env::var("MOCK_AGENT_RESPONSE") {
            // MOCK_AGENT_RESPONSE_CHUNK_CHARS splits the response into small chunks the way a real
            // model streams it, so tests can cover incremental rendering instead of a single chunk
            // that always parses as complete markdown.
            match std::env::var("MOCK_AGENT_RESPONSE_CHUNK_CHARS")
                .ok()
                .and_then(|chars| chars.parse::<usize>().ok())
                .filter(|chars| *chars > 0)
            {
                Some(chunk_chars) => {
                    let characters = response.chars().collect::<Vec<_>>();
                    for chunk in characters.chunks(chunk_chars) {
                        self.send_text_chunk(session_id.clone(), &chunk.iter().collect::<String>())
                            .await?;
                    }
                }
                None => {
                    self.send_text_chunk(session_id.clone(), &response).await?;
                }
            }
        } else {
            // Default behavior
            self.send_text_chunk(session_id.clone(), "Test message 1")
                .await?;

            self.send_text_chunk(session_id.clone(), "Test message 2")
                .await?;
        }

        // Support configurable delay for simulating realistic streaming
        if let Ok(delay_str) = std::env::var("MOCK_AGENT_DELAY_MS")
            && let Ok(delay) = delay_str.parse::<u64>()
        {
            sleep(Duration::from_millis(delay)).await;
        }

        // Support requesting permission from client for testing approval bridging
        if request_permission || std::env::var("MOCK_AGENT_REQUEST_PERMISSION").is_ok() {
            eprintln!("Mock agent: requesting permission from client");

            // Create a tool call update describing the operation
            let tool_call_id = acp::ToolCallId::new("permission-test-001");
            let tool_call = acp::ToolCallUpdate::new(
                tool_call_id,
                acp::ToolCallUpdateFields::new()
                    .title("Execute shell command")
                    .kind(acp::ToolKind::Execute)
                    .status(acp::ToolCallStatus::Pending)
                    .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                        acp::ContentBlock::Text(acp::TextContent::new(
                            "echo 'Hello from permission test'",
                        )),
                    ))])
                    .raw_input(json!({"command": "echo", "args": ["Hello from permission test"]})),
            );

            // Create permission options: allow once and reject once
            let options = vec![
                acp::PermissionOption::new(
                    acp::PermissionOptionId::new("allow"),
                    "Allow",
                    acp::PermissionOptionKind::AllowOnce,
                ),
                acp::PermissionOption::new(
                    acp::PermissionOptionId::new("reject"),
                    "Reject",
                    acp::PermissionOptionKind::RejectOnce,
                ),
            ];

            // Request permission from client
            match self
                .request_permission_via_client(session_id.clone(), tool_call, options)
                .await
            {
                Ok(response) => {
                    eprintln!(
                        "Mock agent: permission response received: {:?}",
                        response.outcome
                    );
                    match response.outcome {
                        acp::RequestPermissionOutcome::Selected(selected) => {
                            let msg =
                                format!("Permission granted with option: {}", selected.option_id);
                            self.send_text_chunk(session_id.clone(), &msg).await?;
                        }
                        _ => {
                            // Handles Cancelled and any future variants
                            self.send_text_chunk(
                                session_id.clone(),
                                "Permission request was cancelled",
                            )
                            .await?;
                        }
                    }
                }
                Err(err) => {
                    eprintln!("Mock agent: permission request failed: {err}");
                    self.send_text_chunk(session_id.clone(), "Permission request failed")
                        .await?;
                }
            }
        }

        // Support interleaved text and tool calls to test for duplicate message bug
        // This sends text DURING the tool call, which should trigger the bug where
        // the incomplete ExecCell gets flushed to history, creating duplicates.
        if std::env::var("MOCK_AGENT_INTERLEAVED_TOOL_CALL").is_ok() {
            eprintln!("Mock agent: sending interleaved tool call sequence");

            let tool_call_id = acp::ToolCallId::new("interleaved-tool-001");

            // Step 1: Send tool call (begin)
            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(tool_call_id.clone(), "Executing interleaved command")
                    .kind(acp::ToolKind::Execute)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"command": "test"})),
            )
            .await?;

            // Small delay to ensure the begin event is processed
            sleep(Duration::from_millis(50)).await;

            // Step 2: Send text DURING the tool call - this triggers the bug!
            // When this text arrives, handle_streaming_delta calls flush_active_cell()
            // which moves the incomplete ExecCell to history.
            self.send_text_chunk(session_id.clone(), "Processing command...")
                .await?;

            // Small delay
            sleep(Duration::from_millis(50)).await;

            // Step 3: Send tool call completion
            // At this point, the ExecCell is no longer in active_cell, so a new one
            // will be created, resulting in duplicate entries.
            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    tool_call_id.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new("Command completed")),
                        ))])
                        .raw_output(json!({"exit_code": 0})),
                ),
            )
            .await?;

            // Final text
            self.send_text_chunk(session_id.clone(), "Interleaved test done.")
                .await?;
        }

        // Support sending tool calls for testing ACP tool call display
        if std::env::var("MOCK_AGENT_SEND_TOOL_CALL").is_ok() {
            eprintln!("Mock agent: sending tool call sequence");

            // Send initial tool call with pending status
            let tool_call_id = acp::ToolCallId::new("test-tool-call-001");
            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(tool_call_id.clone(), "Reading configuration file")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"path": "/etc/config.toml"})),
            )
            .await?;

            // Small delay to simulate execution time
            sleep(Duration::from_millis(50)).await;

            // Send update to in_progress
            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    tool_call_id.clone(),
                    acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::InProgress),
                ),
            )
            .await?;

            // Small delay
            sleep(Duration::from_millis(50)).await;

            // Send update to completed with content
            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    tool_call_id.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(
                                "Configuration loaded successfully",
                            )),
                        ))])
                        .raw_output(json!({"success": true, "lines": 42})),
                ),
            )
            .await?;

            // Send text message after tool call
            self.send_text_chunk(session_id.clone(), "Tool call completed successfully.")
                .await?;
        }

        if let Ok(file_path) = std::env::var("MOCK_AGENT_REQUEST_FILE") {
            eprintln!("Mock agent: requesting file read: {file_path}");
            match self
                .read_file_via_client(session_id.clone(), PathBuf::from(&file_path))
                .await
            {
                Ok(content) => {
                    let msg = format!("Read file content: {content}");
                    self.send_text_chunk(session_id.clone(), &msg).await?;
                }
                Err(err) => {
                    self.send_text_chunk(
                        session_id.clone(),
                        "Failed to read file content via client",
                    )
                    .await?;
                    return Err(err);
                }
            }
        }

        // Support writing files via fs/write_text_file for testing file write implementation
        if let Ok(file_path) = std::env::var("MOCK_AGENT_WRITE_FILE") {
            let content = std::env::var("MOCK_AGENT_WRITE_CONTENT")
                .unwrap_or_else(|_| "default content".to_string());
            eprintln!(
                "Mock agent: requesting file write: {} with {} bytes",
                file_path,
                content.len()
            );
            match self
                .write_file_via_client(session_id.clone(), PathBuf::from(&file_path), content)
                .await
            {
                Ok(()) => {
                    self.send_text_chunk(session_id.clone(), "\nFile written successfully\n")
                        .await?;

                    // Optionally read back the file to verify the write
                    if let Ok(read_content) = self
                        .read_file_via_client(session_id.clone(), PathBuf::from(&file_path))
                        .await
                    {
                        let msg = format!("\nVerified content:\n{read_content}\n");
                        self.send_text_chunk(session_id.clone(), &msg).await?;
                    }
                }
                Err(err) => {
                    let msg = format!("\nFailed to write file: {err}\n");
                    self.send_text_chunk(session_id.clone(), &msg).await?;
                }
            }
        }

        if std::env::var("MOCK_AGENT_STREAM_UNTIL_CANCEL").is_ok() {
            let mut iterations = 0usize;
            while !self.cancel_requested() && iterations < 10_000 {
                self.send_text_chunk(session_id.clone(), "Streaming...")
                    .await?;
                iterations += 1;
                sleep(Duration::from_millis(10)).await;
            }

            return Ok(acp::PromptResponse::new(if self.cancel_requested() {
                acp::StopReason::Cancelled
            } else {
                acp::StopReason::EndTurn
            }));
        }

        // Support multi-call exploring cells with out-of-order completion
        // This tests the scenario where:
        // 1. Multiple Read tool calls are sent (exploring operations)
        // 2. Text streams DURING execution (triggers flush of incomplete ExecCell)
        // 3. Completion events arrive out-of-order (call-2 before call-1)
        if std::env::var("MOCK_AGENT_MULTI_CALL_EXPLORING").is_ok() {
            eprintln!("Mock agent: sending multi-call exploring sequence");

            // Send three Read tool calls
            let call_1 = acp::ToolCallId::new("read-call-001");
            let call_2 = acp::ToolCallId::new("read-call-002");
            let call_3 = acp::ToolCallId::new("read-call-003");

            // Send ToolCall 1 (Read file1.rs)
            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(call_1.clone(), "Reading file1.rs")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"path": "src/file1.rs"})),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            // Send ToolCall 2 (Read file2.rs)
            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(call_2.clone(), "Reading file2.rs")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"path": "src/file2.rs"})),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            // Send text DURING the tool calls - this triggers flush of incomplete ExecCell!
            self.send_text_chunk(session_id.clone(), "Reading multiple files...")
                .await?;

            sleep(Duration::from_millis(30)).await;

            // Send ToolCall 3 (Read file3.rs)
            self.send_tool_call(
                session_id.clone(),
                acp::ToolCall::new(call_3.clone(), "Reading file3.rs")
                    .kind(acp::ToolKind::Read)
                    .status(acp::ToolCallStatus::Pending)
                    .raw_input(json!({"path": "src/file3.rs"})),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            // Complete calls OUT OF ORDER: call-2, then call-3, then call-1
            // This tests that the cell can be retrieved by any pending call_id

            // Complete call-2 first (not the first call!)
            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    call_2.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(
                                "file2.rs read successfully",
                            )),
                        ))])
                        .raw_output(json!({"lines": 100})),
                ),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            // Complete call-3
            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    call_3.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(
                                "file3.rs read successfully",
                            )),
                        ))])
                        .raw_output(json!({"lines": 75})),
                ),
            )
            .await?;

            sleep(Duration::from_millis(30)).await;

            // Complete call-1 last
            self.send_tool_call_update(
                session_id.clone(),
                acp::ToolCallUpdate::new(
                    call_1.clone(),
                    acp::ToolCallUpdateFields::new()
                        .status(acp::ToolCallStatus::Completed)
                        .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                            acp::ContentBlock::Text(acp::TextContent::new(
                                "file1.rs read successfully",
                            )),
                        ))])
                        .raw_output(json!({"lines": 150})),
                ),
            )
            .await?;

            // Final text (unless suppressed for testing)
            if std::env::var("MOCK_AGENT_NO_FINAL_TEXT").is_err() {
                self.send_text_chunk(session_id.clone(), "Multi-call exploring done.")
                    .await?;
            }

            return Ok(acp::PromptResponse::new(acp::StopReason::EndTurn));
        }

        Ok(acp::PromptResponse::new(acp::StopReason::EndTurn))
    }
}

async fn initialize_advertised_nori_client(
    mcp_servers: &[acp::McpServer],
) -> Result<(), Box<dyn std::error::Error>> {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    let Some(http) = mcp_servers.iter().find_map(|server| match server {
        acp::McpServer::Http(http) if http.name == "nori-client" => Some(http),
        _ => None,
    }) else {
        return Ok(());
    };
    let authorization = http
        .headers
        .iter()
        .find(|header| header.name == "Authorization")
        .map(|header| format!("Authorization: {}\r\n", header.value))
        .unwrap_or_default();
    let without_scheme = http
        .url
        .strip_prefix("http://")
        .ok_or("nori-client URL should use plain HTTP")?;
    let (host_port, path) = without_scheme
        .split_once('/')
        .ok_or("nori-client URL should include a path")?;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "mock-acp-agent", "version": "0" }
        }
    })
    .to_string();
    let request = format!(
        "POST /{path} HTTP/1.1\r\n\
Host: {host_port}\r\n\
Accept: application/json, text/event-stream\r\n\
Content-Type: application/json\r\n\
{authorization}\
Content-Length: {}\r\n\
Connection: close\r\n\r\n\
{body}",
        body.len()
    );
    let mut stream = tokio::net::TcpStream::connect(host_port).await?;
    stream.write_all(request.as_bytes()).await?;
    let mut response = String::new();
    stream.read_to_string(&mut response).await?;
    if !response.contains("200 OK") {
        return Err(format!("nori-client initialize failed: {response}").into());
    }
    Ok(())
}

#[tokio::main]
async fn main() -> acp::Result<()> {
    env_logger::init();

    let state = Arc::new(MockState::default());

    Agent
        .builder()
        .name("mock-agent")
        .on_receive_request(
            async move |arguments: acp::InitializeRequest,
                        responder: Responder<acp::InitializeResponse>,
                        _cx: ConnectionTo<Client>| {
                if std::env::var("MOCK_AGENT_HANG").is_ok() {
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                }

                // Support configurable startup delay for testing "Connecting" status.
                // Check for model-specific delay first (e.g.
                // MOCK_AGENT_STARTUP_DELAY_MS_MOCK_MODEL_ALT), then fall back to
                // generic MOCK_AGENT_STARTUP_DELAY_MS.
                let model_name = std::env::var("MOCK_AGENT_MODEL_NAME").unwrap_or_default();
                let model_specific_var = format!(
                    "MOCK_AGENT_STARTUP_DELAY_MS_{}",
                    model_name.replace('-', "_").to_uppercase()
                );
                let delay_ms = std::env::var(&model_specific_var)
                    .or_else(|_| std::env::var("MOCK_AGENT_STARTUP_DELAY_MS"))
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok());

                if let Some(delay) = delay_ms {
                    eprintln!("Mock agent ({model_name}): sleeping for {delay}ms during startup");
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
                }

                // Simulate authentication failure if requested
                if std::env::var("MOCK_AGENT_REQUIRE_AUTH").is_ok() {
                    eprintln!("Mock agent: simulating authentication failure");
                    return responder
                        .respond_with_error(acp::Error::new(-32000, "Authentication required"));
                }
                if std::env::var("MOCK_AGENT_INITIALIZE_FAIL").is_ok() {
                    eprintln!("Mock agent: simulating initialize failure");
                    return responder.respond_with_error(acp::Error::new(
                        -32004,
                        "Mock initialize failure for testing",
                    ));
                }

                eprintln!("Mock agent: initialize");
                let mut response = acp::InitializeResponse::new(arguments.protocol_version)
                    .agent_info(acp::Implementation::new("mock-agent", "0.1.0").title("Mock Agent"));

                let mut capabilities = acp::AgentCapabilities::new();
                let mut has_capabilities = false;

                if std::env::var("MOCK_AGENT_SUPPORT_LOAD_SESSION").is_ok() {
                    eprintln!("Mock agent: advertising load_session capability");
                    capabilities = capabilities.load_session(true);
                    has_capabilities = true;
                }

                if std::env::var("MOCK_AGENT_MCP_HTTP").is_ok() {
                    eprintln!("Mock agent: advertising HTTP MCP capability");
                    capabilities =
                        capabilities.mcp_capabilities(acp::McpCapabilities::new().http(true));
                    has_capabilities = true;
                }

                let mut session_capabilities = acp::SessionCapabilities::new();
                let mut has_session_capabilities = false;

                if std::env::var("MOCK_AGENT_SUPPORT_SESSION_LIST").is_ok() {
                    eprintln!("Mock agent: advertising session/list capability");
                    session_capabilities =
                        session_capabilities.list(acp::SessionListCapabilities::new());
                    has_session_capabilities = true;
                }

                if std::env::var("MOCK_AGENT_SUPPORT_SESSION_RESUME").is_ok() {
                    eprintln!("Mock agent: advertising session/resume capability");
                    session_capabilities =
                        session_capabilities.resume(acp::SessionResumeCapabilities::new());
                    has_session_capabilities = true;
                }

                if std::env::var("MOCK_AGENT_SUPPORT_SESSION_CLOSE").is_ok() {
                    eprintln!("Mock agent: advertising session/close capability");
                    session_capabilities =
                        session_capabilities.close(acp::SessionCloseCapabilities::new());
                    has_session_capabilities = true;
                }

                if std::env::var("MOCK_AGENT_SUPPORT_SESSION_FORK").is_ok() {
                    eprintln!("Mock agent: advertising session/fork capability");
                    session_capabilities =
                        session_capabilities.fork(acp::SessionForkCapabilities::new());
                    has_session_capabilities = true;
                }

                if has_session_capabilities {
                    capabilities = capabilities.session_capabilities(session_capabilities);
                    has_capabilities = true;
                }

                if has_capabilities {
                    response = response.agent_capabilities(capabilities);
                }

                if std::env::var("MOCK_AGENT_GOAL_EXT").is_ok() {
                    eprintln!("Mock agent: advertising the _session/goal goal extension");
                    let mut meta = serde_json::Map::new();
                    meta.insert(
                        "goal".to_string(),
                        serde_json::json!({
                            "version": 1,
                            "controlMethod": "_session/goal",
                            "actions": ["set", "pause", "resume", "clear"],
                        }),
                    );
                    response.meta = Some(meta);
                }

                responder.respond(response)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_arguments: acp::AuthenticateRequest,
                        responder: Responder<acp::AuthenticateResponse>,
                        _cx: ConnectionTo<Client>| {
                responder.respond(acp::AuthenticateResponse::default())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |arguments: acp::NewSessionRequest,
                            responder: Responder<acp::NewSessionResponse>,
                            cx: ConnectionTo<Client>| {
                    let session_id = state.next_session_id.fetch_add(1, Ordering::SeqCst);
                    if let Ok(fail_from) = std::env::var("MOCK_AGENT_FAIL_NEW_SESSION_FROM")
                        && let Ok(fail_from) = fail_from.parse::<i64>()
                        && session_id >= fail_from
                    {
                        eprintln!("Mock agent: simulating new_session failure at id={session_id}");
                        return responder.respond_with_error(acp::Error::new(
                            -32002,
                            "Mock new_session failure for testing",
                        ));
                    }
                    eprintln!("Mock agent: new_session id={session_id}");

                    let session_key = session_id.to_string();
                    let session_config = default_session_config();
                    state
                        .session_configs
                        .lock()
                        .unwrap()
                        .insert(session_key.clone(), session_config.clone());
                    if std::env::var("MOCK_AGENT_NEW_SESSION_NOTIFICATION").is_ok() {
                        MockAgent {
                            cx: cx.clone(),
                            state: state.clone(),
                        }
                        .send_text_chunk(
                            acp::SessionId::new(session_key.clone()),
                            "new session setup notification",
                        )
                        .await?;
                    }
                    if std::env::var("MOCK_AGENT_INITIALIZE_NORI_CLIENT_DURING_NEW_SESSION").is_ok()
                        && let Err(error) =
                            initialize_advertised_nori_client(&arguments.mcp_servers).await
                    {
                        eprintln!(
                            "Mock agent: failed to initialize advertised nori-client MCP server: {error}"
                        );
                    }

                    responder.respond(
                        acp::NewSessionResponse::new(acp::SessionId::new(session_key))
                            .config_options(config_options_for_state(&session_config)),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |arguments: acp::LoadSessionRequest,
                            responder: Responder<acp::LoadSessionResponse>,
                            cx: ConnectionTo<Client>| {
                    let session_config = state
                        .session_configs
                        .lock()
                        .unwrap()
                        .entry(arguments.session_id.to_string())
                        .or_insert_with(default_session_config)
                        .clone();

                    // Send configurable number of notifications during load_session
                    // to simulate history replay. Uses the session_id from the request
                    // so notifications are routed to the correct update channel.
                    if let Ok(count_str) = std::env::var("MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT")
                        && let Ok(count) = count_str.parse::<i64>()
                    {
                        let agent = MockAgent {
                            cx: cx.clone(),
                            state: state.clone(),
                        };
                        let session_id = arguments.session_id.clone();
                        eprintln!("Mock agent: sending {count} notifications during load_session");
                        for i in 0..count {
                            agent
                                .send_text_chunk(session_id.clone(), &format!("replay chunk {i}"))
                                .await?;
                        }
                    }

                    if std::env::var("MOCK_AGENT_LOAD_SESSION_FAIL").is_ok() {
                        eprintln!("Mock agent: simulating load_session failure");
                        return responder.respond_with_error(acp::Error::new(
                            -32001,
                            "Mock load_session failure for testing",
                        ));
                    }

                    responder.respond(
                        acp::LoadSessionResponse::new()
                            .config_options(config_options_for_state(&session_config)),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |arguments: acp::ResumeSessionRequest,
                            responder: Responder<acp::ResumeSessionResponse>,
                            _cx: ConnectionTo<Client>| {
                    if std::env::var("MOCK_AGENT_RESUME_SESSION_FAIL").is_ok() {
                        eprintln!("Mock agent: simulating resume_session failure");
                        return responder.respond_with_error(
                            acp::Error::new(-32002, "session not found").data(serde_json::json!({
                                "detail": "the session is no longer claimed"
                            })),
                        );
                    }
                    eprintln!("Mock agent: resume_session id={}", arguments.session_id);
                    let session_config = state
                        .session_configs
                        .lock()
                        .unwrap()
                        .entry(arguments.session_id.to_string())
                        .or_insert_with(default_session_config)
                        .clone();
                    responder.respond(
                        acp::ResumeSessionResponse::new()
                            .config_options(config_options_for_state(&session_config)),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |arguments: acp::CloseSessionRequest,
                        responder: Responder<acp::CloseSessionResponse>,
                        _cx: ConnectionTo<Client>| {
                if std::env::var("MOCK_AGENT_CLOSE_SESSION_FAIL").is_ok() {
                    eprintln!("Mock agent: simulating close_session failure");
                    return responder
                        .respond_with_error(acp::Error::new(-32002, "session not found"));
                }
                eprintln!("Mock agent: close_session id={}", arguments.session_id);
                responder.respond(acp::CloseSessionResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |arguments: acp::ForkSessionRequest,
                            responder: Responder<acp::ForkSessionResponse>,
                            _cx: ConnectionTo<Client>| {
                    if std::env::var("MOCK_AGENT_FORK_SESSION_FAIL").is_ok() {
                        eprintln!("Mock agent: simulating fork_session failure");
                        return responder
                            .respond_with_error(acp::Error::new(-32002, "session not found"));
                    }
                    let forked_id = state.next_session_id.fetch_add(1, Ordering::SeqCst);
                    eprintln!(
                        "Mock agent: fork_session from={} new_id={forked_id}",
                        arguments.session_id
                    );
                    let forked_key = forked_id.to_string();
                    let session_config = state
                        .session_configs
                        .lock()
                        .unwrap()
                        .entry(arguments.session_id.to_string())
                        .or_insert_with(default_session_config)
                        .clone();
                    state
                        .session_configs
                        .lock()
                        .unwrap()
                        .insert(forked_key.clone(), session_config.clone());
                    responder.respond(
                        acp::ForkSessionResponse::new(acp::SessionId::new(forked_key))
                            .config_options(config_options_for_state(&session_config)),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |arguments: acp::ListSessionsRequest,
                        responder: Responder<acp::ListSessionsResponse>,
                        _cx: ConnectionTo<Client>| {
                if std::env::var("MOCK_AGENT_LIST_SESSIONS_FAIL").is_ok() {
                    eprintln!("Mock agent: simulating list_sessions failure");
                    return responder
                        .respond_with_error(acp::Error::new(-32010, "broker unreachable"));
                }
                eprintln!("Mock agent: session/list");
                let cwd = arguments
                    .cwd
                    .unwrap_or_else(|| PathBuf::from("/mock/session/cwd"));
                let mut first = acp::SessionInfo::new("mock-session-1", cwd.clone())
                    .title("First mock session")
                    .updated_at("2026-01-02T03:04:05Z");
                // MOCK_AGENT_LIST_SESSIONS_META, when set to a JSON object,
                // is attached as the `_meta` extension on the first session
                // row so tests can exercise the real `_meta` wire path.
                if let Ok(meta_json) = std::env::var("MOCK_AGENT_LIST_SESSIONS_META")
                    && let Ok(serde_json::Value::Object(meta)) =
                        serde_json::from_str::<serde_json::Value>(&meta_json)
                {
                    first = first.meta(meta);
                }
                let sessions = vec![first, acp::SessionInfo::new("mock-session-2", cwd)];
                responder.respond(acp::ListSessionsResponse::new(sessions))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |arguments: acp::PromptRequest,
                            responder: Responder<acp::PromptResponse>,
                            cx: ConnectionTo<Client>| {
                    // Run the prompt work in a spawned task so the dispatch loop
                    // stays free to deliver cancel notifications and client
                    // request responses while the agent streams.
                    let agent = MockAgent {
                        cx: cx.clone(),
                        state: state.clone(),
                    };
                    cx.spawn(async move {
                        let result = agent.run_prompt(arguments).await;
                        responder.respond_with_result(result)
                    })?;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |_arguments: acp::SetSessionModeRequest,
                        responder: Responder<acp::SetSessionModeResponse>,
                        _cx: ConnectionTo<Client>| {
                responder.respond(acp::SetSessionModeResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let state = state.clone();
                async move |arguments: acp::SetSessionConfigOptionRequest,
                            responder: Responder<acp::SetSessionConfigOptionResponse>,
                            _cx: ConnectionTo<Client>| {
                    let Some(value) = arguments.value.as_value_id().map(ToString::to_string) else {
                        return responder.respond_with_error(acp::Error::invalid_params());
                    };

                    let result = {
                        let mut sessions = state.session_configs.lock().unwrap();
                        let cfg = sessions
                            .entry(arguments.session_id.to_string())
                            .or_insert_with(default_session_config);

                        match arguments.config_id.to_string().as_str() {
                            "model" => {
                                cfg.model_id = value;
                                if cfg.model_id == "mock-model-fast" {
                                    cfg.thought_level = None;
                                    cfg.speed = Some("fast".to_string());
                                } else {
                                    cfg.speed = None;
                                    if cfg.thought_level.is_none() {
                                        cfg.thought_level = Some("medium".to_string());
                                    }
                                }
                                Ok(config_options_for_state(cfg))
                            }
                            "thought_level" => {
                                cfg.thought_level = Some(value);
                                Ok(config_options_for_state(cfg))
                            }
                            "speed" => {
                                cfg.speed = Some(value);
                                Ok(config_options_for_state(cfg))
                            }
                            _ => Err(acp::Error::invalid_params()),
                        }
                    };

                    match result {
                        Ok(options) => responder
                            .respond(acp::SetSessionConfigOptionResponse::new(options)),
                        Err(error) => responder.respond_with_error(error),
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                // Native goal store for the `_session/goal` extension. Matches
                // any method (untyped fallback), so it must stay registered
                // after every typed request handler.
                let goal_objective: Arc<std::sync::Mutex<Option<String>>> =
                    Arc::new(std::sync::Mutex::new(None));
                async move |arguments: agent_client_protocol::UntypedMessage,
                            responder: Responder<serde_json::Value>,
                            cx: ConnectionTo<Client>| {
                    // The crate strips the leading underscore from ext methods
                    // on some parse paths; accept both spellings.
                    let is_goal_method =
                        matches!(arguments.method(), "_session/goal" | "session/goal");
                    if !is_goal_method || std::env::var("MOCK_AGENT_GOAL_EXT").is_err() {
                        return responder.respond_with_error(acp::Error::method_not_found());
                    }
                    let params = arguments.params.clone();
                    let session_id = params
                        .get("sessionId")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let action = params
                        .get("action")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    eprintln!("Mock agent: _session/goal action={action}");
                    if action == "set" {
                        *goal_objective.lock().unwrap() = params
                            .get("objective")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string);
                    }
                    let objective = goal_objective.lock().unwrap().clone().unwrap_or_default();
                    let goal_value = match action.as_str() {
                        "set" | "resume" => {
                            serde_json::json!({ "objective": objective, "status": "active" })
                        }
                        "pause" => {
                            serde_json::json!({ "objective": objective, "status": "paused" })
                        }
                        "clear" => {
                            goal_objective.lock().unwrap().take();
                            serde_json::Value::Null
                        }
                        _ => return responder.respond_with_error(acp::Error::invalid_params()),
                    };
                    let snapshot_update = |goal: serde_json::Value| {
                        let mut meta = serde_json::Map::new();
                        meta.insert("goal".to_string(), goal);
                        let mut info = acp::SessionInfoUpdate::new();
                        info.meta = Some(meta);
                        acp::SessionUpdate::SessionInfoUpdate(info)
                    };
                    let _ = cx.send_notification(acp::SessionNotification::new(
                        acp::SessionId::new(session_id.clone()),
                        snapshot_update(goal_value),
                    ));
                    if action == "set" && std::env::var("MOCK_AGENT_GOAL_EXT_AUTOCOMPLETE").is_ok()
                    {
                        eprintln!("Mock agent: auto-completing goal");
                        let _ = cx.send_notification(acp::SessionNotification::new(
                            acp::SessionId::new(session_id),
                            snapshot_update(
                                serde_json::json!({ "objective": objective, "status": "complete" }),
                            ),
                        ));
                    }
                    responder.respond(serde_json::json!({}))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            {
                let state = state.clone();
                async move |_notification: acp::CancelNotification, _cx: ConnectionTo<Client>| {
                    eprintln!("Mock agent: cancel");
                    if std::env::var("MOCK_AGENT_IGNORE_CANCEL").is_err() {
                        state.cancel_requested.store(true, Ordering::SeqCst);
                    } else {
                        eprintln!("Mock agent: ignoring cancel (MOCK_AGENT_IGNORE_CANCEL is set)");
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(ByteStreams::new(
            tokio::io::stdout().compat_write(),
            ExitOnEof::new(tokio::io::stdin().compat()),
        ))
        .await
}
