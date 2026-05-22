# Research Notes: Cloud Session Integration

## Transport Architecture Decision

### Option A: ByteStreams (AsyncRead/AsyncWrite adapter)
- Wraps WebSocket read/write halves in custom AsyncRead/AsyncWrite impls
- Uses tokio_util::io::StreamReader for read side, manual poll_write for write side
- Then compat() to convert tokio→futures traits for sacp::ByteStreams
- Complex: requires poll-level implementations with correct waker semantics

### Option B: Lines (Sink/Stream adapter) — **CHOSEN**
- `sacp::Lines` takes `Sink<String, Error=io::Error>` + `Stream<Item=io::Result<String>>`
- WebSocket messages map 1:1 to NDJSON lines — natural framing match
- Simple newtype wrappers: map Message::Text↔String, filter control frames
- Avoids all AsyncRead/AsyncWrite complexity
- Both ByteStreams and Lines implement `ConnectTo<R>`, so the builder pattern is identical

### Why Lines wins
SACP uses NDJSON where each message is one line. WebSocket already provides message framing.
ByteStreams would re-add byte-stream abstraction on top of a message-oriented protocol — wasteful.
Lines matches the semantics directly.

## Key Code Findings

### SacpConnection (acp/src/connection/sacp_connection.rs)
- Has `child: Arc<Mutex<Child>>` and `stderr_task: JoinHandle<()>` — both process-specific
- The builder pattern (Client.builder()...connect_with(transport, ...)) is transport-agnostic
- connect_with accepts any `impl ConnectTo<Host> + 'static`
- All session methods (create_session, prompt, cancel) use `self.cx: ConnectionTo<Agent>` — transport-independent

### sacp::Lines (sacp-11.0.0/src/jsonrpc.rs:3183)
- `Lines<OutgoingSink, IncomingStream>` where:
  - OutgoingSink: `Sink<String, Error=io::Error> + Send + 'static`
  - IncomingStream: `Stream<Item=io::Result<String>> + Send + 'static`
- Implements `ConnectTo<R>` for any Role

### tokio-tungstenite
- Latest: 0.29.0
- connect_async accepts `impl IntoClientRequest + Unpin`
- Auth header: `request.headers_mut().insert("Authorization", format!("Bearer {token}").parse()?)`
- Split via `futures_util::StreamExt::split()` → SplitSink + SplitStream
- WebSocketStream is NOT AsyncRead/AsyncWrite — it's Stream<Message> + Sink<Message>
- Needs TLS feature for wss:// — use `rustls-tls-native-roots` for pure-Rust TLS

### Struct changes needed
- `child: Arc<Mutex<Child>>` → `Option<Arc<Mutex<Child>>>`
- `stderr_task: JoinHandle<()>` → `Option<JoinHandle<()>>`
- shutdown()/Drop: guard on Some before killing process

## Commit 2: Broker Client Research

### Config System (ACP-native path)
- TOML: `NoriConfigToml` at `acp/src/config/types/mod.rs:183` — all fields `Option<T>`, sub-sections use `#[serde(default)]`
- Resolved: `NoriConfig` at `acp/src/config/types/mod.rs:1632` — concrete types with defaults
- Resolution: `NoriConfig::from_toml()` at `acp/src/config/loader.rs:78`
- Home dir: `find_nori_home()` → `$NORI_HOME` or `~/.nori/cli`, config at `{nori_home}/config.toml`
- Backend config: `AcpBackendConfig` at `acp/src/backend/mod.rs:164` — subset passed to `AcpBackend::spawn()`
- Test helper: `build_test_config()` at `acp/src/backend/tests/mod.rs:235` — must update when adding fields
- Pattern for new section: add `CloudConfigToml` with `#[serde(default)]` on `NoriConfigToml`, add resolved `CloudConfig` to `NoriConfig`, resolve in `from_toml()`

### Existing Login Crate (reference implementation)
- `nori-rs/login/src/server.rs` — full OAuth 2.0 + PKCE flow using `tiny_http` + `webbrowser`
- Uses `tiny_http::Server::http("127.0.0.1:1455")` (fixed port) + `webbrowser::open(&url)`
- Async/sync bridge: `std::thread` runs blocking `server.recv()`, forwards to `tokio::sync::mpsc::channel`
- Shutdown: `server.unblock()` to release blocking recv thread
- Broker flow is simpler: no PKCE, no code exchange — just capture JWT from callback query param

### Crate Choices for Broker Client
- **HTTP server for OAuth callback**: `tiny_http` 0.12 (workspace dep, already used in login crate)
- **Browser opening**: `webbrowser` 1.0 (workspace dep, already used in login crate)
- **HTTP client for acquire_session**: `reqwest` 0.12 (workspace dep, already used in core crate)
- **JWT decoding**: `jsonwebtoken` — use `insecure_disable_signature_validation()` to skip verification, only check `exp` claim. Needs to be added as workspace dep.
- Neither `tiny_http`, `webbrowser`, `reqwest`, nor `jsonwebtoken` are currently deps of the `acp` crate — need to add them.

