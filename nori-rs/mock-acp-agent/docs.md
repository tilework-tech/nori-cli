# Noridoc: mock-acp-agent

Path: @/nori-rs/mock-acp-agent

### Overview

The mock-acp-agent crate provides a mock ACP agent binary for testing the Nori TUI. It simulates an AI agent's behavior including streaming responses, tool calls, and permission requests.

### How it fits into the larger codebase

Used by `@/nori-rs/tui-pty-e2e/` for end-to-end integration testing. The mock agent is spawned as a subprocess and communicates over stdin/stdout using the ACP protocol. It is built on the same official `agent-client-protocol` SDK (0.15.1) that `AcpConnection` in `@/nori-rs/acp-host/src/connection/acp_connection.rs` uses on the client side, so the mock exercises the real SDK wire path that production agents traverse.

### Core Implementation

**Agent wiring**: The mock is assembled in `main()` from the SDK's `Agent.builder()` with typed `on_receive_request`/`on_receive_notification` handler closures -- one per method (`initialize`, `authenticate`, `session/new`, `session/load`, `session/resume`, `session/close`, `session/list`, `session/prompt`, `session/set_mode`, `session/set_config_option`, plus the `session/cancel` notification) -- then connected over `ByteStreams` to the wrapped stdin/stdout. Each closure receives the typed request/notification, a `Responder`, and a `ConnectionTo<Client>`; the closures share state through an `Arc<MockState>`. Handled concerns include:
- Session creation, load (history replay), and config-option/mode mutation
- Prompt processing with simulated responses, streaming, and tool calls
- Permission request/response flow
- Cancellation

**Mock Behaviors**: Controlled via environment variables that the E2E tests set on the mock agent process. Each env var activates a specific behavior scenario. Key scenarios include multi-turn conversations, tool call streaming, permission requests, file operations, race condition simulations, and session lifecycle behaviors.

