# Noridoc: broker

Path: @/nori-rs/acp/src/broker

### Overview

- Implements the client-side integration with the nori-sessions broker for cloud VM sessions
- Manages OAuth browser-based authentication, JWT credential persistence, and session acquisition
- Defines `CloudConnectionInfo`, the value type whose presence signals cloud mode throughout the codebase

### How it fits into the larger codebase

```
nori cloud (CLI)                        nori-sessions broker (external)
      |                                        |
      v                                        |
BrokerClient ----HTTP POST /api/sessions/acquire---->|
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
```

- The CLI (`@/nori-rs/cli/src/main.rs`) is the sole caller of `BrokerClient` -- it authenticates, acquires a session, and constructs the `CloudConnectionInfo` that flows downstream
- `CloudConnectionInfo` is threaded through the TUI layer (`Cli` -> `App` -> `ChatWidgetInit` -> `spawn_agent()`) without modification; the TUI does not interact with the broker directly
- The ACP backend (`@/nori-rs/acp/src/backend/spawn_and_relay.rs`) branches on `config.cloud_connection`: when present, it calls `SacpConnection::connect_remote()` instead of `SacpConnection::spawn()`
- The WebSocket transport adapter (`@/nori-rs/acp/src/connection/ws_transport.rs`) is the component that actually opens the WebSocket connection using the `ws_url` and `auth_token` from `CloudConnectionInfo`
- The broker URL can come from the `--broker-url` CLI flag or from `[cloud] broker_url` in `config.toml` (resolved in `@/nori-rs/acp/src/config/types/mod.rs` as `NoriConfig.cloud_broker_url`)

### Core Implementation

- `CloudConnectionInfo` is a plain struct with `ws_url: String` and `auth_token: String`. It is the branch condition throughout the system -- `Option<CloudConnectionInfo>` being `Some` means cloud mode, `None` means local subprocess mode
- `BrokerClient::new()` loads persisted credentials from `{nori_home}/cloud-auth.json` and filters them to match the current `broker_url`. If the stored credentials are for a different broker, they are ignored
- `BrokerClient::has_valid_token()` checks whether the stored JWT is present and not expired, using `is_token_expired()` which decodes the base64url JWT payload and compares the `exp` claim against the current system time
- `BrokerClient::authenticate()` runs an OAuth browser flow: it binds a local HTTP server on an ephemeral port, opens the broker's `/auth/cli?redirect_uri=...` URL in the default browser, waits for a callback with a `?token=` query parameter, and persists the credentials via `save_credentials()`
- `BrokerClient::acquire_session()` POSTs to `{broker_url}/api/sessions/acquire` with a Bearer token and returns a `SessionInfo` containing `session_id` and `ws_url`. HTTP 401 responses map to `BrokerError::TokenExpired`
- `CloudCredentials` is the serialized form persisted to `cloud-auth.json`; it pairs `broker_url` with `auth_token` so credentials for different brokers do not collide

### Things to Know

- The authentication callback server runs in a separate `std::thread` (not a tokio task) because `tiny_http::Server` is synchronous. Communication with the async caller uses a `tokio::sync::oneshot` channel
- JWT expiry checking (`is_token_expired()`) is deliberately lenient: any token that cannot be decoded as a three-part base64url JWT with a valid `exp` claim is treated as expired. This avoids storing invalid tokens
- The `auth_token()` accessor on `BrokerClient` is used by the CLI to extract the token for constructing `CloudConnectionInfo` after session acquisition -- the token flows to the WebSocket connection as a Bearer auth header
- Cloud mode in `AcpBackend::spawn()` skips agent config lookup (`get_agent_config`) since the remote agent is already running on the cloud VM. Error messages for cloud connection failures use simple messages instead of the enhanced error categorization used for local subprocess failures

Created and maintained by Nori.
