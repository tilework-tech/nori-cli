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

## Cloud Mode Feature Gating (Item 2)

### Existing Infrastructure
The codebase already has a `CommandAvailability` / `disabled_builtins` system:
- `nori_protocol::CommandAvailability { enabled: bool, reason: Option<String> }` — per-command gate with user-facing reason
- `ChatWidget::builtin_command_availability` (HashMap) — stores availability state
- `ChatWidget::ensure_builtin_command_enabled(cmd)` — checks map at dispatch time, shows error if disabled
- `BottomPane::set_builtin_command_disabled(cmd, reason)` — propagates to `ChatComposer` → `CommandPopup` for greyed-out rendering
- `CommandPopup::disabled_builtins` — prevents selection and dims disabled commands in the popup

### Cloud Connection Propagation
- `CloudConnectionInfo` flows: CLI → `App::run` → `ChatWidgetInit` → `spawn_agent()`
- `App.cloud_connection: Option<CloudConnectionInfo>` stored on App struct
- `ChatWidget` does NOT currently store `cloud_connection` — it's consumed at construction time for agent spawning only
- Need to add `is_cloud_session: bool` field to `ChatWidget` for runtime checks

### Slash Commands to Disable in Cloud Mode
Client-side only (need local filesystem/process/config access):
- `/settings` — local CLI config (theme, hotkeys, layout)
- `/init` — creates local AGENTS.md file
- `/browse` — opens local file manager
- `/diff` — runs local git diff
- `/mention` — references local files
- `/memory` — shows local instruction files
- `/mcp` — manages local MCP server connections
- `/browser` — launches local Chrome via CDP
- `/switch-skillset` — switches local skillsets

Keep enabled in cloud mode (interact with backend or are UI-only):
- `/agent`, `/model`, `/config`, `/approvals` — remote session config
- `/goal`, `/compact`, `/status`, `/first-prompt` — session state
- `/new` — start new session
- `/undo`, `/fork` — conversation operations (sent to backend)
- `/quit`, `/exit` — exit TUI
- `/login`, `/logout` — authentication

Disabled in cloud mode (added during implementation):
- `/resume`, `/resume-viewonly` — these pick from local transcripts which don't exist for cloud sessions

### Additional Gating Needed
1. **Auto-worktree setup** (`lib.rs:217-277`): skip when `cloud_connection.is_some()`
2. **Deferred spawn for skillset_per_session** (`App::run` lines 328-334): force `needs_deferred_spawn = false` in cloud mode
3. **Worktree warning** in App: suppress in cloud mode
