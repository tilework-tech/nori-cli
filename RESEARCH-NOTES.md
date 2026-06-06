# Research Notes — CLI Cloud Session Refactoring

## Key Findings

### CLI Side (nori-cli) — Complete

1. **cwd fix**: Commit `aaeb6990` added a hardcoded `/home/sprite/org/workspace` for cloud mode. This has now been removed — the broker's SACP proxy (commit `2cf075a1` on `cli-cloud-sessions`) handles sprite-side cwd via AcpTunnelManager. The CLI now uses its local cwd.

2. **`source: "cli"`**: Already sent in `broker/mod.rs:115` as `json!({"source": "cli"})`.

3. **No other CLI changes needed for V1** — the SACP protocol is unchanged. The broker's internal change from relay to proxy is invisible to the CLI.

### Broker Side (nori-sessions) — Work Required

**Branch**: `cli-cloud-sessions` (exists locally and on remote). Has the dumb relay (`cli-tunnel.ts`, 225 lines) and basic tests (315 lines).

**Existing infrastructure on cli-cloud-sessions branch:**
- `src/inbound/ws/cli-tunnel.ts` — dumb bidirectional WebSocket relay
- `src/inbound/http/endpoints/cliAuth.ts` — CLI auth endpoint
- `src/features/lifecycle/claimedBy.ts` — `cliClaimedBy()` already exists
- `test/inbound/ws/cli-tunnel.test.ts` — relay tests
- `test/features/cli-claimed-by.test.ts` — claim format tests

**What needs to change:**

#### 1. Expand ConnectorConfig (manager.ts:370-386)

Current interface has 4 fields. Need to add 4 optional hooks:
- `fetchThreadContext?` — guards `slackProvider.fetchThreadReplies()` at line ~837
- `onSessionCreated?` — guards `writeSlackProxyEnv()` at line ~2060
- `onSessionDestroyed?` — guards `revokeSlackAccessGrantsForSprite()` at line ~1673
- `getDiagnostics?` — guards `slackProvider.getDiagnostics()` at lines ~1611, ~2443

Also need to guard `SlackUserResolver` creation (constructor line ~511-514) — only create when no connector is provided.

#### 2. Create cliLifecycle module (new)

Following Discord's pattern in `features/discordLifecycle/index.ts`:
- Build CLI-specific `ConnectorConfig`
- Create `AcpTunnelManager` instance
- Register lifecycle event handlers (`addOnIdleReclaim`, `addOnBrokerAction`)
- Wire credential rotation events

#### 3. Replace cli-tunnel.ts with cli-session.ts

Replace the dumb relay with an SACP proxy state machine:
- States: `awaiting_initialize → awaiting_session_new → ready → prompting → ready | errored`
- Method dispatch table for 7 SACP methods + 1 notification type
- Route `session/prompt` through `manager.sendPrompt()`
- Stream `onProgress` notifications back as SACP `session/update` frames

#### 4. AcpTunnelManager access gaps

- `sendPrompt()` uses `channelId`/`threadTs` — CLI maps both to `sessionId`
- No public method to forward `session/set_config_option`, `session/set_model`, `session/load` to underlying AcpClient — may need new pass-through methods or direct client access

### Root Cause: "Failed to create ACP session" in Cloud Mode (2026-06-06)

**Error chain:** `Failed to start agent 'claude-code'` → `Failed to spawn ACP agent` → `Cloud session creation failed` → `Failed to create ACP session`

**Root cause:** JSON field name mismatch in the broker's `session/new` response.

The broker proxy (`cli-session.ts:280`) sends:
```json
{"jsonrpc":"2.0","id":2,"result":{"session_id":"nori-abc"}}
```

But the ACP schema's `NewSessionResponse` (agent-client-protocol-schema v0.11.6) uses `#[serde(rename_all = "camelCase")]`, so the CLI deserializer expects:
```json
{"jsonrpc":"2.0","id":2,"result":{"sessionId":"nori-abc"}}
```