### Broker Auth Flow (simplified vs login crate)
1. CLI starts `tiny_http` server on `127.0.0.1:0` (random port)
2. Opens browser to `{broker_url}/auth/cli?redirect_uri=http://localhost:{port}/callback`
3. User authenticates in browser (Firebase)
4. Broker redirects to `http://localhost:{port}/callback?token={jwt}`
5. CLI captures JWT from query param, stores it, shuts down server
- No PKCE, no code exchange, no state parameter needed (broker generates the JWT directly)

### Module Structure
- New module: `acp/src/broker/mod.rs` — `BrokerClient` struct
- `sacp_connection.rs` is already 910 lines (over 500 LoC guideline) — keep broker logic separate
- Credential storage: separate file `{nori_home}/cloud-auth.json` keeps secrets out of main config; broker_url stays in config.toml `[cloud]` section

## Commit 3: CLI cloud Subcommand + Backend Integration Research

### Data Flow Architecture
Cloud connection info needs to flow: CLI → TUI → ChatWidget → spawn_agent → AcpBackend::spawn.
- `CloudConnectionInfo { ws_url, auth_token }` struct in broker module
- Threaded via `#[clap(skip)]` field on `TuiCli`, through `ChatWidgetInit`, `spawn_agent()`, `AcpBackendConfig`
- `AcpBackend::spawn()` branches: cloud → `connect_remote()`, local → existing `spawn()` path
- After connection established, everything converges: `create_session()`, event relay, transcript, hooks all shared

### CLI Layer (cli/src/main.rs)
- `Subcommand` enum at line 63-93: Add `Cloud(CloudCommand)` variant
- `CloudCommand` struct: `--broker-url` optional flag (falls back to config `[cloud].broker_url`)
- Dispatch at line 397: cloud handler does pre-TUI auth+acquire, then calls `nori_tui::run_main()`
- Auth/acquire must happen BEFORE TUI takes over terminal (browser needs normal terminal mode)
- Pattern: set `interactive.cloud_connection = Some(CloudConnectionInfo { ws_url, auth_token })`

### TUI Layer Threading
- `TuiCli` (tui/src/cli.rs): add `#[clap(skip)] cloud_connection: Option<CloudConnectionInfo>`
- `ChatWidgetInit` (tui/src/chatwidget/mod.rs:310): add `cloud_connection` field
- `App::chat_widget_init()` (tui/src/app/mod.rs:513): pass cloud_connection through
- `spawn_agent()` (agent.rs:166): accept cloud_connection, skip `get_agent_config()` check for cloud mode
- `spawn_acp_agent()` (agent.rs:219): accept cloud_connection, set on `AcpBackendConfig`

### AcpBackend::spawn() Cloud Branch
- `AcpBackendConfig` (backend/mod.rs:164): add `cloud_connection: Option<CloudConnectionInfo>`
- In `spawn()`: branch on `config.cloud_connection`
  - Cloud: `SacpConnection::connect_remote(ws_url, auth_token, cwd)` with simple error message
  - Local: existing path with `get_agent_config()`, `SacpConnection::spawn()`, categorized errors
  - Both: `agent_config` is `Option<AcpAgentConfig>` — `Some` for local, `None` for cloud
- `create_session()` still called for both (ACP protocol required through tunnel)
- Error handling for `create_session` uses agent_config if available, else simple cloud error
- Everything after create_session is identical (event relay, transcript, hooks, etc.)

### Agent Name for Cloud Mode
- `config.agent` set to "cloud" by CLI dispatch
- `get_agent_config()` skipped (agent not in local registry)
- Model display and transcript metadata show "cloud"

### Test Implications
- `build_test_config()` (backend/tests/mod.rs:235) must add `cloud_connection: None`
- ChatWidgetInit test constructors must add `cloud_connection: None`
- New unit test: `spawn_agent` with `cloud_connection: Some(...)` skips agent config check
- Integration test: `AcpBackend::spawn()` with cloud_connection connects via mock WS server

## Commit 4: Session Release + Disconnection Handling Research

### Session Release Architecture

#### Where broker session_id is available
- `SessionInfo.session_id` returned by `broker.acquire_session()` at `cli/src/main.rs:543`
- Currently DISCARDED — only `ws_url` is forwarded into `CloudConnectionInfo`
- `BrokerClient` instance is local to the cloud dispatch handler in main.rs

#### Simplest release approach: CLI layer
- After `nori_tui::run_main()` returns (main.rs:562), `broker` and `session_info` are still in scope
- Call `broker.release_session(&session_info.session_id)` with a 5s timeout
- Log and move on if it fails (best-effort cleanup)
- No need to thread broker_session_id through TUI layers

