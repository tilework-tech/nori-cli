# CLI Cloud Session Refactoring — APPLICATION SPEC

**Goal:** Refactor `nori cloud` so the broker manages the ACP session to the sprite via `AcpTunnelManager` (like Discord/Slack integrations), instead of acting as a dumb WebSocket relay, while preserving the existing CLI TUI experience and e2e test.

**Architecture:** The broker replaces its transparent frame relay (`cli-tunnel.ts`) with a `cliLifecycle` module that creates an `AcpTunnelManager` instance with a CLI-specific `ConnectorConfig`. The CLI continues to speak SACP over WebSocket to the broker, but the broker now interprets SACP frames, routes prompts through `AcpTunnelManager.sendPrompt()`, and forwards `session/update` notifications back as SACP frames. This gives CLI sessions the same transcript recording, lifecycle management, transport recovery, and connection-drop resilience that Discord/Slack sessions already have — with no change to the CLI's transport protocol.

**Tech Stack:** TypeScript (broker), Rust (CLI), ACP/SACP JSON-RPC v11, WebSocket (`ws`/`tokio-tungstenite`)

---

## Problem Statement

The current `nori cloud` implementation has the broker act as a dumb bidirectional WebSocket relay between the CLI and sprite:

```
CLI (SACP client) → WS → Broker (cli-tunnel.ts: frame relay) → WS → Sprite (ACP bridge)
```

This architecture prevents:
1. **Transcript recording** — broker sees raw bytes, not semantic events; no `TranscriptRecorder` is attached
2. **Connection drop resilience** — CLI disconnect closes the sprite WS immediately; session state is lost
3. **Lifecycle management** — CLI sessions don't participate in the broker's GC sweep, idle timeouts, or claim management
4. **Session persistence** — no checkpoint orchestration, no resume capability
5. **Unified session model** — CLI sessions use a fundamentally different code path from Discord/Slack, making the session source visible in implementation rather than abstracted away

Discord and Slack integrations solve all of these by routing sessions through `AcpTunnelManager`. The CLI integration should do the same.

---

## Design

### Core Architecture Change

Replace the transparent relay with a broker-managed ACP session routed through `AcpTunnelManager`:

```
CLI (SACP client) → WS → Broker (SACP proxy → AcpTunnelManager → AcpClient) → WS → Sprite (ACP bridge)
```

The broker acts as an **SACP proxy**: it accepts SACP requests from the CLI, translates them into `AcpTunnelManager` / `AcpClient` calls, and forwards `session/update` notifications back as SACP frames. The CLI is unaware of the mediation — the protocol is identical from its perspective.

### Using AcpTunnelManager (Not a Separate Manager)

The `AcpTunnelManager` is ~95% integration-agnostic. Its core pipeline — lifecycle claim, sprite connection, AcpClient management, prompt locking, transport recovery, GC sweep, credential rotation — is fully generic and already used by both Slack and Discord.

The Slack-specific leaks are concentrated in 5 call sites:

| Leaked Slack Code | Location | Fix |
|---|---|---|
| `slackProvider.fetchThreadReplies()` in context replay | `sendPrompt()` body | Guard behind `ConnectorConfig.fetchThreadContext?` callback |
| `writeSlackProxyEnv()` | `createSession()` | Guard behind `ConnectorConfig.onSessionCreated?` callback |
| `revokeSlackAccessGrantsForSprite()` | `destroySession()` | Guard behind `ConnectorConfig.onSessionDestroyed?` callback |
| `SlackUserResolver` creation | Constructor | Make conditional on `ConnectorConfig` presence |
| Slack diagnostics in debug/trace output | `getDebugInfo()`, `captureResumeFailureDiagnostics()` | Guard behind `ConnectorConfig.getDiagnostics?` callback |

These fixes also correct Discord's silent inheritance of Slack code paths — good hygiene regardless.

The expanded `ConnectorConfig` interface:

