# CLI Cloud Session Refactoring — Current Progress

## Completed

### CLI Side (nori-cli repo)
1. **cwd fix** — `spawn_and_relay.rs:26-30` hardcodes `/home/sprite/org/workspace` for cloud mode (commit `aaeb6990`)
2. **`source: "cli"` in acquire request** — `broker/mod.rs:115` sends `{"source": "cli"}` (pre-existing)
3. **E2E test script** — `scripts/cloud-e2e-test.sh` validates full flow through local broker to remote sprite
4. **No other CLI changes needed** — the SACP protocol is unchanged; the broker refactoring is invisible to the CLI

### Broker Side (nori-sessions repo, branch `cli-cloud-sessions`)

1. **ConnectorConfig expansion** (commit `a0d4a5a7`)
   - Added 4 optional hooks to `ConnectorConfig`: `fetchThreadContext`, `onSessionCreated`, `onSessionDestroyed`, `getDiagnostics`
   - Guarded all 5 Slack-specific call sites in AcpTunnelManager
   - Made `SlackUserResolver` nullable (Slack path only)
   - Tests: 3 new tests in `manager.test.ts` for optional hooks

2. **SACP proxy replaces dumb relay** (commit `2cf075a1`)
   - Created `cli-session.ts` — SACP proxy with state machine (awaiting_initialize → awaiting_session_new → ready → prompting → errored)
   - Created `cliLifecycle/index.ts` — CLI lifecycle module following Discord's pattern
   - Wired into `server.ts` and `main.ts`
   - Deleted `cli-tunnel.ts` and its tests
   - Tests: 8 new tests in `cli-session.test.ts` for SACP proxy behavior

3. **Documentation** (commit `5ca919bd`)
   - Updated all relevant noridocs files
   - Created new `cliLifecycle/docs.md`

## Not Yet Done

1. ~~**CLI-side cwd cleanup**~~ — **DONE.** Removed the hardcoded `/home/sprite/org/workspace` override from `spawn_and_relay.rs`. The CLI now uses its local cwd (`config.cwd`) for cloud sessions. The broker's SACP proxy handles sprite-side cwd via AcpTunnelManager. Updated the cloud test to assert cwd correctness.
2. **Broker PR** — The broker changes on `cli-cloud-sessions` branch need to be pushed and reviewed.
3. **E2E test script updates** — `scripts/cloud-e2e-test.sh` may need patch updates since the session creation flow changed (AcpTunnelManager creates the session instead of the CLI). However, the test patches only affect lifecycle management files (`base-version.ts`, `manager.ts`), not the tunnel/session code, so they should still work.
4. **`session/set_config_option` and `session/set_model` forwarding** — Currently stubbed with empty responses in the broker. Need pass-through methods on AcpTunnelManager to forward to underlying AcpClient. No CLI changes needed.
5. **Reconnection (V2)** — The broker-side plumbing enables reconnection (AcpTunnelManager holds session across CLI disconnects), but the CLI session picker UI is deferred.

## Pre-existing Issues (not caused by this work)
- 6 failures in `sessions-routes.test.ts` on the `cli-cloud-sessions` branch (onboarding session tests)
