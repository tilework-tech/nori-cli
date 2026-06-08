# Research Notes

## Cloud Session Architecture

### Current State
- `nori cloud` authenticates with broker, acquires a NEW session every time, connects via WebSocket, launches TUI
- `BrokerClient` has only `acquire_session()` and `release_session()` — no list/resume
- Cloud sessions skip local transcript recording (broker records server-side)
- No feature gating in cloud mode — all TUI features available identically

### Key Files
- `nori-rs/acp/src/broker/mod.rs` — BrokerClient, CloudConnectionInfo, SessionInfo, BrokerError
- `nori-rs/cli/src/main.rs:522-608` — Cloud subcommand handler
- `nori-rs/tui/src/nori/resume_session_picker.rs` — Local resume picker (reference for patterns)
- `nori-rs/acp/src/broker/tests.rs` — Broker client tests (integration tests with tiny_http mock servers)

### API Flow
1. CLI resolves broker URL (flag > config.toml > interactive prompt)
2. CLI authenticates (OAuth browser flow → JWT persisted to cloud-auth.json)
3. CLI acquires session (POST /api/sessions/acquire → SessionInfo { session_id, ws_url })
4. CLI launches TUI with CloudConnectionInfo { ws_url, auth_token }
5. On exit, CLI releases session (POST /api/sessions/{id}/release, best-effort 5s timeout)

### Design Decision: Pre-TUI Session Picker
The session selection must happen BEFORE the TUI launches because the TUI needs a WebSocket URL.
Using simple stdin interaction (eprintln + read_line) — no new crate deps needed.
The local `/resume` picker uses in-TUI SelectionView, but that requires a running TUI.

### New Broker API Endpoints Assumed
- `GET /api/sessions` → Vec<CloudSessionSummary> (list user's sessions)
- `POST /api/sessions/{id}/resume` → SessionInfo { session_id, ws_url } (resume existing session)

### New Types Needed
- `CloudSessionSummary` — session_id, source, created_at, last_active_at, first_message_preview, status
- `BrokerError::ListFailed` — for list endpoint failures
- `BrokerError::ResumeFailed` — for resume endpoint failures
