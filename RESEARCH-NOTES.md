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

### Prior Research (from sub-worktree)

- cwd fix: Removed hardcoded `/home/sprite/org/workspace` — the broker handles sprite-side cwd
- `source: "cli"`: Already sent in `broker/mod.rs:115`
- No CLI protocol changes needed — SACP is unchanged
- AcpTunnelManager is 3492 lines — the proxy interacts only through public API
- `sendPrompt()` uses `channelId`/`threadTs` — CLI maps both to `sessionId`
