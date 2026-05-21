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

## Next Steps (in spec order)

### Commit 2: Broker client module
- `BrokerClient` struct in `nori-rs/acp/src/broker/`
- `authenticate()` — browser OAuth flow (local HTTP server + redirect capture)
- `acquire_session()` — POST /api/sessions/acquire
- Token loading/storage from config
- Credential storage in `~/.nori/cli/config.toml` `[cloud]` section

### Commit 3: CLI `cloud` subcommand + backend integration
- Add `Cloud(CloudCommand)` to `Subcommand` enum in CLI
- `AcpBackend::spawn_cloud()` or equivalent that uses BrokerClient + connect_remote
- TUI launch path for cloud mode

### Commit 4: Session release + error handling
- POST /api/sessions/{id}/release on explicit teardown
- Reconnection UX (show clear message when WS drops)
- Cloud-specific error messages
