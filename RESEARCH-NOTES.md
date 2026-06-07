# Research Notes — CLI Cloud Session Refactoring

## Key Findings

### Cloud Session Resume Gap (2026-06-07)

**Problem:** `AcpBackend::resume_session()` in `session.rs` always calls `SacpConnection::spawn()` (local agent process), ignoring `config.cloud_connection`. This means `/resume` does not work for cloud sessions.

**Root cause chain (3 files):**

1. `nori-rs/acp/src/backend/session.rs:20-42` — Calls `get_agent_config()` unconditionally (fails for cloud-only agents with no local registry entry), then calls `SacpConnection::spawn()` (always local).

2. `nori-rs/tui/src/chatwidget/agent.rs:502` — `spawn_acp_agent_resume()` hardcodes `cloud_connection: None` in the `AcpBackendConfig`. The function signature doesn't accept a `cloud_connection` parameter.

3. `nori-rs/tui/src/chatwidget/constructors.rs:156` — `ChatWidget::new_resumed_acp()` destructures `ChatWidgetInit` but discards `cloud_connection` with `_`.

**Data flow (working path for initial spawn):**
1. CLI args set `cloud_connection` on `App` (`app/mod.rs:282`)
2. `App::chat_widget_init()` passes it into `ChatWidgetInit` (`app/mod.rs:541`)
3. `ChatWidget::new()` captures it and passes to `spawn_acp_agent()` (`constructors.rs:76`)
4. `spawn_acp_agent()` puts it in `AcpBackendConfig` (`agent.rs:313`)
5. `AcpBackend::spawn()` branches on it (`spawn_and_relay.rs:28-56`)

**Data flow (broken resume path):**
1-2: Same as above
3. `ChatWidget::new_resumed_acp()` **discards** `cloud_connection` (`constructors.rs:156`)
4. `spawn_acp_agent_resume()` hardcodes `None` (`agent.rs:502`)
5. `resume_session()` never checks `config.cloud_connection` — always spawns locally

**Fix pattern:** Mirror the cloud-vs-local branching from `spawn_and_relay.rs:28-56` in `session.rs:20-42`. Make `agent_config` an `Option` (None for cloud). Thread `cloud_connection` through the resume path in the TUI layer.

**Error handling:** When `agent_config` is None (cloud mode), error messages that reference `agent_config.provider_info.name`, `.auth_hint`, `.display_name`, `.install_hint` need cloud-appropriate fallbacks. Pattern: `"Cloud session resume failed: {e}"`.

### Session Features Already Working for Cloud

1. **Transcript recording** — Both `spawn()` and `resume_session()` initialize `TranscriptRecorder` identically regardless of `config.cloud_connection`. Cloud sessions get full JSONL transcript recording.

2. **Session metadata (SessionMetaEntry)** — First line of JSONL transcript includes `acp_session_id`, which enables server-side resume. Works for cloud sessions.

3. **Client-side replay** — `transcript_to_replay_client_events()` and `transcript_to_summary()` are connection-agnostic. The replay entries are derived from the local transcript, not from the agent connection type.

4. **Server-side resume (`session/load`)** — The `supports_load_session` capability check at `session.rs:44` reads from connection capabilities, which should work regardless of local/cloud. However, the broker's SACP proxy may not forward `session/load` yet (it's listed as "expected to be unused" in V1 spec).

### Architecture Notes

- `SacpConnection::connect_remote()` signature: `pub async fn connect_remote(ws_url: &str, auth_token: &str, cwd: &Path) -> Result<Self>` (sacp_connection.rs:562)
- `CloudConnectionInfo` struct: `{ ws_url: String, auth_token: String }` (broker/mod.rs:12-15)
- Cloud sessions don't have local agent config — `get_agent_config()` will fail. Must be skipped for cloud mode.
- The `is_cloud` flag on `AcpBackend` (set from `config.cloud_connection.is_some()`) already correctly propagates through both spawn and resume paths.

### Cloud Session Metadata Gap (2026-06-07)

**Problem:** Cloud sessions are recorded to local transcripts identically to local sessions — there is no `is_cloud` field in `SessionMetaEntry`. This means:
- Session pickers cannot distinguish cloud from local sessions
- Users cannot tell which sessions were cloud-based when browsing `/resume`
- No warning when attempting to resume a cloud session from a non-cloud `nori` instance