```typescript
export interface ConnectorConfig {
  // Existing (identity computation)
  computeSpriteId: (args: { channelId: string; threadTs: string; provider: string }) => string;
  computeOriginKey: (args: { channelId: string; threadTs: string; provider: string }) => string;
  computeSpriteIdPrefix: (args: { channelId: string; threadTs: string }) => string;
  originType: string;

  // New optional hooks (abstracting Slack-specific call sites)
  fetchThreadContext?: (args: { channelId: string; threadTs: string }) => Promise<string | null>;
  onSessionCreated?: (args: { spriteName: string; channelId: string; threadTs: string }) => Promise<void>;
  onSessionDestroyed?: (args: { spriteName: string }) => void;
  getDiagnostics?: () => unknown;
}
```

CLI's `ConnectorConfig`:

```typescript
const buildCliConnectorConfig = (): ConnectorConfig => ({
  computeSpriteId: ({ channelId, provider }) => `cli-${channelId}-${provider}`,
  computeOriginKey: ({ channelId, provider }) => cliClaimedBy({ sessionId: channelId, agent: provider }),
  computeSpriteIdPrefix: ({ channelId }) => `cli-${channelId}-`,
  originType: 'cli',
  // No fetchThreadContext — CLI has no thread history to replay
  // No onSessionCreated — no Slack proxy env needed
  // No onSessionDestroyed — no Slack access grants to revoke
});
```

### SACP Translation Layer

The broker translates between CLI SACP frames and `AcpTunnelManager` / `AcpClient` calls.

**Complete SACP method coverage** (7 outbound methods the CLI sends, 1 inbound notification type):

| CLI sends (SACP request/notification) | Broker action |
|---|---|
| `initialize` | Respond directly with cached server capabilities (AcpClient already initialized during WS setup) |
| `session/new` | Respond with cached session ID (AcpTunnelManager creates session lazily on first prompt) |
| `session/prompt` | Route through `manager.sendPrompt(sessionId, sessionId, text, { onProgress })`. Stream each `onProgress` notification back to CLI as a SACP `session/update` frame. On completion, send SACP response with `{ stopReason, text }` |
| `session/cancel` (notification) | Forward to underlying `AcpClient.cancel()` |
| `session/set_config_option` | Forward to underlying `AcpClient` |
| `session/set_model` | Forward to underlying `AcpClient` |
| `session/load` | Forward to underlying `AcpClient` (for future resume support) |

| Sprite sends (via AcpClient) | Broker action |
|---|---|
| `session/update` notification | Forward as SACP `session/update` notification to CLI |
| `request_permission` request | Auto-approved by AcpClient (same as Discord/Slack — sprite is sandboxed) |

**Tool call approval: auto-approve is correct.** The sprite is sandboxed. The CLI user still sees tool calls via `session/update` notifications (`ToolCall`, `ToolCallUpdate` variants) but doesn't get the interactive approval step. This matches Discord/Slack behavior. No AcpClient modification needed.

**File operations (`fs/write_text_file`, `fs/read_text_file`): not applicable.** The broker's AcpClient doesn't advertise these capabilities in `initialize`, so the agent won't send them. The agent has direct filesystem access on the sprite.

### Connection Drop Handling

**CLI disconnects:**
1. Broker detects WebSocket close
2. Broker keeps `AcpClient` and sprite connection alive via AcpTunnelManager
3. If a prompt is in flight, it continues executing on the sprite
4. Session transitions to idle, subject to standard `sessionInactivityMs` timeout (15 min)
5. If timeout expires: AcpTunnelManager destroys session, sprite is released

**Sprite disconnects:**
1. AcpTunnelManager detects transport drop
2. Attempts transport recovery (reconnect WS, resume ACP session — existing recovery logic)
3. If recovery succeeds: transparent to CLI
4. If recovery fails: forward error to CLI, mark session as errored

### Transcript Recording