The `session_id` field on `NewSessionResponse` is required (no default), so deserialization fails when the camelCase key is missing.

**Fix:** Change `{ session_id: sessionId }` to `{ sessionId }` on line 280 of `cli-session.ts` in the broker repo.

**Secondary issues (non-blocking):**
- The `initialize` response sends `serverCapabilities`/`serverInfo` instead of `agentCapabilities`/`agentInfo`, but these have `#[serde(default)]` so they default to empty — no failure, just missing capability data.
- The `session/prompt` response sends `{ stopReason, text }` — `stopReason` matches the expected camelCase field name, `text` is ignored (unknown fields are silently dropped by serde). The `StopReason` enum uses `#[serde(rename_all = "snake_case")]` so values must be like `"end_turn"`, which should match what `AcpTunnelManager.sendPrompt()` returns.

**E2E test gap:** The existing e2e test (`cloud-e2e-test.sh`) doesn't catch this because it doesn't actually exercise the SACP protocol — the script checks for TUI prompt appearance and text responses but doesn't verify the underlying protocol exchanges. A proper e2e test must actually run `nori cloud` through the full session lifecycle: connect → initialize → session/new → session/prompt → close → re-acquire.

### Test Infrastructure

- Broker uses `bun:test` with `mock.module()` pattern
- Tests in `test/` directory mirroring `src/` structure
- `MockAcpClient extends EventEmitter` with mock methods
- `createStubLifecycleManager()` helper for HTTP/WS endpoint tests
- Custom test runner (`scripts/run-tests.ts`) spawns each test file in isolated subprocess
- Test utilities: `mocked()`, `waitFor()`, `lazyMock()`, `flushMicrotasks()`

### Architecture Notes

- AcpTunnelManager is 3492 lines — the proxy interacts only through public API
- `sendPrompt()` signature: `(channelId, threadTs, text, options?) → Promise<SessionPromptResult>`
- `onProgress` callback in `sendPrompt` options is how `session/update` notifications stream
- Discord uses `channelId` for both `channelId` and `threadTs` — CLI should do the same with `sessionId`
- JSON-RPC uses `jsonrpc: '2.0'` despite "SACP v11" naming (v11 is crate version)

### CLI cwd Override Removal — Research (2026-06-05)

**Context:** `spawn_and_relay.rs:26-30` hardcodes `/home/sprite/org/workspace` for cloud mode. Now that the broker's SACP proxy (commit `2cf075a1`) handles session creation via AcpTunnelManager, this hardcode should be removed.

**Data flow for `config.cwd` in cloud mode:**
1. User runs `nori cloud [--cd /path]`
2. `Config::load_with_cli_overrides` resolves cwd to `env::current_dir()` or `--cd` value
3. `AcpBackendConfig.cwd` gets the local path (set in `tui/src/chatwidget/agent.rs:279`)
4. `spawn_and_relay.rs:26-30` currently overrides this to `/home/sprite/org/workspace`

**Three uses of `cwd` after the override:**
1. `SacpConnection::connect_remote(ws_url, auth_token, &cwd)` → `establish_connection` → sets up client-side handlers for `request_permission`, `write_text_file`, `read_text_file`. With the SACP proxy, the broker auto-approves permissions and doesn't advertise file capabilities, so these handlers are never invoked. The current hardcode is actually *wrong* for these handlers — they'd resolve paths against a sprite path on the local machine.
2. `connection.create_session(&cwd, mcp_servers)` → sends `session/new` with cwd. The broker's SACP proxy discards this and uses its own sprite-appropriate path via AcpTunnelManager.
3. Local display: `TranscriptRecorder`, `SessionConfiguredEvent.cwd`, `backend.cwd` — should use local path for correct TUI display.

**Test impact:** Three cloud tests in `part6.rs` — none assert on cwd value, all pass unchanged.

**Conclusion:** Removing the hardcode is safe and actually an improvement. The local cwd is correct for all three usage paths under the SACP proxy architecture.