**Scope of change (9 files + tests):**
1. `transcript/types.rs` — Add `is_cloud: bool` to `SessionMetaEntry` with `#[serde(default)]` for backward compat
2. `transcript/recorder.rs` — Accept `is_cloud: bool` param in `TranscriptRecorder::new()`, pass to `SessionMetaEntry`
3. `transcript/loader.rs` — Add `is_cloud: bool` to `SessionMetadata` and `SessionInfo`, propagate from `SessionMetaEntry`
4. `backend/spawn_and_relay.rs` — Pass `config.cloud_connection.is_some()` to `TranscriptRecorder::new()`
5. `backend/session.rs` — Same for resume path
6. `tui/src/nori/viewonly_session_picker.rs` — Add `is_cloud` to `SessionPickerInfo`, update `From<SessionMetadata>` impl
7. `tui/src/nori/resume_session_picker.rs` — Show `[cloud]` tag in session picker item
8. `tui/src/resume_picker/` — Surface cloud indicator in startup resume picker
9. `tui/src/app/event_handling.rs` — When resuming a cloud session without `cloud_connection`, show a non-blocking warning

**UX pattern (from research):** No major CLI tool uses a cloud badge in session pickers yet. Recommendation: append a short `[cloud]` text tag to the session row. Show a non-blocking warning on mode mismatch rather than preventing resume.

**Backward compatibility:** Old transcripts without `is_cloud` will deserialize with `is_cloud: false` via `#[serde(default)]`. No migration needed.

### Cloud Badge Gap in `/resume` and `/resume-viewonly` Pickers (2026-06-07)

**Problem:** The startup resume picker (at app launch) shows `[cloud]` badges via `metadata_to_row()` in `resume_picker/helpers.rs:14-18`. But the `/resume` and `/resume-viewonly` pickers both use `SessionPickerInfo` from `viewonly_session_picker.rs`, which does NOT carry `is_cloud`. Cloud sessions are listed but visually indistinguishable from local sessions.

**History:** `is_cloud` was added to `SessionPickerInfo` but then removed in commit `083e718d` because nothing read the field, triggering `dead_code` lint. The fix: re-add the field and actually USE it in the display path.

**Files to change:**
1. `tui/src/nori/viewonly_session_picker.rs` — Add `is_cloud: bool` to `SessionPickerInfo`, update `From<SessionMetadata>`, update `load_session_previews()` to carry `is_cloud` from `SessionInfo`, and append `[cloud]` to session name in `viewonly_session_picker_params()`
2. `tui/src/nori/resume_session_picker.rs` — Append `[cloud]` to session name in `resume_session_picker_params()`
3. Tests for both pickers

**Data flow:**
- `SessionMetadata.is_cloud` ← `SessionMetaEntry.is_cloud` (loader.rs:525)
- `SessionInfo.is_cloud` ← `SessionMetaEntry.is_cloud` (loader.rs:513)
- Both are available when constructing `SessionPickerInfo`

### V1 Feature Parity Audit (2026-06-07)

**Finding:** All session features have been verified to work for cloud sessions. No gaps exist on the CLI side.

**Verified features (cloud parity confirmed):**
1. **Transcript recording** — `TranscriptRecorder::new()` at `spawn_and_relay.rs:155` and `session.rs:404` both pass `config.cloud_connection.is_some()` as `is_cloud`
2. **Session metadata** — `SessionMetaEntry.is_cloud` with `#[serde(skip_serializing_if, default)]` for backward compat
3. **Server-side resume** — `session.rs:84-254` uses `session/load` when `acp_session_id` available and agent supports it, regardless of cloud/local
4. **Client-side replay fallback** — `session.rs:255-316` creates fresh session with transcript summary, same for both paths
5. **Session pickers** — `[cloud]` badge in all three: startup (`helpers.rs:14-15`), `/resume` (`resume_session_picker.rs:46-48,104-109`), `/resume-viewonly` (`viewonly_session_picker.rs:228-230`)
6. **Cloud disconnect** — `spawn_and_relay.rs:349-361` emits informative error; `event_handling.rs:1132-1136` warns on mode mismatch during resume
7. **Session release on exit** — `main.rs:596-605` calls `broker.release_session()` with 5s timeout
8. **MCP registration** — `spawn_and_relay.rs:63-76` registers MCP servers outside the cloud/local branch
9. **Thread goals** — replayed from transcript identically for both paths
10. **Ghost snapshots (/undo)** — same infrastructure used for both paths
11. **Hooks** — all hook fields shared in `AcpBackendConfig`

**Known non-parity (by design, not gaps):**
- `WireLogger` only available for local spawn (cloud uses WebSocket, no process stdio)
- `agent_config` is `None` for cloud — enhanced error messages fall through to generic cloud messages
- No auto-re-acquire of cloud connection on resume — user must launch `nori cloud` again

**No "session manifest" concept exists in the codebase.** The closest equivalent is `SessionMetaEntry` (first JSONL line of transcript files). The user's term maps to this.

### Prior Research (from sub-worktree)

- cwd fix: Removed hardcoded `/home/sprite/org/workspace` — the broker handles sprite-side cwd
- `source: "cli"`: Already sent in `broker/mod.rs:115`
- No CLI protocol changes needed — SACP is unchanged
- AcpTunnelManager is 3492 lines — the proxy interacts only through public API
- `sendPrompt()` uses `channelId`/`threadTs` — CLI maps both to `sessionId`
