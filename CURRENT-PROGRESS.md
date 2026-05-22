# Cloud Session Integration — Current Progress

## Completed

### Commit 1: WebSocket transport + SacpConnection::connect_remote()
- Added `tokio-tungstenite` (0.29, rustls-tls-native-roots) and `http` dependencies
- Created `ws_transport` module with `WsReadStream`, `WsSink`, and `connect_ws()` adapters
- Extracted shared SACP builder setup into `establish_connection()` free function
- Made `SacpConnection.child` and `SacpConnection.stderr_task` optional (`Option<>`)
- Added `SacpConnection::connect_remote(ws_url, auth_token, cwd)` constructor
- Updated `shutdown()` and `Drop` to handle remote connections (no child process)
- 5 unit tests for ws_transport (read stream text extraction, close frame, ping/pong filtering, error mapping, sink text wrapping)
- 4 integration tests for connect_remote (establish connection, create session, unreachable URL error, shutdown safety)
- All 466 nori-acp tests pass

### Commit 2: Broker client module
- Created `nori-rs/acp/src/broker/` module with `BrokerClient`, `CloudCredentials`, `SessionInfo`, `BrokerError`, `CloudConnectionInfo`
- `authenticate()` — browser OAuth flow with local HTTP server + redirect capture
- `acquire_session()` — POST /api/sessions/acquire with JWT Bearer auth
- `has_valid_token()` / `is_token_expired()` — JWT expiry checking
- `load_credentials()` / `save_credentials()` — JSON credential persistence in `~/.nori/cli/cloud-auth.json`
- Added `cloud_broker_url: Option<String>` to `NoriConfig` and `[cloud] broker_url` TOML config
- 18 unit tests covering token validation, credential persistence, session acquisition, and error paths

### Commit 3: CLI `cloud` subcommand + backend integration
- Added `CloudCommand` struct with `--broker-url` flag and `TuiCli` flattened config overrides
- Added `Cloud(CloudCommand)` variant to `Subcommand` enum in CLI
- Cloud dispatch handler: resolves broker URL from flag or NoriConfig, authenticates via BrokerClient if needed, acquires session, sets `CloudConnectionInfo` on TuiCli, calls `run_main()`
- Restructured `AcpBackend::spawn()` to branch on `cloud_connection`: uses `SacpConnection::connect_remote()` for cloud, `SacpConnection::spawn()` for local
- Threaded `cloud_connection` through 7-layer data flow: CLI → TuiCli → App → ChatWidgetInit → spawn_agent → spawn_acp_agent → AcpBackendConfig → AcpBackend::spawn
- Cloud mode skips `get_agent_config` check (agent already running remotely)
- Error handling adapts: cloud errors get simple messages, local errors get enhanced error categorization
- 3 CLI parsing tests (with/without broker-url, help text)
- 2 backend integration tests (successful cloud spawn via MockWsServer, failure with unreachable URL)
- All tests pass across nori-acp, nori-cli, and nori-tui

### Commit 4: Session release + disconnect handling
- Added `release_session(session_id)` method to `BrokerClient` — POST /api/sessions/{id}/release with JWT Bearer auth
- Added `ReleaseFailed { status, body }` variant to `BrokerError`
- CLI calls `release_session()` with a 5-second timeout after TUI exits (best-effort cleanup; failures logged, not propagated)
- Added `is_cloud: bool` field to `AcpBackend`, set from `cloud_connection.is_some()` during spawn
- Modified `run_connection_event_relay()` to emit `EventMsg::Error("Cloud session disconnected...")` when WebSocket drops for cloud sessions (previously exited silently)
- 4 unit tests for release_session (success with correct auth, 401 → TokenExpired, 404 → ReleaseFailed, no token → AuthRequired)
- 1 integration test for cloud disconnect detection (mock WS server drops connection, verify error event emitted)
- All 516+ nori-acp tests pass, all 27 nori-cli tests pass

### Commit 5: Documentation + verification
- Verified all tests pass: 546 nori-acp, 1309 nori-tui, 27 nori-cli, 94 E2E (zero failures)
- Verified `cargo build --bin nori` succeeds with zero warnings
- Reviewed existing docs.md files (acp, broker, cli, tui) — all already accurate
- Created `acp/src/connection/docs.md` documenting the dual transport architecture (local subprocess vs remote WebSocket)

## Status

Feature is functionally complete per APPLICATION_SPEC.md. All tests pass, code compiles cleanly, documentation is up to date.

### Out of scope per spec
- Session resume: `nori cloud --resume`
- Session listing: `nori cloud --list`
- Agent selection: `nori cloud --agent <slug>`
