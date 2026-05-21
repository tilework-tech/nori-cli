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