**Session Lifecycle Testing**: Several env vars control `session/load`, `session/resume`, and `session/close` behavior for testing the resume/close paths in `@/nori-rs/harness/src/backend/session.rs`:
- `MOCK_AGENT_SUPPORT_LOAD_SESSION` -- when set, the agent advertises `load_session: true` in its capabilities during `initialize()`
- `MOCK_AGENT_SUPPORT_SESSION_LIST` -- when set, the agent advertises the ACP `session/list` capability during `initialize()` and its `session/list` handler returns two canned `SessionInfo` rows; exercises the agent-sourced `/resume` picker wire path (`AcpConnection::list_sessions()` in `@/nori-rs/acp-host/src/connection/`, surfaced as `agent.session_list`)
- `MOCK_AGENT_LIST_SESSIONS_META` -- when set to a JSON object string, the object is attached as the ACP `_meta` extension on the first `session/list` row (e.g. `{"nori":{"origin":"cloud"}}`), exercising the full wire path for `AcpSessionSummary.meta` (agent -> `session/list` -> `AcpConnection::list_sessions()` -> resume picker's cloud-origin detection, see `@/nori-rs/acp-host/src/connection/docs.md`)
- `MOCK_AGENT_SUPPORT_SESSION_RESUME` -- when set, the agent advertises the ACP `session/resume` capability and its handler reattaches to the requested id, returning `config_options`; exercises the live-reattach resume path (`AcpConnection::resume_session()`), the branch the nori cloud agent uses (`resume` without `loadSession`)
- `MOCK_AGENT_SUPPORT_SESSION_CLOSE` -- when set, the agent advertises the ACP `session/close` capability and its handler acknowledges the close; exercises `AcpConnection::close_session()` and the `/close` command path
- `MOCK_AGENT_RESUME_SESSION_FAIL` -- when set, the `session/resume` handler returns a structured `-32002` error with `data.detail` "the session is no longer claimed", exercising `categorize_acp_error_chain()` in `@/nori-rs/acp-host/src/error_category.rs` (SessionNotFound plus detail extraction over the real stdio transport)
- `MOCK_AGENT_CLOSE_SESSION_FAIL` -- when set, the `session/close` handler returns a structured `-32002` error, proving close failures propagate across the process boundary
- `MOCK_AGENT_MCP_HTTP` -- when set, the agent advertises HTTP MCP capability so the backend-owned `nori-client` MCP server in `@/nori-rs/harness/src/backend/nori_client_mcp.rs` can be tested through the normal `session/new` MCP server advertisement path
- `MOCK_AGENT_INITIALIZE_NORI_CLIENT_DURING_NEW_SESSION` -- when set, `new_session()` eagerly sends an MCP `initialize` request to the advertised `nori-client` server before returning, mirroring agents that initialize advertised MCP servers during session setup
- `MOCK_AGENT_FAIL_NEW_SESSION_FROM` -- when set to an integer N, `new_session()` returns an error once the generated session id is at least N, allowing tests to exercise replacement-session failures without breaking the initial backend startup; `0` fails every `session/new`, which the picker-first cloud tests in `@/nori-rs/tui-pty-e2e/tests/cloud_mode.rs` use to turn any premature session claim into a loud failure
- `MOCK_AGENT_LOAD_SESSION_FAIL` -- when set, the `load_session()` handler returns an error instead of succeeding, allowing tests to exercise the runtime-failure fallback path
- `MOCK_AGENT_LOAD_SESSION_NOTIFICATION_COUNT` -- when set to an integer N, the `load_session()` handler sends N text-chunk notifications (via `send_text_chunk()`) before returning, simulating history replay with a configurable volume of events. Used to test the deferred-relay pattern in `resume_session()` that prevents deadlocks when the notification count exceeds the bounded `event_tx` channel capacity.

**Environment Variable Echo**: The `MOCK_AGENT_ECHO_ENV` env var causes the mock agent's `prompt()` handler to respond with `ENV:<name>=<value>` (or `ENV:<name>=<unset>` if the variable is absent). Used by `test_codex_home_not_inherited_by_agent_subprocess` in `@/nori-rs/acp-host/src/connection/acp_connection_tests.rs` to verify that the parent's `CODEX_HOME` is not inherited by the spawned ACP subprocess.

**Prompt Echo**: The `MOCK_AGENT_ECHO_PROMPT` env var causes the mock agent's `prompt()` handler to echo back the full prompt text it received. Used by session context tests in `@/nori-rs/harness/src/backend/tests/part5.rs` to verify that `AcpBackendConfig.session_context` is correctly prepended to the first user prompt and consumed after that.

**Cancel Ignore**: The `MOCK_AGENT_IGNORE_CANCEL` env var causes the mock agent's `cancel()` handler to silently discard the cancellation instead of setting `cancel_requested`. Used with `MOCK_AGENT_STREAM_UNTIL_CANCEL` by `@/nori-rs/harness/src/backend/tests/part4.rs` to test the cancel timeout watchdog in `session_runtime_driver.rs` -- verifying that the backend force-cancels the prompt after the timeout even when the agent is unresponsive to `CancelNotification`.

**Structured Prompt Failure**: `MOCK_AGENT_PROMPT_FAIL_JSON` makes `prompt()` return a structured `acp::Error` (JSON-RPC code `-32010`) whose `data` carries both a `detail` string and unrelated noise fields (`retry_after_ms`, `trace_id`); `MOCK_AGENT_PROMPT_FAIL_JSON_NO_DETAIL` returns the same shape with no `detail` field. Used by `@/nori-rs/harness/src/backend/tests/part10.rs` to verify `send_prompt_error` in `backend/session_runtime_driver.rs` surfaces the clean top-level message (plus `detail` when present) instead of the raw pretty-printed `data` JSON blob.

**Session Config Options**: The mock agent advertises live ACP session config options on `session/new` and `session/load`, and supports `session/set_config_option` for connection/TUI tests. The default config exposes `Model` plus `Thought Level`. Switching the model to `mock-model-fast` replaces `Thought Level` with a `Speed` selector, which lets tests verify that Nori replaces the full live config snapshot after a config mutation.

**Cancel Tail Ordering**: The `MOCK_AGENT_CANCEL_TAIL_EMPTY_END_TURNS` env var reproduces the Claude-style cancel tail that motivated the ACP cancellation-ordering fix. When a streaming prompt is cancelled, the mock agent queues N immediate empty `end_turn` responses for the next prompt attempts before finally allowing the real follow-up prompt to complete. `MOCK_AGENT_CANCEL_TAIL_FOLLOW_UP_RESPONSE` overrides the text returned by that eventual real follow-up turn. These knobs are used by `@/nori-rs/acp-host/src/connection/acp_connection_tests.rs` and `@/nori-rs/tui-pty-e2e/tests/streaming.rs` to verify that Nori absorbs repeated stale terminal responses without admitting a new logical prompt turn too early.

**Stuck Tool Calls (No Completion)**: The `MOCK_AGENT_STUCK_TOOL_CALLS` env var triggers a scenario where 3 Read tool calls are sent with `Pending` status but never receive completion updates. After a short delay the agent sends its final text response and ends the turn. This reproduces the frozen-display bug where incomplete ExecCells fill the viewport and block `insert_history_lines()` from rendering the agent's text. The fix under test is `finalize_active_cell_as_failed()` in `@/nori-rs/tui/src/chatwidget.rs`.

**Runaway Search Snapshot Amplification**: The `MOCK_AGENT_RUNAWAY_SEARCH` env var triggers a deterministic Search tool-call stream that repeatedly emits `InProgress` updates for the **same** `call_id` while the text artifact grows cumulatively on every update. Tunables:
- `MOCK_AGENT_RUNAWAY_SEARCH_UPDATES` -- number of `ToolCallUpdate(InProgress)` events to emit
- `MOCK_AGENT_RUNAWAY_SEARCH_LINES_PER_UPDATE` -- how many search-result lines to append per update
- `MOCK_AGENT_RUNAWAY_SEARCH_LINE_LEN` -- target width for each generated result line
- `MOCK_AGENT_RUNAWAY_SEARCH_DELAY_MS` -- delay between updates
- `MOCK_AGENT_RUNAWAY_SEARCH_SKIP_COMPLETION` -- if set, do not send a final `Completed` update
- `MOCK_AGENT_RUNAWAY_SEARCH_SKIP_FINAL_TEXT` -- if set, do not send a final text chunk

Used by `@/nori-rs/tui-pty-e2e/tests/acp_runaway_search.rs` to reproduce the current ACP backend bug where one streaming search is normalized and recorded as many full snapshots, eventually crashing `nori` under constrained memory.

**Browser Modify**: The `MOCK_AGENT_BROWSER_MODIFY` env var triggers a scenario where the agent extracts a CDP port from the user prompt (using `browser_modify::extract_cdp_port_from_prompt()` which keys on the `CDP endpoint: http://127.0.0.1:` line prefix from `browser_session::compose_agent_prompt()`), connects to Chrome via CDP WebSocket using `tungstenite`, and changes `document.title` via `Runtime.evaluate`. The CDP operations run on a dedicated `std::thread::spawn` because `tungstenite` is blocking. On success, the agent echoes `BROWSER_MODIFIED:title=<title>` as a text chunk so the E2E test can assert on screen contents. The HTTP connection for the CDP `/json` target list uses `ureq`. Used by `@/nori-rs/tui-pty-e2e/tests/browser_command.rs`.

**Race Condition Simulation**: The `MOCK_AGENT_TOOL_CALLS_DURING_FINAL_STREAM` env var triggers a scenario that reproduces the timing where tool call completions arrive while the final text response is streaming. This is structured in phases:
1. Tool calls that complete before text streaming starts (rendered normally)
2. Text streaming begins (activates the TUI's stream_controller)
3. Additional tool calls begin and complete during text streaming (get deferred by the TUI's interrupt queue)
4. Final text chunk sent and turn ends

This simulates the real-world race condition that the `InterruptManager.flush_completions_and_clear()` in `@/nori-rs/tui/src/chatwidget.rs` handles at task completion.

**Cascade Deferral / Orphan Cell Reproduction**: The `MOCK_AGENT_ORPHAN_TOOL_CELLS` env var triggers a scenario where a tool Begin is cascade-deferred (deferred because the queue is non-empty, even though the stream has ended). The sequence:
1. Tool A Begin handled immediately (no stream active)
2. Text streaming starts (activates `stream_controller`)
3. Tool A End deferred (stream active), making the queue non-empty
4. Tool B Begin deferred (queue non-empty -- cascade deferral)
5. Tool B End deferred
6. Turn ends -- `flush_completions_and_clear` must discard both Begin-B and End-B to avoid creating an orphan `ExecCell` with the raw `call_id` as the command name

**Skipped-Begin / Generic Tool Call**: The `MOCK_AGENT_GENERIC_TOOL_CALL` env var triggers a scenario where a `ToolCall` is sent with a generic title ("Terminal") and no `raw_input`. The ACP translation layer in `@/nori-rs/harness/` skips emitting `ExecCommandBegin` for such calls (no useful display info). On completion, only `ExecCommandEnd` is emitted with the resolved title. This tests the TUI's `handle_exec_end_now` `None` branch -- that it uses `ev.command` from the End event instead of falling back to the raw `call_id`.

**Client Requests**: Outbound requests to the client (file read, file write, and permission approval) are sent via `ConnectionTo<Client>::send_request(...)` and awaited with the SDK's `block_task()` pattern from inside spawned handler tasks. `block_task()` lets the mock await its own outgoing request while the SDK's dispatch loop keeps delivering incoming messages -- the prompt handler runs the real work inside `cx.spawn(...)` so the dispatch loop stays free to deliver cancel notifications and the responses to the mock's own outgoing requests.

### Things to Know

- The mock is a binary crate (no lib.rs) intended only for testing
- Local ACP test runs expect the binary to exist at `target/debug/mock_acp_agent` or via `MOCK_ACP_AGENT_BIN`; `cargo build -p mock-acp-agent` prepares that path when CI is not setting the env var
- Uses the same ACP protocol as real agents for realistic testing
- Simulates streaming with configurable chunk delays
- Supports permission options (accept, deny, skip)
- Session state is tracked per-session ID, including cancel-tail replay state for ordering regressions
- Sleep durations between mock events are tuned to create reliable timing in E2E tests
- **`ExitOnEof` stdin wrapper**: The mock wraps its stdin `AsyncRead` in `ExitOnEof`, which calls `std::process::exit(0)` on EOF. This is required because the SDK's connection runs four actors under a `try_join!` and the foreground future stays alive even after stdin closes, so the mock child would otherwise hang at shutdown. Exiting on EOF preserves the stdin-EOF -> clean-child-exit contract that `AcpConnection`'s graceful shutdown in `@/nori-rs/acp-host/src/connection/acp_connection.rs` relies on
- **`MOCK_AGENT_IGNORE_EOF`**: when set, the `ExitOnEof` wrapper sleeps 60 seconds on stdin EOF instead of exiting, simulating an agent whose EOF teardown stalls (e.g. a hung broker detach). Used by `@/nori-rs/tui-pty-e2e/tests/cloud_mode.rs` to prove the TUI's hard-exit watchdog (see `@/nori-rs/tui/docs.md`, "Exit Is Detach") never waits on a stuck child
- **No catch-all handler**: The mock deliberately does NOT register an `on_receive_dispatch` catch-all. The SDK routes incoming JSON-RPC *responses* through the user handler chain before its default forwarder, so a catch-all that error-replies to "unhandled" messages would intercept the `Dispatch::Response` to the mock's own `fs/write_text_file` request and clobber it with an error, breaking the mock's own file write. The SDK's default handler already rejects unknown *requests* with `method_not_found` and forwards *responses* to their awaiting tasks, so no catch-all is needed

Created and maintained by Nori.
