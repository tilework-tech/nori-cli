# Noridoc: broker

Path: @/nori-rs/acp/src/broker

### Overview

- Implements the client-side integration with the nori-sessions broker for cloud VM sessions
- Manages OAuth browser-based authentication, JWT credential persistence, and the full session lifecycle: list, acquire, resume, and release
- Defines `CloudConnectionInfo`, the value type whose presence signals cloud mode throughout the codebase

### How it fits into the larger codebase

```
nori cloud (CLI)                        nori-sessions broker (external)
      |                                        |
      v                                        |
BrokerClient ----GET /api/sessions------------>|  (list existing sessions)
      |                                        |
      v                                        |
      |--- user picks session or new --------->|
      |                                        |
      +----POST /api/sessions/acquire--------->|  (new session)
      +----POST /api/sessions/{id}/resume----->|  (resume existing)
      |                                        |
      v                                        v
CloudConnectionInfo { ws_url, auth_token }   SessionInfo { session_id, ws_url }
      |
      v
TuiCli.cloud_connection --> App --> ChatWidgetInit --> spawn_agent()
      |
      v
AcpBackendConfig.cloud_connection --> AcpBackend::spawn()
      |
      v
SacpConnection::connect_remote(ws_url, auth_token, cwd)
      ...
[TUI session runs]
      ...
      |
      v
BrokerClient ----HTTP POST /api/sessions/{id}/release---->|
      (best-effort, 5s timeout, called from CLI after run_main)
```

- The CLI (`@/nori-rs/cli/src/main.rs`) is the sole caller of `BrokerClient` -- it authenticates, lists sessions, acquires or resumes a session, constructs the `CloudConnectionInfo` that flows downstream, and releases the session after the TUI exits
- The session selection UI (`@/nori-rs/cli/src/cloud.rs`) formats listed sessions and parses user input before the TUI launches; this is a pre-TUI step because the TUI needs a WebSocket URL to start
- `CloudConnectionInfo` is threaded through the TUI layer (`Cli` -> `App` -> `ChatWidgetInit` -> `spawn_agent()` or `spawn_acp_agent_resume()`) without modification; the TUI does not interact with the broker directly
- The ACP backend (`@/nori-rs/acp/src/backend/spawn_and_relay.rs`) branches on `config.cloud_connection` in `spawn()`: when present, it calls `SacpConnection::connect_remote()` instead of `SacpConnection::spawn()`. `resume_session()` does not have a cloud path -- cloud sessions are not recorded locally, so they never appear in the resume pickers
- The WebSocket transport adapter (`@/nori-rs/acp/src/connection/ws_transport.rs`) is the component that actually opens the WebSocket connection using the `ws_url` and `auth_token` from `CloudConnectionInfo`
- The broker URL is resolved by the CLI through a three-step priority chain: `--broker-url` flag > `[cloud] broker_url` in `config.toml` > interactive stdin prompt (terminal only). The interactive prompt validates the URL scheme (`http://` or `https://`) and persists the entered value via `save_cloud_broker_url()` in `@/nori-rs/acp/src/config/loader.rs`. Non-interactive (piped) invocations that lack a configured URL receive an error with setup instructions

### Core Implementation

