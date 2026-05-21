# nori cloud — Cloud Session Integration

## Overview

`nori cloud` is a subcommand that lets users run full TUI sessions backed by
cloud VMs managed by the nori-sessions broker. The interaction feels identical
to a local `nori` session — same ratatui TUI, same ACP protocol, same chat
experience — but the agent runs on a remote Fly.io sprite instead of a local
subprocess.

## Architecture

### Connection Model

The broker acts as a transparent WebSocket tunnel between nori-cli and the
sprite's ACP bridge. The broker does not parse or transform ACP messages — it
relays WebSocket frames bidirectionally.

```
nori-cli                    Broker                         Sprite
   |                          |                              |
   |--POST /sessions/acquire->|                              |
   |<--{sessionId, wsUrl}-----|                              |
   |                          |                              |
   |==WebSocket===============|==WebSocket===================|
   |   ACP JSON-RPC (NDJSON)  |   ACP JSON-RPC (NDJSON)     |
   |   initialize ----------->|-----------> initialize       |
   |   session/new ---------->|-----------> session/new      |
   |   session/prompt ------->|-----------> session/prompt    |
   |   <-- session/update ----|<--- session/update           |
   |==========================|==============================|
```

The full ACP lifecycle (initialization handshake, session creation, prompting,
cancellation, model switching) runs end-to-end through the tunnel. The TUI
receives the same `ClientEvent` stream it would from a local agent, so all
standard features work: chat, tool approvals, file operations, reasoning
display, etc.

### Session Lifecycle

- **Acquire**: nori-cli calls `POST /api/sessions/acquire` to claim a sprite
  and receive a session ID + WebSocket URL.
- **Connect**: nori-cli opens a WebSocket to the broker's tunnel endpoint. The
  broker opens a corresponding WebSocket to the sprite's `/acp` endpoint and
  relays frames.
- **Disconnect**: When the user exits nori or the WebSocket closes, the session
  is NOT destroyed. The sprite remains claimed and the agent session persists on
  the broker side. This enables future session resume.
- **Release**: Explicit session teardown is a separate user action via
  `POST /api/sessions/{id}/release`. (Session resume is on the roadmap but not
  part of the initial implementation.)

## Authentication

### Browser-based OAuth Flow

1. User runs `nori cloud` with no stored credentials.
2. CLI checks for `broker_url` in config. If missing, prompts:
   "Enter your org's broker URL:".
3. CLI starts a local HTTP server on a random port.
4. Opens browser to
   `{broker_url}/auth/cli?redirect_uri=http://localhost:{port}/callback`.
5. User logs in via Firebase on the broker's auth page.
6. Broker redirects to `http://localhost:{port}/callback?token={jwt}`.
7. CLI captures the JWT, stores it alongside the broker URL.
8. Local HTTP server shuts down.

### Subsequent Runs

- CLI reads the stored token and includes it as `Authorization: Bearer {jwt}`
  on all broker HTTP and WebSocket requests.
- If the token is expired, the browser auth flow is re-triggered automatically.

### Credential Storage

New fields in `~/.nori/cli/config.toml`:

```toml
[cloud]
broker_url = "https://nori-broker.myorg.fly.dev"
auth_token = "eyJhbG..."
```

Alternative: a separate file like `~/.nori/cli/cloud-auth.json` to keep
secrets out of the main config. Decision deferred to implementation.

## Code Changes in nori-cli

### 1. CLI Layer (`nori-rs/cli/`)

Add a `cloud` subcommand to the clap argument parser. It runs the same TUI but
with a different backend initialization path — instead of spawning a local
agent, it acquires a broker session and connects via WebSocket.

### 2. Broker Client (`nori-rs/acp/src/broker/`)

New module containing:

- **`BrokerClient`** struct holding broker URL and auth token.
- **`authenticate()`** — Runs the browser OAuth flow: starts a local HTTP
  server, opens the browser, captures the callback token.
- **`acquire_session()`** — Calls `POST /api/sessions/acquire` with the JWT.
  Returns session ID and WebSocket URL.
- **Token loading/storage** from config.

### 3. WebSocket Transport (`nori-rs/acp/src/connection/`)

Modify `SacpConnection` to support two construction paths:

- **`SacpConnection::spawn()`** — Existing path. Spawns a local subprocess,
  communicates over stdin/stdout via `ByteStreams`.
- **`SacpConnection::connect_remote(ws_url, auth_token)`** — New path. Opens a
  WebSocket via `tokio-tungstenite`, wraps the read/write halves in a
  `ByteStreams`-compatible adapter.

The rest of `SacpConnection`'s API (`create_session`, `prompt`, `cancel`,
`take_event_receiver`) is unchanged. The `child` process field becomes
`Option<Child>` (`None` for remote connections). Shutdown for remote
connections closes the WebSocket instead of killing a process.

### 4. Backend (`nori-rs/acp/src/backend/`)

`AcpBackend::spawn()` gains a parallel path for cloud mode. Instead of calling
`get_agent_config()` + `SacpConnection::spawn()`, it calls
`BrokerClient::acquire_session()` + `SacpConnection::connect_remote()`.
Everything downstream — the session reducer, event relay, TUI integration — is
unchanged.

### 5. New Dependency

- `tokio-tungstenite` for async WebSocket client support.

## Broker-Side Dependencies (nori-sessions repo)

These changes are outside the nori-cli codebase but are required for the
feature to work:

### 1. `GET /auth/cli`

Serves a Firebase login page that accepts a `redirect_uri` query parameter.
After successful login, redirects to `{redirect_uri}?token={jwt}`.

### 2. `POST /api/sessions/acquire`

Already exists. May need minor changes to support CLI-originated sessions
(currently tuned for Slack/dashboard use).

### 3. `GET /api/sessions/{id}/ws` (new)

WebSocket endpoint. Authenticates the JWT, looks up the sprite for the session,
opens (or reuses) a WebSocket to the sprite's `/acp` endpoint, and pipes frames
bidirectionally. Transparent relay — no ACP message parsing.

### 4. `POST /api/sessions/{id}/release`

Already exists. For explicit session teardown when the user wants it (not
triggered on WebSocket disconnect).

## User Experience

### Happy Path

```
$ nori cloud
Enter your org's broker URL: https://nori-broker.myorg.fly.dev
Opening browser for authentication...
Authenticated as user@myorg.com
Acquiring cloud session...
Connected to sprite nori-warm-fox-a3f2

[Full TUI renders with > prompt]
```

### Subsequent Runs

```
$ nori cloud
Acquiring cloud session...
Connected to sprite nori-cool-bear-1d8e

[Full TUI renders immediately]
```

## Feature Parity

Most TUI features work automatically through proper ACP tunneling:
- Chat input/output
- Tool call approval flows
- File operation notifications
- Reasoning/thinking display
- Model switching
- MCP server forwarding

Features that may not work or need adaptation will be addressed as they arise.
The `nori cloud` subcommand sets the expectation that some divergence from local
behavior is acceptable.

## Future Work (not in scope)

- **Session resume**: `nori cloud --resume` reconnects to an existing session.
- **Session listing**: `nori cloud --list` shows active/paused sessions.
- **Agent selection**: `nori cloud --agent <slug>` overrides the org default.
- **Direct sprite connection**: Bypassing the broker for lower latency (ruled
  out for now due to auth complexity).