Attach `TranscriptRecorder` to the `AcpClient` instance during session creation. This is the **same integration path** used by Discord/Slack — no custom frame parsing needed. The recorder listens for `prompt/start`, `prompt/end`, `session/update`, and `close` events on the AcpClient and writes JSONL to S3 at `transcripts/org=<orgId>/session=<sessionId>.jsonl`.

### Lifecycle Integration

- **Claim identity**: `cli:<email>/<iso-timestamp>` (already implemented via `cliClaimedBy()`)
- **Idle timeout**: Standard lifecycle GC sweep marks idle CLI sessions for cleanup (15 min)
- **Activity tracking**: `markActive()` on each prompt via AcpTunnelManager (same as Discord/Slack)
- **Release**: Explicit via `POST /sessions/:id/release` or implicit via idle timeout
- **Credential rotation**: CLI sessions participate in `restartIdleSessionsForCredentialRotation()` automatically through AcpTunnelManager

### Reconnection (Deferred — V2)

The broker-side plumbing naturally enables reconnection: AcpTunnelManager holds the session across CLI disconnects. A future `nori cloud` session picker UI can reconnect to surviving sessions. Not in V1 scope.

---

## Changes Required

### nori-sessions (broker)

#### 1. Expand `ConnectorConfig` interface in `features/acp-tunnel/manager.ts`

Add optional callback hooks (see interface above). Guard each Slack-specific call site:

- Context replay (lines ~837-846): `if (this.connector?.fetchThreadContext) { ... } else if (slackProvider) { ... }`
- `writeSlackProxyEnv` (lines ~2057-2060): `if (this.connector?.onSessionCreated) { await this.connector.onSessionCreated(...) } else { writeSlackProxyEnv(...) }`
- `revokeSlackAccessGrantsForSprite` (line ~1669): `if (this.connector?.onSessionDestroyed) { this.connector.onSessionDestroyed(...) } else { revokeSlackAccessGrantsForSprite(...) }`
- `SlackUserResolver` (lines ~491, 511-514): Create only when no connector is provided
- Slack diagnostics (lines ~1611-1613, 2443-2444): `this.connector?.getDiagnostics?.() ?? slackProvider.getDiagnostics()`

Update Slack and Discord lifecycle modules to provide the new callbacks where applicable (Slack: provide `fetchThreadContext`, `onSessionCreated`, `onSessionDestroyed`; Discord: provide what it needs, omit the rest). This is a refactoring step with no behavior change for existing integrations.

#### 2. New module: `features/cliLifecycle/`

**Files:**
- `features/cliLifecycle/index.ts` — `connectCli()` / `closeCli()` lifecycle functions

**`connectCli()` setup (following Discord pattern):**
- Build `ConnectorConfig` with CLI identity computation
- Create `AcpTunnelManager` instance with lifecycle reference and connector config
- Register lifecycle event handlers (`addOnIdleReclaim`, `addOnBrokerAction`)
- Wire credential rotation events
- Call `attachCliSession({ manager })` to register the WebSocket handler
- Preserved across broker reconnects (same as Discord/Slack)

#### 3. Refactor: `inbound/ws/cli-tunnel.ts` → `inbound/ws/cli-session.ts`

Replace the dumb relay with an SACP proxy. The WebSocket upgrade flow stays mostly the same (auth, session validation). What changes:

**Current (`cli-tunnel.ts`):**
1. Auth + session validation
2. Connect WS to sprite directly
3. `relay()` — forward frames bidirectionally between CLI WS and sprite WS

**New (`cli-session.ts`):**
1. Auth + session validation (unchanged)
2. Accept CLI WebSocket upgrade
3. Start SACP translation loop:
   - Parse incoming JSON-RPC frames from CLI
   - Route `session/prompt` → `manager.sendPrompt()`, stream `onProgress` back
   - Handle `initialize`, `session/new` locally (respond with cached data)
   - Forward `session/cancel`, `session/set_config_option`, `session/set_model` to AcpClient
   - Forward `session/update` notifications from AcpClient → CLI as SACP frames
