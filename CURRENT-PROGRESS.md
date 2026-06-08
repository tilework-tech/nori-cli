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

### Completed: Cloud Mode Feature Gating (Spec Item 2)

**What was done:**
- Added `SlashCommand::available_in_cloud_mode()` method with exhaustive match (no wildcard) classifying each command
- 11 client-only commands disabled in cloud mode: `/settings`, `/init`, `/browse`, `/diff`, `/mention`, `/memory`, `/mcp`, `/browser`, `/switch-skillset`, `/resume`, `/resume-viewonly`
- Created `chatwidget/cloud_mode.rs` with `apply_cloud_mode_restrictions()` that uses existing `CommandAvailability`/`disabled_builtins` infrastructure
- Added `is_cloud_session: bool` field to `ChatWidget`, set from `cloud_connection.is_some()` at construction
- Belt-and-suspenders dispatch guard in `dispatch_command()` blocks cloud-disabled commands even via direct entry
- Auto-worktree setup skipped entirely when `cloud_connection.is_some()` in `lib.rs`
- Deferred skillset-per-session spawn disabled in cloud mode in `App::run()`
- Added 2 unit tests for command classification
- Updated TUI docs

**Design decisions:**
- Reuses existing `CommandAvailability` infrastructure rather than a new mechanism
- Exhaustive match forces classification of new commands at compile time
- Cloud-only commands kept enabled: `/agent`, `/model`, `/config`, `/approvals`, `/goal`, `/new`, `/resume`, `/compact`, `/status`, `/quit`, etc.
- Backend capabilities are authoritative — if the remote backend sends `SessionCapabilitiesView`, it overwrites local cloud-mode settings

### Remaining: Item 3

**Item 3**: Resume Slack/Discord sessions via CLI. This is naturally supported by item 1 — if the broker returns all sessions regardless of source, the `source` field on `CloudSessionSummary` shows where each session originated. The broker-side implementation needs to return these sessions.

### Broker-side work needed (nori-sessions repo)
- `GET /api/sessions` endpoint returning `Vec<CloudSessionSummary>` with sessions from all sources
- `POST /api/sessions/{id}/resume` endpoint returning `SessionInfo { session_id, ws_url }`