#### Async shutdown patterns
- No async Drop in Rust — `block_on` inside Drop panics inside tokio runtime
- Use explicit `async fn release_session()` awaited with `tokio::time::timeout`
- `tokio::spawn` fire-and-forget risks task cancellation on runtime drop
- The runtime is still alive after `run_main()` returns, so plain await works

### WebSocket Disconnection Handling

#### Current behavior (gap)
- WS close frame → `WsReadStream` returns `Poll::Ready(None)` → SACP task exits → `event_tx` dropped → relay loop breaks silently → TUI agent task exits silently → NO USER FEEDBACK
- WS error → same path, also silent
- Only during active prompts are errors surfaced via `Event::Error`

#### `ConnectionEvent` enum (acp/src/connection/mod.rs)
- Only two variants: `SessionUpdate`, `ApprovalRequest`
- No `Disconnected` or `Error` variant

#### Event relay loop (spawn_and_relay.rs:263-384)
- `event_rx.recv()` returning `None` → loop breaks silently
- This is where disconnect detection should go
- Need to differentiate: user-initiated shutdown vs. unexpected connection loss
- `is_shutting_down` flag already present on `AcpBackend` — can check this

#### Proposed disconnect detection
1. When `event_rx.recv()` returns `None` in the relay loop:
   - Check if this was a user-initiated shutdown (check shutdown flag)
   - If NOT: send an error event to TUI ("Cloud session disconnected")
2. Use `BackendEvent::Control(Event::Error(...))` to surface the disconnect
3. The TUI already handles `Event::Error` via `send_prompt_error` / error display

### Cloud-Specific Error Messages

#### Current error categories (session_runtime_driver.rs:568-610)
- `AcpErrorCategory::Authentication`, `QuotaExceeded`, `ExecutableNotFound`, `Initialization`, `PromptTooLong`, `ApiServerError`, `Unknown`
- These are for local agent errors — cloud errors need different messaging

#### Cloud error scenarios
1. **Broker unreachable** — handled at CLI layer before TUI starts (already has error messages)
2. **Auth expired during session** — WS might get 401/close; surface as "Authentication expired"
3. **WS connection dropped** — surface as "Cloud session disconnected. The remote session may still be active."
4. **Prompt fails during cloud session** — existing `Event::Error` path works, but message should say "Cloud agent error" not "Agent process crashed"

### Module Locations (key files to modify)
- `acp/src/broker/mod.rs` — add `release_session()` and `ReleaseFailed` error variant
- `acp/src/backend/spawn_and_relay.rs:263-384` — disconnect detection in relay loop
- `acp/src/connection/mod.rs` — no changes needed (no new ConnectionEvent variant; detect at relay level)
- `cli/src/main.rs:522-564` — call release after `run_main()` returns
- `acp/src/broker/mod.rs:10-13` — CloudConnectionInfo may need `is_cloud` flag or similar for backend to know it's cloud mode
- `acp/src/backend/mod.rs` — store `is_cloud` flag on AcpBackend for cloud-specific error messages

## Commit 6: Interactive Broker URL Prompt Research

### Spec Gap
The APPLICATION_SPEC says: "CLI checks for `broker_url` in config. If missing, prompts: 'Enter your org's broker URL:'"
Current implementation (main.rs:527-531) returns an error with instructions instead of prompting.

### Interactive stdin Reading
- No interactive prompt crate (`dialoguer`, `inquire`) in workspace deps
- The `read_api_key_from_stdin()` in `cli/src/login.rs:94` is the closest analog but only handles piped input
- The cloud handler runs BEFORE `nori_tui::run_main()`, so crossterm raw mode is NOT active — safe to use stdin
- Pattern: `eprint!("prompt")` + `io::stderr().flush()` + `stdin().lock().read_line()`
- Use `eprint!`/`eprintln!` for all output (existing convention in cloud handler)

### Config Persistence
- No existing `save_config()` or config write function in `acp/src/config/`
- `toml_edit` is a workspace dep (v0.23.5, used by `codex-core`) but NOT a dep of the `acp` crate
- Need to add `toml_edit` to `acp/Cargo.toml` to use format-preserving TOML edits
- Config file path: `find_nori_home()?.join("config.toml")` → `~/.nori/cli/config.toml`
- Write pattern: parse with `DocumentMut`, set `doc["cloud"]["broker_url"] = value(url)`, write back
- Atomic write: use `tempfile::NamedTempFile` + `persist()` (tempfile already a dev-dep of acp)

### Implementation Plan
1. Add `toml_edit` dep to `acp/Cargo.toml`
2. Add `save_cloud_broker_url(nori_home, url)` function in `acp/src/config/loader.rs`
3. Modify cloud handler in `cli/src/main.rs:522-531`:
   - When both CLI flag and config are None, prompt for broker URL
   - After receiving URL, call `save_cloud_broker_url()` to persist
   - Continue with the cloud flow using the provided URL
4. Handle edge cases: empty input, non-terminal stdin (piped), Ctrl+C during prompt
