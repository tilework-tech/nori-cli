# Current Progress

## Status: Item 1 implemented (CLI side)

### Completed: Cloud Session Selection (Spec Item 1)

**What was done:**
- Added `CloudSessionSummary` type and `BrokerError::ListFailed`/`ResumeFailed` variants to `acp/src/broker/mod.rs`
- Added `BrokerClient::list_sessions()` (GET /api/sessions) and `BrokerClient::resume_session()` (POST /api/sessions/{id}/resume) methods
- Created `cli/src/cloud.rs` with session selection UI: `format_cloud_session_list()`, `parse_session_choice()`, `prompt_session_selection()`
- Modified `cli/src/main.rs` cloud handler to list sessions, show picker, and route to resume or acquire
- Added 9 integration tests for broker methods and 8 unit tests for CLI selection logic
- Updated broker, CLI, and ACP docs

**Design decisions:**
- Pre-TUI session picker (stdin/stderr) — TUI needs WebSocket URL before it can launch
- Graceful degradation on 404 (broker doesn't support listing yet) — falls back to new session
- Non-interactive terminals skip picker and use old behavior (auto-acquire)
- No new crate dependencies

### Remaining: Items 2 and 3

**Item 2**: Disable client-side features in cloud mode (settings, slash commands, worktree, skillset switching). No feature gating currently exists in the TUI for cloud mode. Needs a new mechanism to conditionally hide/disable features.

**Item 3**: Resume Slack/Discord sessions via CLI. This is naturally supported by item 1 — if the broker returns all sessions regardless of source, the `source` field on `CloudSessionSummary` shows where each session originated. The broker-side implementation needs to return these sessions.

### Broker-side work needed (nori-sessions repo)
- `GET /api/sessions` endpoint returning `Vec<CloudSessionSummary>` with sessions from all sources
- `POST /api/sessions/{id}/resume` endpoint returning `SessionInfo { session_id, ws_url }`
