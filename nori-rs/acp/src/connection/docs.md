# Noridoc: connection

Path: @/nori-rs/acp/src/connection

### Overview

- Provides `SacpConnection`, the unified ACP transport layer for communicating with agents over SACP v11 JSON-RPC
- Supports two construction paths: local subprocess via `spawn()` (stdin/stdout) and remote WebSocket via `connect_remote()`
- Both paths converge on a shared `establish_connection()` function, so the downstream API surface (session creation, prompting, cancellation) is identical regardless of transport

### How it fits into the larger codebase

```
AcpBackend (spawn/resume)
    |
    v
SacpConnection
    |
    +-- spawn() --> sacp::ByteStreams (stdin/stdout of child process)
    |                   |
    |                   v
    |               Local ACP Agent (subprocess)
    |
    +-- connect_remote() --> sacp::Lines (ws_transport adapters)
                                |
                                v
                            Remote ACP Agent (cloud sprite via broker)
```

- `AcpBackend` in `@/nori-rs/acp/src/backend/` is the sole consumer of `SacpConnection` -- it calls `spawn()` for local agents and `connect_remote()` when `AcpBackendConfig.cloud_connection` is `Some`
- The `broker` module (`@/nori-rs/acp/src/broker/`) provides the `CloudConnectionInfo` (ws_url + auth_token) consumed by `connect_remote()`
- MCP server configuration from `config.toml` is converted to ACP schema types via `mcp.rs` and passed at session creation time
- All transport events (session updates, permission requests, file operations) flow into a single ordered `mpsc::Receiver<ConnectionEvent>` consumed by the backend's relay loop
- The wire logging layer (`wire_log.rs`) optionally wraps local subprocess transports when `[acp_proxy]` is enabled in config; it is not applied to WebSocket connections

### Core Implementation

- **`SacpConnection`** (`sacp_connection.rs`): The central `Send + Sync` type that owns the SACP connection, child process (if local), and event receiver. Key methods: `create_session()`, `load_session()`, `prompt()`, `cancel()`, `set_model()` (behind `#[cfg(feature = "unstable")]`). The `child` and `stderr_task` fields are `Option` -- `Some` for local subprocess connections, `None` for remote WebSocket connections
- **Dual transport convergence**: Both `spawn()` and `connect_remote()` delegate to `establish_connection()`, which accepts any `sacp::ConnectTo<Client>` transport. This function registers all SACP handlers (notification, permission request, file read/write) and performs the initialization handshake. The local path passes `sacp::ByteStreams`, the remote path passes `sacp::Lines`
- **WebSocket transport** (`ws_transport.rs`): `connect_ws(url, auth_token)` establishes a WebSocket connection with Bearer auth via `tokio-tungstenite`. Returns split adapter halves: `WsSink<S>` (wraps write half as `Sink<String>`) and `WsReadStream<S>` (wraps read half as `Stream<Item = io::Result<String>>`). The read stream extracts text/binary messages, filters ping/pong/frame control messages, and terminates on close frames. WebSocket errors map to `io::Error` with `ConnectionAborted` kind
- **MCP server forwarding** (`mcp.rs`): `to_sacp_mcp_servers()` converts CLI-configured MCP servers from `codex_core::config::types::McpServerConfig` to ACP `McpServer` values, resolving environment variables and OAuth tokens eagerly at conversion time. Disabled servers are filtered out. See `@/nori-rs/acp/docs.md` for details on OAuth token injection
- **Ordered event inbox**: Session notifications, permission requests, and synthetic file-operation updates all flow through one `ConnectionEvent` channel. The backend consumes this single inbox to avoid ordering ambiguity between notification and approval channels

### Things to Know

- The minimum supported ACP protocol version is V1, enforced during the initialization handshake in `establish_connection()`
- For local connections, the child process is spawned in its own process group (`setpgid(0, 0)`) and `CODEX_HOME` is stripped from the environment to prevent config parser conflicts (see `@/nori-rs/acp/docs.md` for rationale)
- File write handlers restrict writes to the workspace directory or `/tmp` (canonicalized path check). File read handlers are unrestricted. Both emit synthetic `ToolCall` updates for TUI rendering
- The `is_shutting_down` flag on `AcpBackend` suppresses cloud disconnect error messages during normal `Op::Shutdown` -- without this, WebSocket close during shutdown would produce a spurious user-visible error
- `SacpConnection::prompt()` absorbs stale cancel-tail responses (empty `end_turn` responses left over from a previous cancellation) by retrying until streamed updates arrive, keeping the reducer contract unchanged

Created and maintained by Nori.
