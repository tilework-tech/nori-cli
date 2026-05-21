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