- `CloudConnectionInfo` is a plain struct with `ws_url: String` and `auth_token: String`. It is the branch condition throughout the system -- `Option<CloudConnectionInfo>` being `Some` means cloud mode, `None` means local subprocess mode
- `CloudSessionSummary` is the listing type returned by `list_sessions()`, carrying `session_id`, `source` (e.g. "cli", "slack", "discord"), `created_at`, `last_active_at`, optional `first_message_preview`, and `status`. The `source` field enables the CLI session picker to show where each session originated
- `BrokerClient::new()` loads persisted credentials from `{nori_home}/cloud-auth.json` and filters them to match the current `broker_url`. If the stored credentials are for a different broker, they are ignored
- `BrokerClient::has_valid_token()` checks whether the stored JWT is present and not expired, using `is_token_expired()` which decodes the base64url JWT payload and compares the `exp` claim against the current system time
- `BrokerClient::authenticate()` runs an OAuth browser flow: it binds a local HTTP server on an ephemeral port, opens the broker's `/api/auth/cli?redirect_uri=...` URL in the default browser, waits up to 2 minutes for a callback with a `?token=` query parameter, and persists the credentials via `save_credentials()`. The 2-minute timeout prevents the CLI from hanging indefinitely if the user abandons the browser flow; on timeout, `server.unblock()` is called to shut down the callback listener thread cleanly
- `BrokerClient::list_sessions()` GETs `{broker_url}/api/sessions` with a Bearer token, returning `Vec<CloudSessionSummary>`. This enables the CLI to show the user their existing sessions before deciding whether to create a new one or resume. HTTP 401 maps to `BrokerError::TokenExpired`; non-success responses map to `BrokerError::ListFailed`
- `BrokerClient::acquire_session()` POSTs to `{broker_url}/api/sessions/acquire` with a Bearer token and a JSON body `{"source": "cli"}`, returning a `SessionInfo` containing `session_id` and `ws_url`. The `source` field identifies this as a CLI client so the broker uses the correct claim identity format. HTTP 401 responses map to `BrokerError::TokenExpired`
- `BrokerClient::resume_session()` POSTs to `{broker_url}/api/sessions/{session_id}/resume` with a Bearer token, returning a `SessionInfo` with the reconnection `ws_url`. This allows resuming sessions originally started from any source (CLI, Slack, Discord). HTTP 401 maps to `BrokerError::TokenExpired`; non-success responses map to `BrokerError::ResumeFailed`
- `BrokerClient::release_session()` POSTs to `{broker_url}/api/sessions/{session_id}/release` with a Bearer token to explicitly release a cloud session. Called by the CLI as a best-effort cleanup after the TUI exits (wrapped in a 5-second timeout). HTTP 401 maps to `TokenExpired`; non-success responses map to `BrokerError::ReleaseFailed`
- `CloudCredentials` is the serialized form persisted to `cloud-auth.json`; it pairs `broker_url` with `auth_token` so credentials for different brokers do not collide

### Things to Know

- The authentication callback server runs in a separate `std::thread` (not a tokio task) because `tiny_http::Server` is synchronous. The server is wrapped in `Arc` so both the callback thread and the timeout handler can call `unblock()` to shut it down. Communication with the async caller uses a `tokio::sync::oneshot` channel, wrapped in a `tokio::time::timeout` for the 2-minute deadline
- JWT expiry checking (`is_token_expired()`) is deliberately lenient: any token that cannot be decoded as a three-part base64url JWT with a valid `exp` claim is treated as expired. This avoids storing invalid tokens
- The `auth_token()` accessor on `BrokerClient` is used by the CLI to extract the token for constructing `CloudConnectionInfo` after session acquisition -- the token flows to the WebSocket connection as a Bearer auth header
- `BrokerClient` covers the full session lifecycle: authenticate -> list -> acquire/resume -> release. The CLI drives this lifecycle; the TUI and backend never call the broker directly
- All broker API methods follow the same error pattern: check for token presence, check local JWT expiry, make the HTTP request, map 401 to `TokenExpired`, and map other non-success status codes to the method-specific `BrokerError` variant (`ListFailed`, `AcquireFailed`, `ResumeFailed`, `ReleaseFailed`)
- The CLI gracefully degrades when `list_sessions()` fails: a 404 (broker does not support listing) returns an empty list, other errors log a warning and fall back to creating a new session. Non-interactive terminals skip session listing entirely and go directly to `acquire_session()`
- Cloud mode in `AcpBackend::spawn()` skips agent config lookup (`get_agent_config`) since the remote agent is already running on the cloud VM. Error messages for cloud connection failures use simple messages instead of the enhanced error categorization used for local subprocess failures. Cloud sessions also skip local transcript recording (`transcript_recorder` is left `None`) because the broker records transcripts server-side; as a result they are not persisted locally and do not surface in the local resume pickers
- The CLI uses its local cwd for cloud sessions -- the broker's SACP proxy (in the nori-sessions repo) manages the sprite-side working directory independently via `AcpTunnelManager`. The `cwd` sent in the `session/new` RPC is discarded by the broker. This means client-side file handlers, transcript recording, and TUI display all reflect the user's local directory, not the sprite's workspace path

Created and maintained by Nori.
