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

### Commit 6: Interactive broker URL prompt
- Added `toml_edit` dependency to `nori-acp` for format-preserving TOML writes
- Added `save_cloud_broker_url(nori_home, broker_url)` function in `acp/src/config/loader.rs`
  - Reads/creates `config.toml`, sets `[cloud] broker_url` via `toml_edit::DocumentMut`, writes back
  - Creates `nori_home` directory if it doesn't exist
- Modified cloud handler in `cli/src/main.rs` to prompt for broker URL when missing
  - Uses `eprint!` + `stdin().read_line()` for interactive prompt
  - Validates URL starts with `http://` or `https://`
  - Persists entered URL to config via `save_cloud_broker_url()`
  - Falls back to error message when stdin is not a terminal (piped usage)
- 4 unit tests for `save_cloud_broker_url` (empty config, preserving existing config, overwriting value, creating directory)
- All nori-acp and nori-cli tests pass

### Commit 7: Documentation accuracy fix
- Updated `core/docs.md` data flow diagram to reflect dual transport: "Agent (JSON-RPC via subprocess or WebSocket)" instead of "Agent subprocess (JSON-RPC)"
- Verified all other docs.md files (acp, broker, connection, cli, tui, nori-protocol) were already accurate

### Commit 8: Auto re-authentication retry on token expiry
- Modified cloud handler in `cli/src/main.rs` to catch `BrokerError::TokenExpired` from `acquire_session()`
- On `TokenExpired`, CLI prints "Token expired, re-authenticating...", calls `broker.authenticate()`, and retries `acquire_session()` once
- Handles edge cases: token passes local `has_valid_token()` but broker returns HTTP 401, or token expires between check and acquire (race condition)
- Single retry only — if the fresh token also fails, the error propagates (prevents infinite loops)
- Updated broker/docs.md and cli/docs.md to document the retry behavior
- No new test added: the retry orchestration is in `main.rs` (not unit-testable without mocking browser auth), and the individual components (TokenExpired on 401, authenticate refreshing token, acquire succeeding) are already thoroughly tested
- All tests pass (556 acp, 27 cli)

### Broker-side: CLI cloud session endpoints (nori-sessions repo, branch cli-cloud-sessions, PR #830)
- Added `cliClaimedBy()` to `claimedBy.ts` — generates `cli:<email>/<iso-timestamp>` claim identifiers
- Added `GET /auth/cli` unauthenticated endpoint serving Firebase JS SDK login page with localhost-only redirect_uri validation
- Modified `POST /sessions/acquire` to accept `source: 'cli'` and return `ws_url` in response
- Added `session_id` (snake_case) alongside `sessionId` (camelCase) in acquire response for Rust CLI compatibility
- Added `POST /sessions/:id/release` path-based release endpoint
- Created WebSocket tunnel at `/api/sessions/:id/ws` — bidirectional frame relay between CLI and sprite ACP, using `ws.Server` with `noServer: true`, Firebase token auth, and existing sprite connector infrastructure
- 18 tests across 4 test files, all passing
- Updated noridocs across 5 docs.md files
- PR #830 open with all CI checks green (Format, Clippy, Dependency audit, Broker TypeScript, Build Linux+macOS, Test, Docs, E2E Linux+macOS)

### Broker-side: End-to-end fixes (nori-sessions repo, branch cli-cloud-sessions)
- Fixed WebSocket tunnel to call `lifecycle.markActive()` at relay start and on each client→sprite message — prevents GC from reclaiming active CLI sessions after 15-minute inactivity timeout
- Fixed acquire endpoint to default to `cliClaimedBy` when request body is empty/absent — CLI sends bare POST with no JSON body, which was incorrectly falling through to `webClaimedBy`
- 2 new tests (markActive on message relay, empty body → CLI claim identity), total 20 tests across 4 files
- Updated 4 docs.md files (ws/docs.md, lifecycle/docs.md, endpoints/docs.md, broker/docs.md)

## Status

Feature is functionally complete per APPLICATION_SPEC.md. All tests pass, code compiles cleanly with zero clippy warnings, documentation is up to date. Broker PR #830 is open with all CI green.

### Remaining CLI-side follow-up
- None — broker now correctly handles CLI's empty-body acquire requests

### Out of scope per spec
- Session resume: `nori cloud --resume`
- Session listing: `nori cloud --list`
- Agent selection: `nori cloud --agent <slug>`
