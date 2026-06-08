# Current Progress

## Status: All CLI-side items complete

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
- Cloud-enabled commands: `/agent`, `/model`, `/config`, `/approvals`, `/goal`, `/new`, `/compact`, `/undo`, `/status`, `/first-prompt`, `/quit`, `/exit`, `/login`, `/logout`, `/fork`
- Cloud restrictions re-applied after `SessionCapabilitiesChanged` events to ensure local-only commands stay disabled even if the backend sends new capabilities

### Completed: Slack/Discord Session Resumption (Spec Item 3)

**What was done:**
- Naturally supported by item 1 — the session picker displays all sessions from the broker, with a `source` field (cli, slack, discord, etc.) shown next to each entry
- `CloudSessionSummary.source` field differentiates session origins
- No additional CLI code needed; the `format_cloud_session_list()` function already renders `(source)` per session

**Design decisions:**
- No CLI-side filtering by source — all sessions are shown regardless of origin
- The broker is responsible for returning sessions from all sources

### Broker-side work needed (nori-sessions repo — separate PR)
- `GET /api/sessions` endpoint returning `Vec<CloudSessionSummary>` with sessions from all sources (cli, slack, discord)
- `POST /api/sessions/{id}/resume` endpoint returning `SessionInfo { session_id, ws_url }`

## Status: All CLI-side items complete

All three spec items are implemented on the CLI side. The broker-side work (nori-sessions repo) is tracked separately.