4. On CLI disconnect: AcpTunnelManager keeps session alive (no explicit teardown)
5. On session idle timeout: AcpTunnelManager destroys session automatically

The SACP proxy is a small state machine: `awaiting_initialize → awaiting_session_new → ready → prompting → ready | errored`.

The sprite WS connection, AcpClient creation, and ACP session initialization are all handled internally by AcpTunnelManager when it lazy-creates the session on the first `sendPrompt()` call. The proxy does not connect to the sprite directly.

#### 4. Remove `cli-tunnel.ts`

The dumb relay file is replaced entirely by `cli-session.ts`. Delete it.

#### 5. Update `inbound/http/endpoints/sessions.ts` (optional)

Consider including `cwd` (sprite workspace path) in the acquire response. The broker knows the sprite's workspace layout. This would let the CLI use it if needed, rather than hardcoding paths on either side.

#### 6. Tests

**`cli-session.test.ts`** — SACP proxy behavior:
- `initialize` request receives valid capabilities response
- `session/new` request receives session ID response
- `session/prompt` routes through AcpTunnelManager and streams `session/update` notifications back to CLI
- `session/cancel` notification reaches AcpClient
- `session/set_config_option` reaches AcpClient
- Unknown SACP methods receive JSON-RPC "method not found" error
- CLI disconnect does not immediately destroy AcpTunnelManager session

**`connector-config.test.ts`** — ConnectorConfig abstraction:
- Slack-specific call sites use callback when ConnectorConfig provides one
- Slack-specific call sites fall through to Slack defaults when no ConnectorConfig callback
- CLI ConnectorConfig produces correct sprite IDs and origin keys

**Migrate `cli-tunnel.test.ts`** — existing relay tests become irrelevant; replace with proxy tests above.

### nori-cli (CLI)

CLI changes are **minimal** because the broker continues to accept SACP. The CLI is unaware of the mediation.

#### 1. Remove hardcoded cloud cwd

**File:** `nori-rs/acp/src/backend/spawn_and_relay.rs` (lines 26-30)

The hardcoded `/home/sprite/org/workspace` override for cloud mode can be removed. The broker now sets the cwd when creating the ACP session on the sprite via AcpTunnelManager. The CLI's `session/new` cwd is acknowledged but not forwarded; the broker uses a sprite-appropriate path.

If the acquire response includes the sprite workspace path (see sessions.ts change above), the CLI can use it instead of hardcoding. Either approach works.

#### 2. Verify `source: "cli"` in acquire request

**File:** `nori-rs/acp/src/broker/mod.rs`

Ensure `acquire_session()` sends `{ "source": "cli" }` in the request body so `cliClaimedBy()` fires on the broker side. (This appears to already be implemented based on prior commit history.)

#### 3. No other CLI changes needed for V1

The SACP protocol is unchanged. The CLI still:
- Connects WebSocket to the broker's `/api/sessions/:id/ws` endpoint
- Sends `initialize` → `session/new` → `session/prompt` as before
- Receives `session/update` notifications as before
- Sends `session/cancel` to interrupt as before

The broker's internal change from relay to proxy is invisible.

---

## What the E2E Test Validates

The existing e2e test (`scripts/cloud-e2e-test.sh`):
1. Starts broker, acquires sprite, launches `nori cloud`
2. Sends "say hello world" → asserts "Hello" in response
3. Sends "what is 2 plus 2" → asserts "4" in response

After refactoring, the test continues to pass because the CLI still speaks SACP to the broker WebSocket endpoint. The broker's internal change from relay to AcpTunnelManager-mediated proxy is invisible to the CLI.

**Additional test coverage needed:**
- Transcript appears in S3 after a CLI session (integration test)
- CLI disconnect does not immediately release the sprite (broker unit test)
- Idle timeout eventually releases the sprite (broker unit test)

