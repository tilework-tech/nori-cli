# Noridoc: connection

Path: @/nori-rs/acp-host/src/connection

### Overview

- Provides `AcpConnection`, the ACP transport layer for communicating with agents over JSON-RPC using the official `agent-client-protocol` SDK
- The single construction path is `spawn()`: launch the agent as a child subprocess and speak newline-delimited JSON-RPC over its stdin/stdout
- Owns the child's full lifecycle: spawn, exit watching and reaping, stderr capture, graceful shutdown, and kill-as-backstop

### How it fits into the larger codebase

```
AcpBackend (spawn/resume)
    |
    v
AcpConnection::spawn()
    |
    v
agent_client_protocol::Lines over child stdin/stdout
    |
    v
Local ACP Agent (subprocess, own process group)
```

- This module is part of the `nori-acp-host` crate (`@/nori-rs/acp-host/`), the agent-agnostic Layer-0 leaf; `nori-harness` re-exports it as `nori_harness::connection`
- `AcpBackend` in `@/nori-rs/harness/src/backend/` is the sole consumer of `AcpConnection` -- both `AcpBackend::spawn()` and `AcpBackend::resume_session()` call `AcpConnection::spawn()` with an `AcpAgentConfig` resolved from the registry in `@/nori-rs/acp-host/src/registry.rs`
- `nori cloud` rides this exact path: `@/nori-rs/cli/src/cloud.rs` pins a registry entry that runs `nori-handroll cloud-acp`, and that child is spawned here like any other local agent. There is no remote/WebSocket transport in this crate; it lives in the nori-sessions repo
- MCP server configuration from `config.toml` is converted to ACP schema types via `mcp.rs` and passed at session creation time
- All transport events (session updates, permission requests, synthetic file-operation updates, and child exits) flow into a single ordered `mpsc::Receiver<ConnectionEvent>` consumed by the backend's relay loop in `@/nori-rs/harness/src/backend/spawn_and_relay.rs`
- The wire logging layer (`wire_log.rs`) optionally wraps the subprocess transport when `[acp_proxy]` is enabled in config

### Core Implementation

- **`AcpConnection`** (`acp_connection.rs`): The central `Send + Sync` type that owns the SDK connection task, the child's teardown handles, and the event receiver. Key methods: `create_session()`, `load_session()`, `resume_session()`, `close_session()`, `list_sessions()`, `prompt()`, `cancel()`, `set_config_option()`
- **`establish_connection()`**: A free function that registers all SDK handlers (notification, permission request, file read/write) and performs the initialization handshake. The caller (`spawn()`) supplies the `ConnectionEvent` channel so extra producers -- notably the child exit watcher -- can report through the same ordered stream
- **Child exit watcher**: A background task OWNS the `Child`: it `wait()`s and reaps the process, publishes the exit status on a `watch` channel, and emits `ConnectionEvent::ChildExited { status, stderr_tail }` (defined in `mod.rs`) when the child dies. The connection holds only a `ChildHandle` (pid, exit-status receiver, kill `Notify`) -- never the `Child` itself
- **stderr capture**: A background task logs each stderr line via tracing and keeps a bounded tail of recent lines. The tail is attached to startup failures and `ChildExited` events so the real cause (e.g. an auth hint) is user-visible
- **Startup race**: `spawn()` races the initialization handshake against child death. An agent that exits immediately (e.g. unauthenticated `nori-handroll` printing "run: nori-handroll login") fails the spawn fast with its stderr tail in the error text, which lets `categorize_acp_error` in the backend classify the failure (e.g. Authentication) instead of reporting protocol incompatibility
- **Graceful shutdown**: `shutdown()` delegates to `shutdown_with_grace()` with a generous default. Aborting the connection task drops the transport and closes the child's stdin -- stdin EOF is the agent's shutdown signal. The connection waits up to the grace period for a voluntary exit before SIGKILLing the process group, then waits for the watcher to reap so no zombie outlives shutdown
- **MCP server forwarding** (`mcp.rs`): `to_acp_mcp_servers()` converts CLI-configured MCP servers to ACP `McpServer` values, resolving environment variables and OAuth tokens eagerly at conversion time. Disabled servers are filtered out. See `@/nori-rs/harness/docs.md` for details on OAuth token injection
- **Ordered event inbox**: Session notifications, permission requests, synthetic file-operation updates, and child exits all flow through one `ConnectionEvent` channel. The backend consumes this single inbox to avoid ordering ambiguity between channels

### Things to Know

- The connection sends `InitializeRequest` with `ProtocolVersion::LATEST` and enforces `MINIMUM_SUPPORTED_VERSION = ProtocolVersion::V1` during the initialization handshake in `establish_connection()`; wire/schema types come from `agent_client_protocol_schema::v1` (aliased as `acp`)
- The child process is spawned in its own process group (`setpgid(0, 0)`) and `CODEX_HOME` is stripped from the environment to prevent config parser conflicts (see `@/nori-rs/harness/docs.md` for rationale)
- The stdin-EOF-then-grace shutdown contract exists because agents may need network cleanup on exit; `nori-handroll cloud-acp` releases its broker session on stdin EOF, and the previous immediate-SIGKILL shutdown leaked every cloud session
- `Drop` on `AcpConnection` is only a backstop for paths that never ran `shutdown()`: it requests a kill via the recorded pid, with a pid-reuse guard that only signals while the child is unreaped
- Without the `ChildExited` event, a dead child is a silent EOF that the ACP SDK layer treats as non-terminal -- pending requests would hang forever. The backend relay turns it into a visible error and fails any in-flight prompt
- File write handlers restrict writes to the workspace directory or `/tmp` (canonicalized path check). File read handlers are unrestricted. Both emit synthetic `ToolCall` updates for TUI rendering
- `AcpConnection::prompt()` absorbs stale cancel-tail responses (empty `end_turn` responses left over from a previous cancellation) by retrying until streamed updates arrive, keeping the reducer contract unchanged
- `resume_session()` sends ACP `session/resume` — a live reattach with **no** history replay, unlike `session/load`. It is the reattach path for agents that advertise `sessionCapabilities.resume` with `loadSession: false` (the nori cloud contract). The response carries no session id (the input id is returned) but may carry `config_options`, which replace the live session-config snapshot. `close_session()` sends ACP `session/close` to release the agent-side session. Failures on both preserve the structured `acp::Error` (JSON-RPC code plus `data.detail`) in the anyhow chain so `categorize_acp_error_chain()` in `@/nori-rs/acp-host/src/error_category.rs` can classify them precisely
- `list_sessions()` sends the ACP `session/list` request and drains cursor pagination internally (following `next_cursor` until exhausted, bounded by a page cap so a misbehaving agent that keeps returning a cursor cannot loop unbounded), concatenating every page in agent order. Each ACP `SessionInfo` is mapped into `AcpSessionSummary` (defined in `mod.rs`, re-exported from `nori_harness`), an owned boundary type carrying `session_id`, `cwd`, optional `title`, and optional `updated_at`. The boundary type decouples consumers from the raw ACP schema, letting `@/nori-rs/tui` -- which has no ACP-schema dependency -- render the agent-sourced `/resume` picker. It mirrors `load_session()`'s structure; the agent only honors `session/list` when it advertises the capability (projected as `agent.session_list`, see `@/nori-rs/harness/docs.md`)

Created and maintained by Nori.
