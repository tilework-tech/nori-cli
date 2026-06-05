# Research Notes — CLI Cloud Session Refactoring

## Key Findings

### CLI Side (nori-cli) — Complete

1. **cwd fix**: Already committed (`aaeb6990`). `spawn_and_relay.rs:26-30` hardcodes `/home/sprite/org/workspace` for cloud mode. This remains correct until the broker refactoring is complete — the broker will eventually set cwd via AcpTunnelManager.

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