---

## Edge Cases

1. **Concurrent prompts**: AcpClient only supports one active prompt. The SACP proxy rejects a second `session/prompt` with a JSON-RPC error if one is already in flight. The TUI already enforces this, but the broker should enforce it defensively.

2. **Unknown SACP methods**: The proxy responds with JSON-RPC "method not found" for unrecognized methods. This is forward-compatible — if the CLI adds new methods, the broker explicitly rejects rather than silently dropping.

3. **Sprite restarts mid-session**: AcpTunnelManager's transport recovery kicks in automatically (reconnect WS, resume ACP session). If recovery succeeds, the CLI is unaware. If it fails, the proxy sends an error event to the CLI.

4. **Auth token expiry during session**: Firebase token is validated at WebSocket upgrade time only. Long-running sessions continue uninterrupted. This matches Discord/Slack behavior.

5. **Broker restart during session**: All sessions are lost. Same as current behavior and same as Discord/Slack. Future work: pending turn resume sweep (like Slack's `runPendingTurnResumeSweep()`).

6. **CLI sends `session/load` (resume)**: For V1, this can be forwarded to AcpClient but is expected to be unused. The plumbing exists for V2 session resume.

---

## Backwards Compatibility

- **CLI → broker protocol**: SACP over WebSocket — unchanged. Existing CLIs work with the new broker.
- **Broker → sprite protocol**: ACP over WebSocket via AcpClient — same path as Discord/Slack.
- **HTTP API**: `/sessions/acquire` and `/sessions/:id/release` — unchanged.
- **WebSocket endpoint**: `/api/sessions/:id/ws` — same path, same auth. Internal behavior changes but protocol is identical.
- **Lifecycle claims**: `cliClaimedBy()` format — unchanged.
- **Discord/Slack behavior**: ConnectorConfig expansion is additive. Existing integrations continue to work. Slack-specific code paths are guarded behind `connector?.callback ?? existingSlackDefault` pattern, preserving behavior when no connector callback is provided.

---

**Testing Details:** Tests verify behavior at the SACP proxy boundary (correct routing of each SACP method through AcpTunnelManager), connection drop resilience (CLI disconnect keeps session alive), lifecycle integration (idle timeout releases sprite), and ConnectorConfig abstraction (Slack code paths are properly guarded). All tests are blackbox against the proxy/manager interface.

**Implementation Details:**
- CLI sessions route through the same `AcpTunnelManager` pipeline as Discord/Slack — unified session model
- SACP proxy is a state machine: `awaiting_initialize → awaiting_session_new → ready → prompting → ready | errored`
- ConnectorConfig expansion is 5 optional callbacks, each guarding one Slack-specific call site
- Tool approval is auto-approved (same as Discord/Slack) — no AcpClient modification needed
- CLI code changes are ~10 lines (remove hardcoded cwd)
- E2E test continues passing without modification
- Transcript recording comes free from AcpClient/TranscriptRecorder integration — no custom frame parsing
- Reconnection/session picker is architecturally enabled but deferred to V2

**Open Questions:**
1. `AcpTunnelManager.sendPrompt()` uses `channelId`/`threadTs` as routing keys. For CLI sessions, both map to `sessionId`. Should we rename these parameters in the manager signature, or accept the Slack-ism and document the mapping? (Renaming is cleaner but touches a lot of call sites.)
2. `session/set_config_option` and `session/set_model` need to reach the underlying `AcpClient`. Does AcpTunnelManager expose a way to access the client for a given session, or do we need to add pass-through methods?
3. The e2e test script (`scripts/cloud-e2e-test.sh`) patches the broker to skip provisioning. After this refactoring, the session creation flow changes (AcpTunnelManager creates the session instead of the CLI). The e2e test patches may need updating — confirm they still bypass correctly.
